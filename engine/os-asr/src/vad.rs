use serde::Serialize;
#[cfg(feature = "stt")]
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
}

impl VadOptions {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            threshold: 0.5,
            min_silence_secs: 0.3,
            min_speech_secs: 0.25,
            max_speech_secs: 20.0,
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
    #[cfg(not(feature = "stt"))]
    pub fn segments(&self, audio: &Audio) -> Result<Vec<SpeechSpan>, AsrError> {
        let _ = (audio, &self.options);
        Err(AsrError::NotInThisBuild)
    }

    /// Find speech spans in a whole clip.
    #[cfg(feature = "stt")]
    pub fn segments(&self, audio: &Audio) -> Result<Vec<SpeechSpan>, AsrError> {
        let config = self.config();
        let buffer_secs = self.options.max_speech_secs + 10.0;
        let vad = VoiceActivityDetector::create(&config, buffer_secs).ok_or(AsrError::Create)?;
        let mut spans = Vec::new();
        let mut drain = |vad: &VoiceActivityDetector| {
            while let Some(segment) = vad.front() {
                let start = segment.start() as f32 / SAMPLE_RATE as f32;
                let end = start + segment.n() as f32 / SAMPLE_RATE as f32;
                spans.push(SpeechSpan { start, end });
                vad.pop();
            }
        };
        for window in audio.samples.chunks(WINDOW) {
            vad.accept_waveform(window);
            drain(&vad);
        }
        vad.flush();
        drain(&vad);
        Ok(spans)
    }
}

#[cfg(not(feature = "stt"))]
impl SileroVad {
    /// No live detector without speech recognition in the build.
    pub fn streaming(&self) -> Result<StreamingVad, AsrError> {
        Err(AsrError::NotInThisBuild)
    }
}

#[cfg(feature = "stt")]
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
        Ok(StreamingVad { vad, pending: Vec::new() })
    }
}

/// Silero's window: the detector consumes audio in blocks of this many samples.
#[cfg(feature = "stt")]
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

#[cfg(feature = "stt")]
pub struct StreamingVad {
    vad: VoiceActivityDetector,
    pending: Vec<f32>,
}

/// The live detector in a build without speech recognition: never created.
#[cfg(not(feature = "stt"))]
pub struct StreamingVad {
    _never: std::convert::Infallible,
}

#[cfg(not(feature = "stt"))]
impl StreamingVad {
    pub fn accept(&mut self, _samples: &[f32]) -> Vec<Utterance> {
        match self._never {}
    }

    pub fn finish(&mut self) -> Vec<Utterance> {
        match self._never {}
    }
}

#[cfg(feature = "stt")]
impl StreamingVad {
    /// Feed 16 kHz mono samples; returns utterances that have ended.
    pub fn accept(&mut self, samples: &[f32]) -> Vec<Utterance> {
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() / WINDOW * WINDOW;
        for window in self.pending[..whole].chunks(WINDOW) {
            self.vad.accept_waveform(window);
        }
        self.pending.drain(..whole);
        self.drain()
    }

    /// End of stream: returns the final utterance, if speech was in progress.
    pub fn finish(&mut self) -> Vec<Utterance> {
        if !self.pending.is_empty() {
            self.pending.resize(WINDOW, 0.0);
            self.vad.accept_waveform(&self.pending);
            self.pending.clear();
        }
        self.vad.flush();
        self.drain()
    }

    fn drain(&mut self) -> Vec<Utterance> {
        let mut out = Vec::new();
        while let Some(segment) = self.vad.front() {
            out.push(Utterance {
                start: segment.start() as f32 / SAMPLE_RATE as f32,
                samples: segment.samples().to_vec(),
            });
            self.vad.pop();
        }
        out
    }
}
