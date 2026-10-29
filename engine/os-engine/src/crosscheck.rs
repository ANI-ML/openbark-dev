//! Second-opinion recognition (M1 item 4).
//!
//! The primary recogniser (Parakeet) occasionally drops a magnitude word
//! ("one hundred millilitres" heard as "10 milliliters") or garbles a term
//! ("otitis" as "artitis"). The faithfulness pass can't catch that: the note
//! matches the transcript, and the transcript is wrong. So segments that
//! matter are recognised again by a different model (Whisper turbo) and
//! compared:
//! - numbers that differ are recorded; notes that rely on them are flagged
//! - known veterinary terms the second model heard become visible hints
//!
//! Nothing in the transcript is replaced: both readings are kept for review.

use serde::{Deserialize, Serialize};

use crate::numbers::{extract_numbers, normalize_spoken};
use crate::transcript::Segment;
use crate::vocab::{Hint, Lexicon};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondOpinion {
    /// What the second recogniser heard.
    pub text: String,
    /// Numbers in the primary text that the second recogniser did not hear.
    pub primary_only: Vec<String>,
    /// Numbers the second recogniser heard that the primary did not.
    pub second_only: Vec<String>,
}

impl SecondOpinion {
    pub fn agrees(&self) -> bool {
        self.primary_only.is_empty() && self.second_only.is_empty()
    }
}

fn numbers(text: &str) -> Vec<String> {
    extract_numbers(&normalize_spoken(text))
}

/// Multiset difference a - b.
fn minus(a: &[String], b: &[String]) -> Vec<String> {
    let mut rest = b.to_vec();
    a.iter()
        .filter(|x| match rest.iter().position(|y| y == *x) {
            Some(i) => {
                rest.remove(i);
                false
            }
            None => true,
        })
        .cloned()
        .collect()
}

pub fn compare(primary: &str, second: &str) -> SecondOpinion {
    let (p, s) = (numbers(primary), numbers(second));
    SecondOpinion { text: second.to_string(), primary_only: minus(&p, &s), second_only: minus(&s, &p) }
}

/// Endings typical of drug and condition names, used to pick which segments
/// deserve a second listen.
const CLINICAL_ENDINGS: &[&str] = &[
    "itis", "osis", "emia", "aemia", "ectomy", "otomy", "pathy", "plasia", "algia", "oma", "azole", "mycin",
    "cillin", "oxacin", "pril", "olol", "sartan", "tidine", "prazole", "dronate", "mab", "pam", "lam", "fen",
    "coxib", "pine", "zine", "done", "olone", "sone", "mide",
];

/// Does this segment merit a second recogniser? Numbers, existing hints,
/// words that look like drug or condition names, or talk about medication
/// ("give", "twice a day", "for itch"), where a brand name may have been
/// heard as an ordinary word ("apple" for Apoquel).
pub fn worth_checking(segment: &Segment) -> bool {
    if !numbers(&segment.text).is_empty()
        || !segment.hints.is_empty()
        || crate::vocab::has_drug_cue(&segment.text)
    {
        return true;
    }
    segment.text.split(|c: char| !c.is_alphabetic()).any(|w| {
        let w = w.to_lowercase();
        w.len() >= 6 && CLINICAL_ENDINGS.iter().any(|e| w.ends_with(e))
    })
}

/// Hints from the second reading: lexicon terms it heard that the primary
/// text lacks, attached to the primary word that most resembles them, or
/// else to the word standing in the same place between words both readings
/// share ("start apple sixteen" against "start Apoquel 16").
pub fn second_opinion_hints(lexicon: &Lexicon, primary: &str, second: &str) -> Vec<Hint> {
    let primary_lower = primary.to_lowercase();
    lexicon
        .terms_in(second)
        .into_iter()
        .filter(|term| !primary_lower.contains(term.as_str()))
        .filter_map(|term| {
            let heard =
                lexicon.closest_word(primary, &term).or_else(|| lexicon.aligned_word(primary, second, &term));
            heard.map(|heard| Hint { heard, likely: vec![term], kind: "second recogniser".into() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_magnitude_is_a_disagreement() {
        let o = compare(
            "Also give her 10 milliliters of lactated ringers fluids",
            "Also give her 100 milliliters of lactated ringers fluids",
        );
        assert!(!o.agrees());
        assert_eq!(o.primary_only, vec!["10"]);
        assert_eq!(o.second_only, vec!["100"]);
    }

    #[test]
    fn spelled_and_written_numbers_agree() {
        let o = compare("Her weight today is twelve point six kilos.", "Her weight today is 12.6 kilos.");
        assert!(o.agrees(), "{o:?}");
        let o = compare("one milligram per kilo", "1 mg per kilo");
        assert!(o.agrees(), "{o:?}");
    }

    #[test]
    fn chooses_segments_that_matter() {
        let seg = |text: &str| Segment {
            id: 0,
            start: 0.0,
            end: 1.0,
            speaker: None,
            text: text.into(),
            hints: vec![],
            check: None,
        };
        assert!(worth_checking(&seg("Temperature is 38.5.")));
        assert!(worth_checking(&seg("The artitis has resolved")));
        assert!(worth_checking(&seg("send home some mirtazapine")));
        assert!(!worth_checking(&seg("That's a relief, thank you so much.")));
        // A brand heard as an ordinary word, in talk about medication.
        assert!(worth_checking(&seg("We'll put her on apple for the itch")));
        assert!(!worth_checking(&seg("She ate an apple this morning")));
    }

    #[test]
    fn second_reading_suggests_the_term_it_heard() {
        let lex = Lexicon::load(None);
        let h = second_opinion_hints(
            &lex,
            "The artitis has resolved, so no more medication.",
            "The otitis has resolved, so no more medication.",
        );
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!(h[0].heard, "artitis");
        assert_eq!(h[0].likely, vec!["otitis"]);
        assert!(second_opinion_hints(&lex, "Give maropitant now.", "Give maropitant now.").is_empty());
    }

    #[test]
    fn a_brand_heard_as_another_word_is_placed_by_its_neighbours() {
        let lex = Lexicon::load(None);
        let h = second_opinion_hints(
            &lex,
            "Start apple sixteen milligrams twice a day.",
            "Start Apoquel 16 milligrams twice a day.",
        );
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!((h[0].heard.as_str(), h[0].likely[0].as_str()), ("apple", "apoquel"));
        // Nothing in the same place: no hint.
        assert!(second_opinion_hints(&lex, "Thank you so much.", "Apoquel.").is_empty());
    }
}
