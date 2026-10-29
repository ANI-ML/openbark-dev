//! Speaker labels for a recording in the room (ADR-0030; a call's speaker is
//! its channel): cutting an utterance where the voice changes, and labelling
//! every segment again once the recording is complete.
//!
//! Voice detection ends an utterance at a pause, so two people answering
//! each other quickly (or a room's echo filling the gap) reach the recogniser
//! as one utterance. It's still recognised whole, so the words are as good as
//! before; the recogniser's word times then say where to cut it, and the
//! speaker model where the voice changed. Each piece is a segment of its own
//! from the moment it's shown, so segment ids never change afterwards: the
//! final pass changes speakers only.

use os_asr::{Recognised, SAMPLE_RATE, TimedToken};

use crate::transcript::Transcript;

/// How a room recording finds its speakers. The default is what the app
/// does; `openbark record` can change it to measure (hidden options).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerTuning {
    /// Voice detection: the pause that ends an utterance, seconds.
    pub min_silence_secs: f32,
    /// Voice detection: the longest utterance, seconds.
    pub max_speech_secs: f32,
    /// Cut an utterance where the voice changes; `None` keeps one label per
    /// utterance.
    pub split: Option<os_diar::SplitOptions>,
    /// Label every segment again once the recording is complete; `None`
    /// keeps the labels shown while recording. Its `speakers` is replaced by
    /// the recording's "Speakers in the room".
    pub recluster: Option<os_diar::ClusterOptions>,
}

impl Default for SpeakerTuning {
    fn default() -> Self {
        Self {
            min_silence_secs: 0.3,
            max_speech_secs: 20.0,
            split: Some(os_diar::SplitOptions::default()),
            recluster: Some(os_diar::ClusterOptions::default()),
        }
    }
}

impl SpeakerTuning {
    /// Speaker labels as before ADR-0030: one per utterance, as shown live.
    pub fn legacy() -> Self {
        Self { min_silence_secs: 0.3, max_speech_secs: 20.0, split: None, recluster: None }
    }
}

/// Part of an utterance with one voice: its samples (offsets into the
/// utterance) and its words.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub from: usize,
    pub to: usize,
    pub text: String,
}

/// Where each word after the first begins, as a sample offset into an
/// utterance of `len` samples, with the index of its first token. With the
/// token durations (Parakeet TDT) it's halfway through the gap before the
/// word; without, where the word's first token was heard.
pub fn word_starts(tokens: &[TimedToken], len: usize) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (j, t) in tokens.iter().enumerate().skip(1).filter(|(_, t)| t.starts_word()) {
        let prev = &tokens[j - 1];
        let prev_end = if prev.duration > 0.0 { (prev.start + prev.duration).min(t.start) } else { t.start };
        let at = ((prev_end + t.start) / 2.0).max(prev.start).max(0.0);
        let at = ((at * SAMPLE_RATE as f32) as usize).min(len);
        // Two words heard in the same frame: the first is kept.
        if out.last().is_none_or(|(last, _)| at > *last) {
            out.push((at, j));
        }
    }
    out
}

/// The text of some tokens, as the recogniser spells it out.
fn text_of(tokens: &[TimedToken]) -> String {
    let text: String = tokens.iter().map(|t| t.text.replace('\u{2581}', " ")).collect();
    text.trim().to_string()
}

/// A recognised utterance of `len` samples cut at `cuts` (offsets taken from
/// `starts`, see [`word_starts`]), each piece with its own words. Whole when
/// there's no cut, or when the tokens don't spell out the recognised text
/// (then it can't be divided safely).
pub fn pieces(len: usize, recognised: &Recognised, starts: &[(usize, usize)], cuts: &[usize]) -> Vec<Piece> {
    let whole = || vec![Piece { from: 0, to: len, text: recognised.text.clone() }];
    if cuts.is_empty() {
        return whole();
    }
    let tokens = &recognised.tokens;
    if text_of(tokens) != recognised.text {
        tracing::debug!("recogniser tokens don't spell its text; utterance kept whole");
        return whole();
    }
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let (mut from, mut first) = (0usize, 0usize);
    for &cut in cuts {
        let Some(&(_, j)) = starts.iter().find(|(at, _)| *at == cut) else { continue };
        if j <= first || cut <= from || cut >= len {
            continue;
        }
        out.push(Piece { from, to: cut, text: text_of(&tokens[first..j]) });
        (from, first) = (cut, j);
    }
    out.push(Piece { from, to: len, text: text_of(&tokens[first..]) });
    out
}

/// One utterance (16 kHz mono) as recognised, cut where the voice changes.
pub fn split_utterance(
    tracker: &os_diar::SpeakerTracker,
    samples: &[f32],
    recognised: &Recognised,
    options: &os_diar::SplitOptions,
) -> Vec<Piece> {
    let starts = word_starts(&recognised.tokens, samples.len());
    let cuts = if starts.is_empty() {
        Vec::new()
    } else {
        let at: Vec<usize> = starts.iter().map(|(a, _)| *a).collect();
        tracker.voice_changes(samples, &at, options)
    };
    pieces(samples.len(), recognised, &starts, &cuts)
}

/// The final labels: every segment's speaker from all the voices at once
/// (`voices[i]` is segment `i`'s, see [`os_diar::recluster`]). Only speakers
/// change; ids, times and words stay, so whatever cites a segment still
/// finds it. False (nothing changed) when there's nothing to go on.
pub fn relabel(
    transcript: &mut Transcript,
    voices: &[Option<os_diar::Voice>],
    options: &os_diar::ClusterOptions,
) -> bool {
    if voices.len() != transcript.segments.len() {
        return false;
    }
    let Some(labels) = os_diar::recluster(voices, options) else { return false };
    for (segment, label) in transcript.segments.iter_mut().zip(labels) {
        segment.speaker = label;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Segment;

    const S: usize = SAMPLE_RATE as usize;

    fn token(text: &str, start: f32, duration: f32) -> TimedToken {
        TimedToken { text: text.into(), start, duration }
    }

    /// "Is he eating? Yes, he is." with times, the answer from 1.6 s.
    fn recognised() -> Recognised {
        Recognised {
            text: "Is he eating? Yes, he is.".into(),
            tokens: vec![
                token(" Is", 0.08, 0.16),
                token(" he", 0.32, 0.16),
                token(" eat", 0.56, 0.16),
                token("ing", 0.72, 0.16),
                token("?", 0.88, 0.08),
                token(" Yes", 1.6, 0.24),
                token(",", 1.84, 0.08),
                token(" he", 2.0, 0.16),
                token(" is", 2.24, 0.16),
                token(".", 2.4, 0.08),
            ],
        }
    }

    #[test]
    fn word_starts_fall_in_the_gap_before_each_word() {
        let starts = word_starts(&recognised().tokens, 3 * S);
        let tokens: Vec<usize> = starts.iter().map(|(_, j)| *j).collect();
        assert_eq!(tokens, [1, 2, 5, 7, 8]);
        // "Yes": the question ended at 0.96 s, the answer began at 1.6 s.
        let near = |a: usize, secs: f32| a.abs_diff((secs * S as f32) as usize) <= 2;
        let yes = starts.iter().find(|(_, j)| *j == 5).unwrap().0;
        assert!(near(yes, 1.28), "{yes}");
        // Without durations, at the word's first token.
        let bare: Vec<TimedToken> =
            recognised().tokens.into_iter().map(|t| TimedToken { duration: 0.0, ..t }).collect();
        assert!(near(word_starts(&bare, 3 * S)[2].0, 1.6));
        assert!(word_starts(&[], 3 * S).is_empty());
    }

    #[test]
    fn a_cut_divides_the_words_and_the_audio() {
        let r = recognised();
        let starts = word_starts(&r.tokens, 3 * S);
        let yes = starts[2].0;
        let p = pieces(3 * S, &r, &starts, &[yes]);
        assert_eq!(
            p,
            vec![
                Piece { from: 0, to: yes, text: "Is he eating?".into() },
                Piece { from: yes, to: 3 * S, text: "Yes, he is.".into() },
            ]
        );
        // Every word kept, in order.
        let joined: Vec<&str> = p.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(joined.join(" "), r.text);
    }

    #[test]
    fn utterances_stay_whole_without_a_cut_or_when_tokens_dont_spell_the_text() {
        let r = recognised();
        let starts = word_starts(&r.tokens, 3 * S);
        let whole = vec![Piece { from: 0, to: 3 * S, text: r.text.clone() }];
        assert_eq!(pieces(3 * S, &r, &starts, &[]), whole);
        let odd = Recognised { text: "Is he eating? Yes he is".into(), ..r.clone() };
        assert_eq!(pieces(3 * S, &odd, &starts, &[starts[2].0]).len(), 1);
        // A recogniser without times (Whisper).
        let none = Recognised { text: r.text.clone(), tokens: Vec::new() };
        assert_eq!(pieces(3 * S, &none, &[], &[S]), vec![Piece { from: 0, to: 3 * S, text: r.text.clone() }]);
        // Spelled with the sentencepiece mark instead of a space.
        let marked = Recognised {
            tokens: r
                .tokens
                .iter()
                .map(|t| TimedToken { text: t.text.replace(' ', "\u{2581}"), ..t.clone() })
                .collect(),
            ..r.clone()
        };
        let starts = word_starts(&marked.tokens, 3 * S);
        assert_eq!(pieces(3 * S, &marked, &starts, &[starts[2].0])[1].text, "Yes, he is.");
    }

    fn segment(id: u32, speaker: Option<u32>, text: &str) -> Segment {
        Segment {
            id,
            start: id as f32 * 3.0,
            end: id as f32 * 3.0 + 2.5,
            speaker,
            text: text.into(),
            hints: Vec::new(),
            check: None,
        }
    }

    fn voice(axis: usize) -> Option<os_diar::Voice> {
        let mut embedding = vec![0.0f32; 4];
        embedding[axis] = 1.0;
        Some(os_diar::Voice { embedding, secs: 2.5, band: -10.0 })
    }

    #[test]
    fn relabelling_changes_speakers_only() {
        // Live, the third segment was taken for the first voice.
        let mut t = Transcript {
            segments: vec![
                segment(0, Some(0), "What brings Milo in?"),
                segment(1, Some(1), "He's been vomiting."),
                segment(2, Some(0), "Since Tuesday."),
            ],
        };
        let before = t.clone();
        assert!(relabel(&mut t, &[voice(0), voice(1), voice(1)], &os_diar::ClusterOptions::default()));
        let speakers: Vec<Option<u32>> = t.segments.iter().map(|s| s.speaker).collect();
        assert_eq!(speakers, [Some(0), Some(1), Some(1)]);
        for (a, b) in t.segments.iter().zip(&before.segments) {
            assert_eq!((a.id, a.start, a.end, &a.text), (b.id, b.start, b.end, &b.text));
        }
        // Voices that don't line up with the segments change nothing.
        let mut u = before.clone();
        assert!(!relabel(&mut u, &[voice(0)], &os_diar::ClusterOptions::default()));
        assert_eq!(u, before);
    }
}
