use serde::{Deserialize, Serialize};

use crate::crosscheck::SecondOpinion;
use crate::vocab::Hint;

/// One recognised utterance. `id` is what notes cite as evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: u32,
    pub start: f32,
    pub end: f32,
    #[serde(default)]
    pub speaker: Option<u32>,
    pub text: String,
    /// Likely mis-heard veterinary terms in `text` (shown, never applied).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<Hint>,
    /// What a second recogniser heard, for segments worth a second listen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<SecondOpinion>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// Parse JSON Lines: one [`Segment`] per non-empty line.
    pub fn from_jsonl(input: &str) -> Result<Self, serde_json::Error> {
        let segments = input
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        Ok(Self { segments })
    }

    pub fn to_jsonl(&self) -> String {
        self.segments.iter().map(|s| serde_json::to_string(s).expect("segment serialises") + "\n").collect()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn get(&self, id: u32) -> Option<&Segment> {
        self.segments.iter().find(|s| s.id == id)
    }

    pub fn speaker_count(&self) -> usize {
        let mut speakers: Vec<u32> = self.segments.iter().filter_map(|s| s.speaker).collect();
        speakers.sort_unstable();
        speakers.dedup();
        speakers.len()
    }

    /// The transcript as the LLM sees it: `[id] Speaker N (mm:ss): text`.
    pub fn for_prompt(&self) -> String {
        self.segments
            .iter()
            .map(|s| {
                let who = s.speaker.map_or_else(|| "Speaker ?".to_string(), |n| format!("Speaker {}", n + 1));
                let hints: String = s
                    .hints
                    .iter()
                    .map(|h| format!(" [heard \"{}\", likely {}]", h.heard, h.likely.join(" or ")))
                    .collect();
                format!("[{}] {} ({}): {}{}\n", s.id, who, mmss(s.start), s.text, hints)
            })
            .collect()
    }
}

pub fn mmss(secs: f32) -> String {
    let total = secs.max(0.0) as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Transcript {
        Transcript {
            segments: vec![
                Segment {
                    id: 0,
                    start: 0.0,
                    end: 2.5,
                    speaker: Some(0),
                    text: "Hi, what brings Milo in?".into(),
                    hints: Vec::new(),
                    check: None,
                },
                Segment {
                    id: 1,
                    start: 2.6,
                    end: 6.0,
                    speaker: Some(1),
                    text: "He's been vomiting since Tuesday.".into(),
                    hints: Vec::new(),
                    check: None,
                },
            ],
        }
    }

    #[test]
    fn jsonl_round_trip() {
        let t = sample();
        assert_eq!(Transcript::from_jsonl(&t.to_jsonl()).unwrap(), t);
    }

    #[test]
    fn jsonl_ignores_blank_lines_and_missing_speaker() {
        let t = Transcript::from_jsonl("\n{\"id\":3,\"start\":1,\"end\":2,\"text\":\"ok\"}\n\n").unwrap();
        assert_eq!(t.segments[0].speaker, None);
        assert_eq!(t.segments.len(), 1);
    }

    #[test]
    fn prompt_format_is_stable() {
        assert_eq!(
            sample().for_prompt(),
            "[0] Speaker 1 (00:00): Hi, what brings Milo in?\n[1] Speaker 2 (00:02): He's been vomiting since Tuesday.\n"
        );
        assert_eq!(sample().speaker_count(), 2);
        assert_eq!(mmss(125.9), "02:05");
    }

    #[test]
    fn a_known_mishearing_reaches_the_prompt_as_a_hint() {
        // "apple" for Apoquel, in talk about medication: the note model sees
        // the hint like any other; the transcript text is unchanged.
        let text = "Start her on apple, sixteen milligrams twice a day.";
        let mut t = sample();
        t.segments[1].text = text.into();
        t.segments[1].hints = crate::vocab::Lexicon::load(None).hints(text);
        assert!(
            t.for_prompt().ends_with(&format!("{text} [heard \"apple\", likely apoquel]\n")),
            "{}",
            t.for_prompt()
        );
        assert_eq!(t.segments[1].text, text);
    }
}
