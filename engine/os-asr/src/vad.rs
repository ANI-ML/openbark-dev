use serde::Serialize;
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use crate::{AsrError, Audio, SAMPLE_RATE, require_file};

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct SpeechSpan {
    pub start: f32,
    pub end: f32,
}

#[derive(Debug, Clone)]
pub struct VadOptions {
    pub model: String,
    pub threshold: f32,
    pub min_silence_secs: f32,
    pub min_speech_secs: f32,
    /// Longer speech is split; keeps ASR chunks well inside model limits.
    pub max_speech_secs: f32,
    /// Detect speech on a levelled copy of the audio ([`crate::level`]), so a
    /// quiet voice (a phone on speaker across the room) is heard as speech.
    /// Utterances still carry the original samples.
    pub level: bool,
    /// Audio kept before and after each detected utterance, so a first or
    /// last word the detector heard late or let go early isn't cut.
    pub pad_before_secs: f32,
    pub pad_after_secs: f32,
}

impl VadOptions {
    pub fn new(model: impl Into<String>) -> Self {
        let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(d);
        Self {
            model: model.into(),
            threshold: env("OPENBARK_X_VAD_THRESHOLD", 0.5),
            min_silence_secs: env("OPENBARK_X_MIN_SILENCE", 0.3),
            min_speech_secs: env("OPENBARK_X_MIN_SPEECH", 0.25),
            max_speech_secs: 20.0,
            level: env("OPENBARK_X_LEVEL", 0.0) > 0.0,
            pad_before_secs: env("OPENBARK_X_PAD_BEFORE", 0.0),
            pad_after_secs: env("OPENBARK_X_PAD_AFTER", 0.0),
        }
    }
}

pub struct SileroVad {
    options: VadOptions,
}

impl SileroVad {
    pub fn new(options: VadOptions) -> Result<Self, AsrError> {
        require_file(&options.model)?;
        Ok(Self { options })
    }

    /// Find speech spans in a whole clip.
    pub fn segments(&self, audio: &Audio) -> Result<Vec<SpeechSpan>, AsrError> {
        let config = self.config();
        let buffer_secs = self.options.max_speech_secs + 10.0;
        let vad = VoiceActivityDetector::create(&config, buffer_secs).ok_or(AsrError::Create)?;
        let levelled;
        let heard: &[f32] = if self.options.level {
            let mut lev = crate::level::Leveller::new();
            let mut out = lev.process(&audio.samples);
            out.extend(lev.finish());
            levelled = out;
            &levelled
        } else {
            &audio.samples
        };
        let total = audio.duration_secs();
        let (before, after) = (self.options.pad_before_secs, self.options.pad_after_secs);
        let mut spans: Vec<SpeechSpan> = Vec::new();
        let mut drain = |vad: &VoiceActivityDetector| {
            while let Some(segment) = vad.front() {
                let start = segment.start() as f32 / SAMPLE_RATE as f32;
                let end = start + segment.n() as f32 / SAMPLE_RATE as f32;
                let mut span = SpeechSpan { start: (start - before).max(0.0), end: (end + after).min(total) };
                // Padding never reaches back into the previous utterance.
                if let Some(last) = spans.last() {
                    span.start = span.start.max(last.end.min(start));
                }
                spans.push(span);
                vad.pop();
            }
        };
        for window in heard.chunks(WINDOW) {
            vad.accept_waveform(window);
            drain(&vad);
        }
        vad.flush();
        drain(&vad);
        Ok(spans)
    }
}

impl SileroVad {
    fn config(&self) -> VadModelConfig {
        VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: Some(self.options.model.clone()),
                threshold: self.options.threshold,
                min_silence_duration: self.options.min_silence_secs,
                min_speech_duration: self.options.min_speech_secs,
                window_size: WINDOW as i32,
                max_speech_duration: self.options.max_speech_secs,
            },
            sample_rate: SAMPLE_RATE as i32,
            num_threads: 1,
            provider: Some("cpu".into()),
            ..Default::default()
        }
    }

    /// A detector for live audio: feed blocks as they arrive and collect each
    /// utterance as soon as its trailing silence is heard.
    pub fn streaming(&self) -> Result<StreamingVad, AsrError> {
        let vad = VoiceActivityDetector::create(&self.config(), self.options.max_speech_secs + 10.0)
            .ok_or(AsrError::Create)?;
        Ok(StreamingVad {
            vad,
            pending: Vec::new(),
            leveller: self.options.level.then(crate::level::Leveller::new),
            history: Vec::new(),
            history_start: 0,
            fed: 0,
            last_end: 0,
            pad_before: (self.options.pad_before_secs * SAMPLE_RATE as f32) as usize,
            pad_after: (self.options.pad_after_secs * SAMPLE_RATE as f32) as usize,
            keep: ((self.options.max_speech_secs + 10.0) * SAMPLE_RATE as f32) as usize,
            held: std::collections::VecDeque::new(),
        })
    }
}

/// Silero's window: the detector consumes audio in blocks of this many samples.
const WINDOW: usize = 512;

/// One utterance from the live detector.
pub struct Utterance {
    /// Start time in seconds from the beginning of the stream.
    pub start: f32,
    pub samples: Vec<f32>,
}

impl Utterance {
    pub fn end(&self) -> f32 {
        self.start + self.samples.len() as f32 / SAMPLE_RATE as f32
    }
}

pub struct StreamingVad {
    vad: VoiceActivityDetector,
    /// What the detector hears, waiting for a whole window.
    pending: Vec<f32>,
    leveller: Option<crate::level::Leveller>,
    /// The original samples from `history_start` on (absolute sample
    /// indices), for utterances and their padding.
    history: Vec<f32>,
    history_start: usize,
    /// Samples given to the detector so far.
    fed: usize,
    /// End of the last utterance returned (absolute index): padding never
    /// reaches back into it.
    last_end: usize,
    pad_before: usize,
    pad_after: usize,
    keep: usize,
    /// Utterances waiting for their trailing padding to arrive, in order.
    held: std::collections::VecDeque<(usize, usize)>,
}

impl StreamingVad {
    /// Feed 16 kHz mono samples; returns utterances that have ended.
    pub fn accept(&mut self, samples: &[f32]) -> Vec<Utterance> {
        self.history.extend_from_slice(samples);
        match &mut self.leveller {
            Some(lev) => self.pending.extend(lev.process(samples)),
            None => self.pending.extend_from_slice(samples),
        }
        let whole = self.pending.len() / WINDOW * WINDOW;
        for window in self.pending[..whole].chunks(WINDOW) {
            self.vad.accept_waveform(window);
        }
        self.fed += whole;
        self.pending.drain(..whole);
        let out = self.drain(false);
        self.trim();
        out
    }

    /// End of stream: returns the final utterance, if speech was in progress.
    pub fn finish(&mut self) -> Vec<Utterance> {
        if let Some(lev) = &mut self.leveller {
            self.pending.extend(lev.finish());
        }
        if !self.pending.is_empty() {
            self.fed += self.pending.len();
            self.pending.resize(WINDOW, 0.0);
            self.vad.accept_waveform(&self.pending);
            self.pending.clear();
        }
        self.vad.flush();
        self.drain(true)
    }

    fn drain(&mut self, at_end: bool) -> Vec<Utterance> {
        while let Some(segment) = self.vad.front() {
            let start = segment.start().max(0) as usize;
            self.held.push_back((start, start + segment.n().max(0) as usize));
            self.vad.pop();
        }
        let mut out = Vec::new();
        while let Some(&(start, end)) = self.held.front() {
            let heard = self.history_start + self.history.len();
            if end + self.pad_after > heard && !at_end {
                break;
            }
            self.held.pop_front();
            out.push(self.release(start, end));
        }
        out
    }

    /// The utterance from `start` to `end` with its padding (as much of the
    /// trailing padding as has been heard).
    fn release(&mut self, start: usize, end: usize) -> Utterance {
        let heard = self.history_start + self.history.len();
        let want_end = end + self.pad_after;
        let from = start.saturating_sub(self.pad_before).max(self.last_end.min(start)).max(self.history_start);
        let to = want_end.min(heard);
        self.last_end = to;
        let samples = self.history[from - self.history_start..to.max(from) - self.history_start].to_vec();
        Utterance { start: from as f32 / SAMPLE_RATE as f32, samples }
    }

    /// Forget original samples no utterance can still need.
    fn trim(&mut self) {
        let heard = self.history_start + self.history.len();
        let mut keep_from = heard.saturating_sub(self.keep).min(self.fed.saturating_sub(self.keep));
        if let Some(&(start, _)) = self.held.front() {
            keep_from = keep_from.min(start.saturating_sub(self.pad_before));
        }
        if keep_from > self.history_start + SAMPLE_RATE as usize * 5 {
            self.history.drain(..keep_from - self.history_start);
            self.history_start = keep_from;
        }
    }
}
