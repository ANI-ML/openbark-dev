//! Speaker diarization: who spoke when.
//!
//! sherpa-onnx pyannote segmentation-3.0 + a speaker-embedding model +
//! clustering. Roles (DVM / client / technician) are assigned later by the
//! user; the engine only reports anonymous speaker indices.

use os_asr::sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};
use os_asr::{Audio, SpeechSpan};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum DiarError {
    #[error("model file not found: {0}")]
    ModelNotFound(String),
    #[error("failed to create diarizer (check model files)")]
    Create,
    #[error("diarization failed")]
    Process,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct SpeakerTurn {
    pub start: f32,
    pub end: f32,
    pub speaker: u32,
}

#[derive(Debug, Clone)]
pub struct DiarOptions {
    pub segmentation_model: String,
    pub embedding_model: String,
    /// Known speaker count, or `None` to cluster by threshold.
    pub num_speakers: Option<u32>,
    pub threshold: f32,
    /// Segmentation window step as a fraction of the 10 s window. sherpa's
    /// default (0.1) runs every moment ~10 times. 0.25 was 2.5x faster with
    /// identical purity on the fixtures (spike S2); 0.5 over-split speakers.
    pub window_shift_ratio: f32,
}

impl DiarOptions {
    pub fn new(segmentation_model: impl Into<String>, embedding_model: impl Into<String>) -> Self {
        Self {
            segmentation_model: segmentation_model.into(),
            embedding_model: embedding_model.into(),
            num_speakers: None,
            // Tuned on the synthetic fixtures (spike S2): 0.5 over-splits
            // speakers, 0.8+ merges them; 0.7 gave 99.6-99.7% purity.
            threshold: 0.7,
            window_shift_ratio: 0.25,
        }
    }
}

pub struct Diarizer {
    inner: OfflineSpeakerDiarization,
}

impl Diarizer {
    pub fn new(options: &DiarOptions) -> Result<Self, DiarError> {
        for file in [&options.segmentation_model, &options.embedding_model] {
            if !std::path::Path::new(file).is_file() {
                return Err(DiarError::ModelNotFound(file.clone()));
            }
        }
        let threads = std::thread::available_parallelism().map(|n| n.get().min(8) as i32).unwrap_or(4);
        let config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(options.segmentation_model.clone()),
                    window_shift_ratio: options.window_shift_ratio,
                },
                num_threads: threads,
                provider: Some("cpu".into()),
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(options.embedding_model.clone()),
                num_threads: threads,
                provider: Some("cpu".into()),
                ..Default::default()
            },
            clustering: FastClusteringConfig {
                num_clusters: options.num_speakers.map_or(-1, |n| n as i32),
                threshold: options.threshold,
                ..Default::default()
            },
            ..Default::default()
        };
        let inner = OfflineSpeakerDiarization::create(&config).ok_or(DiarError::Create)?;
        Ok(Self { inner })
    }

    pub fn turns(&self, audio: &Audio) -> Result<Vec<SpeakerTurn>, DiarError> {
        let result = self.inner.process(&audio.samples).ok_or(DiarError::Process)?;
        Ok(result
            .sort_by_start_time()
            .into_iter()
            .map(|s| SpeakerTurn { start: s.start, end: s.end, speaker: s.speaker.max(0) as u32 })
            .collect())
    }
}

/// A span of speech attributed to one speaker.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AttributedSpan {
    pub start: f32,
    pub end: f32,
    pub speaker: Option<u32>,
}

/// Split VAD speech spans wherever the diarized speaker changes, so each ASR
/// chunk has exactly one speaker. Pieces shorter than `min_piece` are merged
/// into their neighbour rather than transcribed alone.
pub fn attribute(spans: &[SpeechSpan], turns: &[SpeakerTurn], min_piece: f32) -> Vec<AttributedSpan> {
    let mut out = Vec::new();
    for span in spans {
        let mut pieces: Vec<AttributedSpan> = turns
            .iter()
            .filter_map(|t| {
                let (start, end) = (t.start.max(span.start), t.end.min(span.end));
                (end > start).then_some(AttributedSpan { start, end, speaker: Some(t.speaker) })
            })
            .collect();
        if pieces.is_empty() {
            out.push(AttributedSpan {
                start: span.start,
                end: span.end,
                speaker: nearest_speaker(span, turns),
            });
            continue;
        }
        // Overlapping turns (crosstalk): keep chronological order, trim overlaps.
        pieces.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<AttributedSpan> = Vec::new();
        for mut p in pieces {
            if let Some(last) = merged.last_mut() {
                if p.start < last.end {
                    p.start = last.end;
                }
                if p.end <= p.start {
                    continue;
                }
                if last.speaker == p.speaker || p.end - p.start < min_piece {
                    last.end = last.end.max(p.end);
                    continue;
                }
                if last.end - last.start < min_piece {
                    last.speaker = p.speaker;
                    last.end = p.end;
                    continue;
                }
            }
            merged.push(p);
        }
        // Cover the whole VAD span so no audio is dropped between pieces.
        if let Some(first) = merged.first_mut() {
            first.start = span.start;
        }
        if let Some(last) = merged.last_mut() {
            last.end = span.end;
        }
        for pair in 1..merged.len() {
            let boundary = merged[pair].start;
            merged[pair - 1].end = boundary;
        }
        out.extend(merged);
    }
    out
}

/// The speaker who talks most during `[start, end]`; falls back to the
/// nearest turn when no turn overlaps (e.g. speech the segmenter missed).
pub fn dominant_speaker(start: f32, end: f32, turns: &[SpeakerTurn]) -> Option<u32> {
    let mut totals: Vec<(u32, f32)> = Vec::new();
    for t in turns {
        let overlap = t.end.min(end) - t.start.max(start);
        if overlap > 0.0 {
            match totals.iter_mut().find(|(s, _)| *s == t.speaker) {
                Some((_, total)) => *total += overlap,
                None => totals.push((t.speaker, overlap)),
            }
        }
    }
    totals
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
        .or_else(|| nearest_speaker(&SpeechSpan { start, end }, turns))
}

fn nearest_speaker(span: &SpeechSpan, turns: &[SpeakerTurn]) -> Option<u32> {
    let mid = (span.start + span.end) / 2.0;
    turns
        .iter()
        .min_by(|a, b| {
            let da = (mid - (a.start + a.end) / 2.0).abs();
            let db = (mid - (b.start + b.end) / 2.0).abs();
            da.total_cmp(&db)
        })
        .map(|t| t.speaker)
}

/// Live speaker labelling: one voice embedding per utterance, matched to the
/// speakers heard so far by cosine similarity (new speaker below the
/// threshold). No pass over the whole recording is needed after stop.
///
/// A voice through a telephone (a client on a phone on speaker) carries
/// almost nothing above 3.4 kHz, while a voice in the room reaches 8 kHz.
/// The voice model can place the two close together (narrowband, quiet,
/// reverberant speech loses what tells voices apart), so each speaker also
/// keeps the mean of its utterances' high-band level ([`high_band_db`]), and
/// a phone voice and a room voice are never merged ([`band_apart`]).
pub struct SpeakerTracker {
    extractor: os_asr::sherpa_onnx::SpeakerEmbeddingExtractor,
    speakers: Vec<Centroid>,
    threshold: f32,
    band_cue: bool,
}

struct Centroid {
    sum: Vec<f32>,
    count: u32,
    band_sum: f32,
    band_count: u32,
}

impl Centroid {
    fn similarity(&self, e: &[f32]) -> f32 {
        let norm = self.sum.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
        self.sum.iter().zip(e).map(|(a, b)| a * b).sum::<f32>() / norm
    }

    fn band(&self) -> Option<f32> {
        (self.band_count > 0).then(|| self.band_sum / self.band_count as f32)
    }

    fn add(&mut self, embedding: &[f32], band: Option<f32>) {
        self.sum.iter_mut().zip(embedding).for_each(|(s, e)| *s += e);
        self.count += 1;
        if let Some(b) = band {
            self.band_sum += b;
            self.band_count += 1;
        }
    }
}

/// Utterances shorter than this give unreliable embeddings: they are matched
/// to an existing speaker but never start a new one.
const MIN_EMBED_SECS: f32 = 1.0;

/// Below this high-band level (dB, see [`high_band_db`]) an utterance sounds
/// like a telephone. Synthetic consults: room voices -20 to -2 dB per line,
/// phone voices -40 to -30 dB.
pub const PHONE_BAND_DB: f32 = -24.0;

/// Are two high-band levels a phone voice and a room voice? One must sound
/// like a telephone and the two must be at least `gap` dB apart, so voices in
/// an ordinary consult (all full band) are never split by this.
pub fn band_apart(a: f32, b: f32, gap: f32) -> bool {
    (a - b).abs() > gap && a.min(b) < PHONE_BAND_DB
}

/// Energy between 4 and 8 kHz relative to the telephone band (300 to 3400
/// Hz), in dB, for 16 kHz audio. Speech in the room: about -20 to 0 dB over
/// a sentence; speech through a phone line: about -30 dB or lower (only
/// the room's own noise is up there).
pub fn high_band_db(samples: &[f32]) -> f32 {
    let qs = [0.541_196_1f32, 1.306_563];
    let mut high: Vec<Biquad> = qs.iter().map(|q| Biquad::new(4000.0, *q, true)).collect();
    let mut band: Vec<Biquad> = qs
        .iter()
        .map(|q| Biquad::new(300.0, *q, true))
        .chain(qs.iter().map(|q| Biquad::new(3400.0, *q, false)))
        .collect();
    let (mut h, mut m) = (0.0f64, 0.0f64);
    for &x in samples {
        let hx = high.iter_mut().fold(x, |v, f| f.run(v));
        let mx = band.iter_mut().fold(x, |v, f| f.run(v));
        h += (hx * hx) as f64;
        m += (mx * mx) as f64;
    }
    (10.0 * (h / m.max(1e-20)).max(1e-12).log10()) as f32
}

/// An RBJ-cookbook biquad at 16 kHz.
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(freq: f32, q: f32, high: bool) -> Self {
        let w = 2.0 * std::f32::consts::PI * freq / os_asr::SAMPLE_RATE as f32;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let b = if high {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        } else {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        };
        Self { b: b.map(|x| x / a0), a: [-2.0 * cos / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

impl SpeakerTracker {
    pub fn new(embedding_model: &str, threshold: f32) -> Result<Self, DiarError> {
        if !std::path::Path::new(embedding_model).is_file() {
            return Err(DiarError::ModelNotFound(embedding_model.into()));
        }
        let config = SpeakerEmbeddingExtractorConfig {
            model: Some(embedding_model.into()),
            num_threads: 2,
            provider: Some("cpu".into()),
            ..Default::default()
        };
        let extractor =
            os_asr::sherpa_onnx::SpeakerEmbeddingExtractor::create(&config).ok_or(DiarError::Create)?;
        let band_cue = std::env::var("OPENBARK_X_BAND").map_or(true, |v| v != "0");
        Ok(Self { extractor, speakers: Vec::new(), threshold, band_cue })
    }

    fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        let stream = self.extractor.create_stream()?;
        stream.accept_waveform(os_asr::SAMPLE_RATE as i32, samples);
        stream.input_finished();
        if !self.extractor.is_ready(&stream) {
            return None;
        }
        let mut e = self.extractor.compute(&stream)?;
        let norm = e.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
        e.iter_mut().for_each(|v| *v /= norm);
        Some(e)
    }

    /// Speaker index for one utterance (16 kHz mono), or `None` if unknown.
    pub fn assign(&mut self, samples: &[f32]) -> Option<u32> {
        let embedding = self.embed(samples)?;
        let secs = samples.len() as f32 / os_asr::SAMPLE_RATE as f32;
        let long_enough = secs >= MIN_EMBED_SECS;
        let band = self.band_cue.then(|| high_band_db(samples));
        // A short utterance's high band is less certain: it needs a wider gap.
        let gap = if long_enough { 10.0 } else { 16.0 };
        let best = self
            .speakers
            .iter()
            .enumerate()
            .filter(|(_, c)| match (band, c.band()) {
                (Some(b), Some(cb)) => !band_apart(b, cb, gap),
                _ => true,
            })
            .map(|(i, c)| (i, c.similarity(&embedding)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let learn = if long_enough { band } else { None };
        match best {
            Some((i, sim)) if sim >= self.threshold || !long_enough => {
                if long_enough {
                    self.speakers[i].add(&embedding, learn);
                }
                Some(i as u32)
            }
            _ if long_enough => {
                let mut c = Centroid { sum: vec![0.0; embedding.len()], count: 0, band_sum: 0.0, band_count: 0 };
                c.add(&embedding, learn);
                self.speakers.push(c);
                Some(self.speakers.len() as u32 - 1)
            }
            // Short, and unlike every speaker so far by its band: the nearest
            // voice overall rather than a voice it can't be.
            None if !self.speakers.is_empty() => self
                .speakers
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.similarity(&embedding)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i as u32),
            _ => None,
        }
    }

    pub fn speaker_count(&self) -> usize {
        self.speakers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: f32, end: f32) -> SpeechSpan {
        SpeechSpan { start, end }
    }
    fn turn(start: f32, end: f32, speaker: u32) -> SpeakerTurn {
        SpeakerTurn { start, end, speaker }
    }

    fn tone(freq: f32, secs: f32) -> Vec<f32> {
        (0..(secs * 16_000.0) as usize)
            .map(|i| 0.1 * (2.0 * std::f32::consts::PI * freq * i as f32 / 16_000.0).sin())
            .collect()
    }

    #[test]
    fn high_band_level_tells_a_phone_line_from_the_room() {
        // Room speech has energy up to 8 kHz; a phone line stops at 3.4 kHz.
        let room: Vec<f32> = tone(1000.0, 1.0).iter().zip(tone(6000.0, 1.0)).map(|(a, b)| a + 0.3 * b).collect();
        let phone: Vec<f32> =
            tone(1000.0, 1.0).iter().zip(tone(6000.0, 1.0)).map(|(a, b)| a + 0.001 * b).collect();
        let (r, p) = (high_band_db(&room), high_band_db(&phone));
        assert!(r > -12.0 && r < -8.0, "room {r}");
        assert!(p < -40.0, "phone {p}");
        assert!(band_apart(r, p, 10.0));
    }

    #[test]
    fn full_band_voices_are_never_split_by_band() {
        // Two room voices 15 dB apart in high band, both above the phone level.
        assert!(!band_apart(-3.0, -18.0, 10.0));
        // A phone voice and a room voice.
        assert!(band_apart(-12.0, -32.0, 10.0));
        // Two phone voices.
        assert!(!band_apart(-30.0, -36.0, 10.0));
        // A short utterance needs the wider gap.
        assert!(!band_apart(-12.0, -26.0, 16.0));
    }

    #[test]
    fn dominant_speaker_takes_the_longest_overlap() {
        let turns = [turn(0.0, 2.0, 0), turn(2.0, 6.0, 1), turn(6.0, 7.0, 0)];
        assert_eq!(dominant_speaker(1.0, 5.0, &turns), Some(1));
        assert_eq!(dominant_speaker(0.0, 1.5, &turns), Some(0));
        assert_eq!(dominant_speaker(20.0, 21.0, &turns), Some(0), "nearest turn when nothing overlaps");
        assert_eq!(dominant_speaker(1.0, 2.0, &[]), None);
    }

    #[test]
    fn single_speaker_span_is_unchanged() {
        let out = attribute(&[span(1.0, 4.0)], &[turn(0.5, 5.0, 0)], 0.4);
        assert_eq!(out, vec![AttributedSpan { start: 1.0, end: 4.0, speaker: Some(0) }]);
    }

    #[test]
    fn splits_at_speaker_change() {
        let out = attribute(&[span(0.0, 6.0)], &[turn(0.0, 3.0, 0), turn(3.0, 6.0, 1)], 0.4);
        assert_eq!(
            out,
            vec![
                AttributedSpan { start: 0.0, end: 3.0, speaker: Some(0) },
                AttributedSpan { start: 3.0, end: 6.0, speaker: Some(1) },
            ]
        );
    }

    #[test]
    fn tiny_interjection_is_absorbed() {
        let out =
            attribute(&[span(0.0, 6.0)], &[turn(0.0, 3.0, 0), turn(3.0, 3.2, 1), turn(3.2, 6.0, 0)], 0.4);
        assert_eq!(out, vec![AttributedSpan { start: 0.0, end: 6.0, speaker: Some(0) }]);
    }

    #[test]
    fn span_without_turns_takes_nearest_speaker() {
        let out = attribute(&[span(10.0, 11.0)], &[turn(0.0, 2.0, 0), turn(8.0, 9.5, 1)], 0.4);
        assert_eq!(out[0].speaker, Some(1));
    }

    #[test]
    fn crosstalk_overlap_is_trimmed() {
        let out = attribute(&[span(0.0, 5.0)], &[turn(0.0, 3.0, 0), turn(2.0, 5.0, 1)], 0.4);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].end, out[1].start);
        assert_eq!(out[1].speaker, Some(1));
    }
}
