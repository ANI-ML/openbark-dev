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

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: f32, end: f32) -> SpeechSpan {
        SpeechSpan { start, end }
    }
    fn turn(start: f32, end: f32, speaker: u32) -> SpeakerTurn {
        SpeakerTurn { start, end, speaker }
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
