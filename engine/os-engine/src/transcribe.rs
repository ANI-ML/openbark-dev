use std::time::Instant;

use os_asr::{AsrBackend, Audio, SileroVad, SpeechSpan};
use os_diar::Diarizer;
use serde::{Deserialize, Serialize};

use crate::EngineError;
use crate::transcript::{Segment, Transcript};

/// Speech shorter than this is skipped (ASR on tiny clips mostly yields noise).
const MIN_SPEECH_SECS: f32 = 0.2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscribeStats {
    pub audio_secs: f32,
    pub vad_ms: u64,
    /// Diarization runs in parallel with ASR, so these overlap in wall time.
    pub diarization_ms: u64,
    pub asr_ms: u64,
    pub total_ms: u64,
    /// Real-time factor: processing time / audio duration (lower is faster).
    pub rtf: f32,
    pub segments: usize,
    pub speakers: usize,
}

pub struct Transcriber {
    pub vad: SileroVad,
    pub asr: Box<dyn AsrBackend>,
    pub diarizer: Option<Diarizer>,
    /// A client on a phone on speaker (see `LiveOptions::speakerphone`):
    /// each span is brought to a normal level before recognition.
    pub speakerphone: bool,
}

impl Transcriber {
    /// Transcribe a whole recording.
    ///
    /// ASR (over VAD speech spans) and diarization run concurrently; speakers
    /// are attributed afterwards by overlap. `on_segment` sees each segment as
    /// soon as it is recognised, before speakers are known (`speaker: None`);
    /// the returned transcript carries the final speaker labels.
    pub fn run(
        &self,
        audio: &Audio,
        on_segment: &mut dyn FnMut(&Segment),
    ) -> Result<(Transcript, TranscribeStats), EngineError> {
        let started = Instant::now();
        let mut stats = TranscribeStats { audio_secs: audio.duration_secs(), ..Default::default() };

        let t = Instant::now();
        let spans = self.vad.segments(audio)?;
        stats.vad_ms = t.elapsed().as_millis() as u64;

        let (mut transcript, turns) = std::thread::scope(|scope| -> Result<_, EngineError> {
            let diarization = self.diarizer.as_ref().map(|d| {
                scope.spawn(move || {
                    let t = Instant::now();
                    d.turns(audio).map(|turns| (turns, t.elapsed().as_millis() as u64))
                })
            });

            let t = Instant::now();
            let transcript = self.recognize(audio, &spans, on_segment)?;
            stats.asr_ms = t.elapsed().as_millis() as u64;

            let turns = match diarization {
                Some(handle) => {
                    let (turns, ms) = handle.join().expect("diarization thread panicked")?;
                    stats.diarization_ms = ms;
                    Some(turns)
                }
                None => None,
            };
            Ok((transcript, turns))
        })?;

        if let Some(turns) = turns {
            for segment in &mut transcript.segments {
                segment.speaker = os_diar::dominant_speaker(segment.start, segment.end, &turns);
            }
            merge_speaker_ids(&mut transcript);
        }

        stats.total_ms = started.elapsed().as_millis() as u64;
        stats.rtf =
            if stats.audio_secs > 0.0 { stats.total_ms as f32 / 1000.0 / stats.audio_secs } else { 0.0 };
        stats.segments = transcript.segments.len();
        stats.speakers = transcript.speaker_count();
        Ok((transcript, stats))
    }

    fn recognize(
        &self,
        audio: &Audio,
        spans: &[SpeechSpan],
        on_segment: &mut dyn FnMut(&Segment),
    ) -> Result<Transcript, EngineError> {
        let mut transcript = Transcript::default();
        let lexicon = crate::vocab::Lexicon::default_with_user();
        for span in spans {
            if span.end - span.start < MIN_SPEECH_SECS {
                continue;
            }
            let slice = audio.slice(span.start, span.end);
            let levelled = self.speakerphone.then(|| os_asr::level::normalise(slice));
            let text = self.asr.transcribe(levelled.as_deref().unwrap_or(slice))?;
            if text.is_empty() {
                continue;
            }
            let segment = Segment {
                id: transcript.segments.len() as u32,
                start: span.start,
                end: span.end,
                speaker: None,
                hints: lexicon.hints(&text),
                check: None,
                text,
            };
            on_segment(&segment);
            transcript.segments.push(segment);
        }
        Ok(transcript)
    }
}

/// Renumber speakers 0..n in order of first appearance, so the first person
/// to speak is "Speaker 1" regardless of the clustering's internal ids.
fn merge_speaker_ids(transcript: &mut Transcript) {
    let mut order: Vec<u32> = Vec::new();
    for s in &transcript.segments {
        if let Some(id) = s.speaker
            && !order.contains(&id)
        {
            order.push(id);
        }
    }
    for s in &mut transcript.segments {
        s.speaker = s.speaker.and_then(|id| order.iter().position(|o| *o == id).map(|p| p as u32));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speakers_renumbered_by_first_appearance() {
        let seg = |id, speaker| Segment {
            id,
            start: 0.0,
            end: 1.0,
            speaker: Some(speaker),
            text: "x".into(),
            hints: Vec::new(),
            check: None,
        };
        let mut t = Transcript { segments: vec![seg(0, 2), seg(1, 0), seg(2, 2), seg(3, 5)] };
        merge_speaker_ids(&mut t);
        let speakers: Vec<_> = t.segments.iter().map(|s| s.speaker.unwrap()).collect();
        assert_eq!(speakers, vec![0, 1, 0, 2]);
    }
}
