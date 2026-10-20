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
    /// A second detector on the original audio marks where a louder voice
    /// starts or stops inside a levelled utterance, and the utterance is cut
    /// there ([`split_at_level_steps`]): lifting the quiet voice also lifts
    /// its echo, which would otherwise bridge the pause before the vet
    /// answers. Utterances always carry the original samples.
    pub level: bool,
    /// Audio kept before and after each detected utterance, so a first or
    /// last word the detector heard late or let go early isn't cut.
    pub pad_before_secs: f32,
    pub pad_after_secs: f32,
    /// A voice this much louder (dB) than the speech beside it is another
    /// speaker: the levelled utterance is cut where it starts or stops.
    pub level_step_db: f32,
}

impl VadOptions {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            threshold: 0.5,
            min_silence_secs: 0.3,
            min_speech_secs: 0.25,
            max_speech_secs: 20.0,
            level: false,
            pad_before_secs: 0.0,
            pad_after_secs: 0.0,
            level_step_db: 8.0,
        }
    }

    /// A client on a phone on speaker in the room (the Record screen's
    /// "Phone on speaker"): detect speech on the levelled copy, cut at level
    /// steps. Measured on the speakerphone fixtures (docs/eval/M2.md).
    pub fn speakerphone(mut self) -> Self {
        self.level = true;
        self
    }
}

pub struct SileroVad {
    options: VadOptions,
}

/// Silero's window: the detector consumes audio in blocks of this many samples.
const WINDOW: usize = 512;

/// No piece shorter than this is cut off a levelled utterance (samples).
const MIN_PIECE: usize = SAMPLE_RATE as usize * 3 / 10;

impl SileroVad {
    pub fn new(options: VadOptions) -> Result<Self, AsrError> {
        require_file(&options.model)?;
        Ok(Self { options })
    }

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

    fn detector(&self) -> Result<VoiceActivityDetector, AsrError> {
        VoiceActivityDetector::create(&self.config(), self.options.max_speech_secs + 10.0).ok_or(AsrError::Create)
    }

    /// Find speech spans in a whole clip.
    pub fn segments(&self, audio: &Audio) -> Result<Vec<SpeechSpan>, AsrError> {
        let detect = |samples: &[f32]| -> Result<Vec<(usize, usize)>, AsrError> {
            let vad = self.detector()?;
            let mut spans = Vec::new();
            let mut drain = |vad: &VoiceActivityDetector| {
                while let Some(segment) = vad.front() {
                    let start = segment.start().max(0) as usize;
                    spans.push((start, start + segment.n().max(0) as usize));
                    vad.pop();
                }
            };
            for window in samples.chunks(WINDOW) {
                vad.accept_waveform(window);
                drain(&vad);
            }
            vad.flush();
            drain(&vad);
            Ok(spans)
        };
        let spans = if self.options.level {
            let mut lev = crate::level::Leveller::new();
            let mut levelled = lev.process(&audio.samples);
            levelled.extend(lev.finish());
            let raw = detect(&audio.samples)?;
            let samples = &audio.samples;
            detect(&levelled)?
                .into_iter()
                .flat_map(|span| {
                    split_at_level_steps(span, &raw, self.options.level_step_db, &|a, b| {
                        crate::level::speech_level_db(&samples[a.min(samples.len())..b.min(samples.len())])
                    })
                })
                .flat_map(|(a, b)| {
                    let (a, b) = (a.min(samples.len()), b.min(samples.len()));
                    split_at_level_dips(&samples[a..b], self.options.level_step_db)
                        .into_iter()
                        .map(move |(x, y)| (a + x, a + y))
                })
                .collect()
        } else {
            detect(&audio.samples)?
        };
        let total = audio.samples.len();
        let (before, after) = (secs_to_samples(self.options.pad_before_secs), secs_to_samples(self.options.pad_after_secs));
        let mut out: Vec<SpeechSpan> = Vec::new();
        let mut last_end = 0usize;
        for (start, end) in spans {
            // Padding never reaches back into the previous utterance.
            let from = start.saturating_sub(before).max(last_end.min(start));
            let to = (end + after).min(total).max(from);
            last_end = to;
            out.push(SpeechSpan { start: from as f32 / SAMPLE_RATE as f32, end: to as f32 / SAMPLE_RATE as f32 });
        }
        Ok(out)
    }

    /// A detector for live audio: feed blocks as they arrive and collect each
    /// utterance as soon as its trailing silence is heard.
    pub fn streaming(&self) -> Result<StreamingVad, AsrError> {
        Ok(StreamingVad {
            vad: self.detector()?,
            raw: if self.options.level { Some(self.detector()?) } else { None },
            pending: Vec::new(),
            raw_pending: Vec::new(),
            leveller: self.options.level.then(crate::level::Leveller::new),
            history: Vec::new(),
            history_start: 0,
            raw_fed: 0,
            raw_spans: Vec::new(),
            raw_onset: None,
            last_end: 0,
            pad_before: secs_to_samples(self.options.pad_before_secs),
            pad_after: secs_to_samples(self.options.pad_after_secs),
            keep: secs_to_samples(self.options.max_speech_secs + 10.0),
            step_db: self.options.level_step_db,
            held: std::collections::VecDeque::new(),
        })
    }
}

fn secs_to_samples(secs: f32) -> usize {
    (secs.max(0.0) * SAMPLE_RATE as f32) as usize
}

/// Cut a span the levelled detector found wherever a voice clearly louder
/// than the speech before it starts (a detector on the original audio found
/// speech starting there), or such a voice stops and quieter speech goes on.
/// `level(a, b)` is the speech level of samples `a..b` in dB. Pieces are at
/// least [`MIN_PIECE`] long. All positions are sample indices.
pub fn split_at_level_steps(
    span: (usize, usize),
    raw: &[(usize, usize)],
    step_db: f32,
    level: &dyn Fn(usize, usize) -> f32,
) -> Vec<(usize, usize)> {
    let (start, end) = span;
    let mut cuts: Vec<usize> = Vec::new();
    let mut piece = start;
    for &(rs, re) in raw.iter().filter(|(rs, re)| *re > start && *rs < end) {
        let re = re.min(end);
        // A louder voice starts inside the span.
        if rs >= piece + MIN_PIECE && re >= rs + MIN_PIECE && level(rs, re) - level(piece, rs) >= step_db {
            cuts.push(rs);
            piece = rs;
        }
        // A louder voice stops and quieter speech goes on after it.
        let next = raw.iter().map(|r| r.0).find(|s| *s > re).unwrap_or(end).min(end);
        if re >= piece + MIN_PIECE
            && next >= re + MIN_PIECE
            && level(piece.max(rs), re) - level(re, next) >= step_db
        {
            cuts.push(re);
            piece = re;
        }
    }
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut from = start;
    for c in cuts {
        out.push((from, c));
        from = c;
    }
    out.push((from, end));
    out
}

/// Cut speech wherever a pause separates two stretches whose levels differ by
/// at least `step_db`: a quiet voice and a loud one taking turns, which the
/// detector on the original audio didn't separate either (at 15 dB below the
/// vet a phone voice is loud enough for it, and the room echo bridges the
/// pause). A pause is at least 160 ms within 6 dB of the quietest part.
/// Returns sample ranges within `samples`, each at least [`MIN_PIECE`] long.
pub fn split_at_level_dips(samples: &[f32], step_db: f32) -> Vec<(usize, usize)> {
    const FRAME: usize = SAMPLE_RATE as usize / 50; // 20 ms
    const MIN_DIP: usize = 8; // frames
    let whole = vec![(0, samples.len())];
    let levels: Vec<f32> = samples
        .chunks(FRAME)
        .map(|f| 10.0 * (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).max(1e-14).log10())
        .collect();
    if levels.len() < 3 * MIN_DIP {
        return whole;
    }
    let mut sorted = levels.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted[sorted.len() / 10] + 6.0;
    // Pauses: runs of quiet frames, as frame ranges.
    let mut dips: Vec<(usize, usize)> = Vec::new();
    let mut run: Option<usize> = None;
    for (i, l) in levels.iter().chain(std::iter::once(&f32::MAX)).enumerate() {
        match (*l < floor, run) {
            (true, None) => run = Some(i),
            (false, Some(s)) => {
                if i - s >= MIN_DIP && s > 0 && i < levels.len() {
                    dips.push((s, i));
                }
                run = None;
            }
            _ => {}
        }
    }
    // Stretches between pauses, and the level of each.
    let mut bounds = vec![0usize];
    for &(s, e) in &dips {
        bounds.push(s);
        bounds.push(e);
    }
    bounds.push(levels.len());
    let stretch = |k: usize| (bounds[2 * k] * FRAME, (bounds[2 * k + 1] * FRAME).min(samples.len()));
    let count = dips.len() + 1;
    let level = |(a, b): (usize, usize)| crate::level::speech_level_db(&samples[a..b]);
    let mut out = Vec::new();
    let mut from = 0usize;
    for k in 0..count - 1 {
        let (left, right) = (stretch(k), stretch(k + 1));
        let cut = (dips[k].0 + dips[k].1) / 2 * FRAME;
        let long_enough = left.1 - left.0 >= MIN_PIECE && right.1 - right.0 >= MIN_PIECE;
        if long_enough && cut >= from + MIN_PIECE && (level(left) - level(right)).abs() >= step_db {
            out.push((from, cut));
            from = cut;
        }
    }
    out.push((from, samples.len()));
    out
}

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
    /// The detector deciding utterances: on the levelled copy when levelling.
    vad: VoiceActivityDetector,
    /// When levelling: a detector on the original audio, for level steps.
    raw: Option<VoiceActivityDetector>,
    /// What each detector hears, waiting for a whole window.
    pending: Vec<f32>,
    raw_pending: Vec<f32>,
    leveller: Option<crate::level::Leveller>,
    /// The original samples from `history_start` on (absolute sample
    /// indices), for utterances and their padding.
    history: Vec<f32>,
    history_start: usize,
    raw_fed: usize,
    /// Speech the raw detector found (absolute sample indices), recent only.
    raw_spans: Vec<(usize, usize)>,
    /// Where the raw detector's current speech was first detected.
    raw_onset: Option<usize>,
    /// End of the last utterance returned: padding never reaches back into it.
    last_end: usize,
    pad_before: usize,
    pad_after: usize,
    keep: usize,
    step_db: f32,
    /// Utterances waiting for their trailing padding (and the raw detector)
    /// to arrive, in order.
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
        Self::feed(&self.vad, &mut self.pending);
        if let Some(raw) = &self.raw {
            self.raw_pending.extend_from_slice(samples);
            let windows = self.raw_pending.len() / WINDOW;
            for w in 0..windows {
                raw.accept_waveform(&self.raw_pending[w * WINDOW..(w + 1) * WINDOW]);
                self.raw_fed += WINDOW;
                match (raw.detected(), self.raw_onset) {
                    (true, None) => self.raw_onset = Some(self.raw_fed),
                    (false, Some(_)) => self.raw_onset = None,
                    _ => {}
                }
            }
            self.raw_pending.drain(..windows * WINDOW);
        }
        let out = self.drain(false);
        self.trim();
        out
    }

    fn feed(vad: &VoiceActivityDetector, pending: &mut Vec<f32>) {
        let whole = pending.len() / WINDOW * WINDOW;
        for window in pending[..whole].chunks(WINDOW) {
            vad.accept_waveform(window);
        }
        pending.drain(..whole);
    }

    /// End of stream: returns the final utterance, if speech was in progress.
    pub fn finish(&mut self) -> Vec<Utterance> {
        if let Some(lev) = &mut self.leveller {
            self.pending.extend(lev.finish());
        }
        for (vad, pending) in [(Some(&self.vad), &mut self.pending), (self.raw.as_ref(), &mut self.raw_pending)] {
            let Some(vad) = vad else { continue };
            if !pending.is_empty() {
                pending.resize(WINDOW, 0.0);
                vad.accept_waveform(pending);
                pending.clear();
            }
            vad.flush();
        }
        self.drain(true)
    }

    fn drain(&mut self, at_end: bool) -> Vec<Utterance> {
        while let Some(segment) = self.vad.front() {
            let start = segment.start().max(0) as usize;
            self.held.push_back((start, start + segment.n().max(0) as usize));
            self.vad.pop();
        }
        if let Some(raw) = &self.raw {
            while let Some(segment) = raw.front() {
                let start = segment.start().max(0) as usize;
                self.raw_spans.push((start, start + segment.n().max(0) as usize));
                raw.pop();
            }
        }
        let mut out = Vec::new();
        while let Some(&(start, end)) = self.held.front() {
            let heard = self.history_start + self.history.len();
            if !at_end && end + self.pad_after > heard {
                break;
            }
            // Wait while the raw detector may still be inside speech that
            // began within this utterance (its span isn't known until it ends).
            let onset_margin = SAMPLE_RATE as usize / 2;
            let raw_busy = self.raw_onset.is_some_and(|on| on < end + onset_margin);
            if !at_end && raw_busy && heard < end + secs_to_samples(5.0) {
                break;
            }
            self.held.pop_front();
            out.extend(self.release(start, end));
        }
        out
    }

    /// The utterance from `start` to `end`, cut at level steps, the first
    /// piece with its leading padding and the last with its trailing padding.
    fn release(&mut self, start: usize, end: usize) -> Vec<Utterance> {
        let heard = self.history_start + self.history.len();
        let pieces = if self.raw.is_some() {
            let (history, offset) = (&self.history, self.history_start);
            let level = |a: usize, b: usize| {
                let a = a.clamp(offset, offset + history.len()) - offset;
                let b = b.clamp(offset, offset + history.len()) - offset;
                crate::level::speech_level_db(&history[a..b.max(a)])
            };
            let offset = self.history_start;
            split_at_level_steps((start, end), &self.raw_spans, self.step_db, &level)
                .into_iter()
                .flat_map(|(a, b)| {
                    let (x0, x1) = (a.max(offset) - offset, b.max(offset) - offset);
                    let x1 = x1.min(self.history.len());
                    split_at_level_dips(&self.history[x0.min(x1)..x1], self.step_db)
                        .into_iter()
                        .map(move |(x, y)| (a + x, a + y))
                })
                .collect()
        } else {
            vec![(start, end)]
        };
        let last = pieces.len() - 1;
        let mut out = Vec::with_capacity(pieces.len());
        for (i, (a, b)) in pieces.into_iter().enumerate() {
            let from = if i == 0 { a.saturating_sub(self.pad_before).max(self.last_end.min(a)) } else { a };
            let to = if i == last { (b + self.pad_after).min(heard) } else { b };
            let from = from.max(self.history_start);
            let to = to.max(from);
            self.last_end = to;
            out.push(Utterance {
                start: from as f32 / SAMPLE_RATE as f32,
                samples: self.history[from - self.history_start..to - self.history_start].to_vec(),
            });
        }
        out
    }

    /// Forget original samples and raw spans no utterance can still need.
    fn trim(&mut self) {
        let heard = self.history_start + self.history.len();
        let mut keep_from = heard.saturating_sub(self.keep);
        if let Some(&(start, _)) = self.held.front() {
            keep_from = keep_from.min(start.saturating_sub(self.pad_before));
        }
        self.raw_spans.retain(|(_, e)| *e >= keep_from);
        if keep_from > self.history_start + SAMPLE_RATE as usize * 5 {
            self.history.drain(..keep_from - self.history_start);
            self.history_start = keep_from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: usize = SAMPLE_RATE as usize;

    /// Levels by region: quiet (-45 dB) before 3 s, loud (-20 dB) from 3 to 6 s,
    /// quiet again after 6 s.
    fn level(a: usize, b: usize) -> f32 {
        let mid = (a + b) / 2;
        if (3 * S..6 * S).contains(&mid) { -20.0 } else { -45.0 }
    }

    #[test]
    fn a_louder_voice_starting_inside_a_levelled_utterance_cuts_it() {
        // The levelled detector heard one utterance from 0 to 6 s (a quiet
        // client, then the vet answering); the raw one heard the vet from 3 s.
        let pieces = split_at_level_steps((0, 6 * S), &[(3 * S, 6 * S)], 8.0, &level);
        assert_eq!(pieces, vec![(0, 3 * S), (3 * S, 6 * S)]);
    }

    #[test]
    fn a_louder_voice_stopping_before_quiet_speech_cuts_it() {
        let pieces = split_at_level_steps((3 * S, 9 * S), &[(3 * S, 6 * S)], 8.0, &level);
        assert_eq!(pieces, vec![(3 * S, 6 * S), (6 * S, 9 * S)]);
    }

    #[test]
    fn both_steps_in_one_utterance() {
        let pieces = split_at_level_steps((0, 9 * S), &[(3 * S, 6 * S)], 8.0, &level);
        assert_eq!(pieces, vec![(0, 3 * S), (3 * S, 6 * S), (6 * S, 9 * S)]);
    }

    #[test]
    fn speech_of_one_level_is_never_cut() {
        let flat = |_: usize, _: usize| -30.0;
        // The raw detector split the speech at a pause; the levelled one didn't.
        let pieces = split_at_level_steps((0, 6 * S), &[(0, 2 * S), (2 * S + 4_000, 6 * S)], 8.0, &flat);
        assert_eq!(pieces, vec![(0, 6 * S)]);
    }

    fn tone(secs: f32, db: f32) -> Vec<f32> {
        let amp = 10f32.powf(db / 20.0) * std::f32::consts::SQRT_2;
        (0..(secs * S as f32) as usize).map(|i| amp * (i as f32 * 0.2).sin()).collect()
    }

    #[test]
    fn a_pause_between_a_loud_and_a_quiet_voice_is_cut() {
        // Vet at -22 dBFS, 0.6 s pause (noise at -70), client at -38, pause, vet.
        let mut s = tone(2.0, -22.0);
        let gap = tone(0.6, -70.0);
        s.extend(&gap);
        let client_at = s.len();
        s.extend(tone(2.0, -38.0));
        s.extend(&gap);
        let vet_at = s.len();
        s.extend(tone(2.0, -22.0));
        let pieces = split_at_level_dips(&s, 8.0);
        assert_eq!(pieces.len(), 3, "{pieces:?}");
        let near = |a: usize, b: usize| a.abs_diff(b) <= S * 4 / 10;
        assert!(near(pieces[0].1, client_at - gap.len() / 2), "{pieces:?}");
        assert!(near(pieces[1].1, vet_at - gap.len() / 2), "{pieces:?}");
        assert_eq!(pieces[2].1, s.len());
    }

    #[test]
    fn pauses_between_sentences_of_one_voice_are_not_cut() {
        let mut s = tone(2.0, -22.0);
        s.extend(tone(0.6, -70.0));
        s.extend(tone(2.0, -25.0)); // a little softer, the same voice
        assert_eq!(split_at_level_dips(&s, 8.0), vec![(0, s.len())]);
    }

    #[test]
    fn no_piece_shorter_than_the_minimum() {
        // A loud voice starting 0.1 s into the utterance: nothing worth cutting off.
        let pieces = split_at_level_steps((0, 6 * S), &[(S / 10, 6 * S)], 8.0, &|a, _| {
            if a < S / 10 { -45.0 } else { -20.0 }
        });
        assert_eq!(pieces, vec![(0, 6 * S)]);
    }
}
