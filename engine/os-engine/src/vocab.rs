//! Veterinary vocabulary hints.
//!
//! Speech-to-text mishears drug and condition names ("marapitin" for
//! maropitant). Rather than silently rewrite the transcript (a wrong
//! correction could swap one drug for another), each likely mishearing gets a
//! visible hint: the transcript keeps what was heard, the clinician sees the
//! suggestion, and the note model reads both. Hotword biasing was tried first
//! and made Parakeet worse (M1 spike).
//!
//! Matching: a word or word pair that is not itself a known term, compared
//! with each lexicon term by Jaro-Winkler similarity over a simplified sound
//! key. When the two best terms are close, both are offered.

use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../../../packages/vocab/veterinary.tsv");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hint {
    /// The words as recognised.
    pub heard: String,
    /// Likely intended terms, best first (one, or two when ambiguous).
    pub likely: Vec<String>,
    pub kind: String,
}

#[derive(Debug, Clone)]
struct Term {
    text: String,
    key: String,
    kind: String,
    words: usize,
}

#[derive(Debug, Clone)]
pub struct Lexicon {
    terms: Vec<Term>,
}

/// A rough sound key: lowercase letters only, common spelling variants
/// merged, doubled letters collapsed, vowels after the first letter reduced.
fn sound_key(s: &str) -> String {
    let mut t: String = s.to_lowercase().chars().filter(|c| c.is_ascii_alphabetic()).collect();
    for (a, b) in [("ph", "f"), ("ck", "k"), ("qu", "kw"), ("x", "ks"), ("z", "s"), ("y", "i"), ("c", "k")] {
        t = t.replace(a, b);
    }
    let mut out = String::new();
    for (i, c) in t.chars().enumerate() {
        let vowel = "aeiou".contains(c);
        let c = if vowel && i > 0 { 'a' } else { c };
        if out.ends_with(c) {
            continue;
        }
        out.push(c);
    }
    out
}

/// Jaro-Winkler similarity in [0, 1].
fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_match = vec![false; a.len()];
    let mut b_match = vec![false; b.len()];
    let mut matches = 0usize;
    for i in 0..a.len() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(b.len());
        for j in lo..hi {
            if !b_match[j] && a[i] == b[j] {
                a_match[i] = true;
                b_match[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let (mut k, mut transpositions) = (0usize, 0usize);
    for i in 0..a.len() {
        if a_match[i] {
            while !b_match[k] {
                k += 1;
            }
            if a[i] != b[k] {
                transpositions += 1;
            }
            k += 1;
        }
    }
    let m = matches as f64;
    let jaro = (m / a.len() as f64 + m / b.len() as f64 + (m - transpositions as f64 / 2.0) / m) / 3.0;
    let prefix = a.iter().zip(&b).take(4).take_while(|(x, y)| x == y).count() as f64;
    jaro + prefix * 0.1 * (1.0 - jaro)
}

/// Similarity required to suggest a term (mean of spelling and sound), with
/// spelling alone at least `SPELLING_FLOOR` so sound keys can't match on
/// their own ("milligram" is not "melena").
///
/// Drug and vaccine names are long and distinctive, so a lower bar is safe.
/// Condition and procedure names are short and crowded ("artitis" scores
/// 0.948 for arthritis but 0.783 for otitis, the word actually said), so they
/// need a near-exact match; a missing hint is better than a misleading one.
const THRESHOLD_DRUG: f64 = 0.87;
const THRESHOLD_OTHER: f64 = 0.95;
const SPELLING_FLOOR: f64 = 0.75;

/// Everyday clinical English that is never hinted.
const COMMON: &[&str] = &[
    "milligram",
    "milligrams",
    "milliliter",
    "milliliters",
    "millilitre",
    "millilitres",
    "kilogram",
    "kilograms",
    "kilo",
    "kilos",
    "pounds",
    "degrees",
    "temperature",
    "medication",
    "medications",
    "medicine",
    "injection",
    "injections",
    "ointment",
    "tablet",
    "tablets",
    "capsule",
    "capsules",
    "appetite",
    "technician",
    "dehydration",
    "dehydrated",
    "examination",
    "examine",
    "recommend",
    "recommendation",
    "appointment",
    "tablespoon",
    "teaspoon",
    "discomfort",
    "uncomfortable",
    "vomiting",
    "diarrhea",
    "diarrhoea",
    "constipation",
    "breathing",
    "swelling",
    "bloodwork",
    "vaccine",
    "vaccines",
    "vaccination",
    "booster",
    "allergies",
    "allergy",
    "infection",
    "surgery",
    "anesthesia",
    "anaesthesia",
    "antibiotic",
    "antibiotics",
    "parasite",
    "parasites",
    "prescription",
    "condition",
    "emergency",
    "hospital",
    "behaviour",
    "behavior",
    "weight",
    "heartbeat",
    "kidneys",
    "thyroid",
    "pressure",
    "everything",
    "something",
    "yesterday",
    "tomorrow",
    "morning",
    "evening",
    "probably",
    "definitely",
];
/// If the runner-up is within this margin, offer both.
const AMBIGUITY: f64 = 0.03;

impl Lexicon {
    pub fn parse(tsv: &str) -> Self {
        let terms = tsv
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let (text, kind) = l.split_once('\t').unwrap_or((l, "term"));
                let text = text.trim().to_lowercase();
                (!text.is_empty()).then(|| Term {
                    key: sound_key(&text),
                    words: text.split_whitespace().count(),
                    kind: kind.trim().to_string(),
                    text,
                })
            })
            .collect();
        Self { terms }
    }

    /// The built-in lexicon plus the user's additions, if any.
    pub fn load(user_file: Option<&std::path::Path>) -> Self {
        let mut lex = Self::parse(BUILTIN);
        if let Some(extra) = user_file.and_then(|p| std::fs::read_to_string(p).ok()) {
            lex.terms.extend(Self::parse(&extra).terms);
        }
        lex
    }

    /// Built-in terms plus `<local data>/OpenBark/vocab.tsv`, if present.
    pub fn default_with_user() -> Self {
        let user = std::env::var_os("OPENBARK_VOCAB")
            .map(std::path::PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|d| d.join("OpenBark").join("vocab.tsv")));
        Self::load(user.as_deref())
    }

    /// Lexicon terms that appear (as whole words) in `text`.
    pub fn terms_in(&self, text: &str) -> Vec<String> {
        let norm: String = text
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
            .collect();
        let padded = format!(" {} ", norm.split_whitespace().collect::<Vec<_>>().join(" "));
        let mut found: Vec<String> = self
            .terms
            .iter()
            .filter(|t| padded.contains(&format!(" {} ", t.text)))
            .map(|t| t.text.clone())
            .collect();
        found.dedup();
        found
    }

    /// The word in `text` that most resembles `term` (for attaching a hint),
    /// if any is reasonably close and isn't itself a known term.
    pub fn closest_word(&self, text: &str, term: &str) -> Option<String> {
        let words: Vec<&str> =
            text.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
        // Single words and adjacent pairs ("your analysis" for "urinalysis").
        let mut candidates: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        candidates.extend(words.windows(2).map(|p| format!("{} {}", p[0], p[1])));
        candidates
            .into_iter()
            .filter(|c| c.chars().filter(|ch| ch.is_alphabetic()).count() >= 5 && !self.is_term(c))
            .map(|c| (jaro_winkler(&c.to_lowercase().replace(' ', ""), term), c))
            .filter(|(s, _)| *s >= 0.80)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, c)| c)
    }

    fn is_term(&self, phrase: &str) -> bool {
        let p = phrase.to_lowercase();
        self.terms.iter().any(|t| t.text == p || t.text.trim_end_matches('s') == p.trim_end_matches('s'))
    }

    /// Hints for one line of transcript.
    pub fn hints(&self, text: &str) -> Vec<Hint> {
        let words: Vec<&str> = text
            .split(|c: char| c.is_whitespace() || c == '-')
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
            .filter(|w| !w.is_empty())
            .collect();
        let mut hints: Vec<Hint> = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let mut best_here: Option<(Hint, usize)> = None;
            for n in [2usize, 1] {
                if i + n > words.len() {
                    continue;
                }
                let phrase = words[i..i + n].join(" ");
                let letters = phrase.chars().filter(|c| c.is_alphabetic()).count();
                let lower = phrase.to_lowercase();
                if letters < 5 || self.is_term(&phrase) || lower.split(' ').any(|w| COMMON.contains(&w)) {
                    continue;
                }
                let key = sound_key(&phrase);
                let mut scored: Vec<(f64, &Term)> = self
                    .terms
                    .iter()
                    .filter(|t| t.words == n)
                    .filter_map(|t| {
                        let spelling = jaro_winkler(&lower, &t.text);
                        let score = (spelling + jaro_winkler(&key, &t.key)) / 2.0;
                        let bar = if matches!(t.kind.as_str(), "drug" | "vaccine") {
                            THRESHOLD_DRUG
                        } else {
                            THRESHOLD_OTHER
                        };
                        (spelling >= SPELLING_FLOOR && score >= bar).then_some((score, t))
                    })
                    .collect();
                scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                if let Some((top, term)) = scored.first() {
                    let mut likely = vec![term.text.clone()];
                    if let Some((second, t2)) = scored.get(1)
                        && top - second < AMBIGUITY
                        && t2.text != term.text
                    {
                        likely.push(t2.text.clone());
                    }
                    let hint = Hint { heard: phrase.clone(), likely, kind: term.kind.clone() };
                    best_here = Some((hint, n));
                    break;
                }
            }
            match best_here {
                Some((h, n)) => {
                    hints.push(h);
                    i += n;
                }
                None => i += 1,
            }
        }
        hints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex() -> Lexicon {
        Lexicon::load(None)
    }

    #[test]
    fn suggests_misheard_drug_names() {
        let h = lex().hints("an anti-nausea injection, marapitin, at one milligram per kilo");
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!(h[0].heard, "marapitin");
        assert_eq!(h[0].likely[0], "maropitant");
        let h = lex().hints("I'm going to send home Myrtizepine ointment");
        assert_eq!(h[0].likely[0], "mirtazapine", "{h:?}");
        let h = lex().hints("send home MurdaZipine ointment");
        assert_eq!(h[0].likely[0], "mirtazapine", "{h:?}");
    }

    #[test]
    fn leaves_ordinary_speech_and_correct_terms_alone() {
        assert!(lex().hints("She stopped shaking her head after about four days").is_empty());
        assert!(lex().hints("Give maropitant and recheck in three days").is_empty());
        assert!(lex().hints("Her temperature is thirty nine point one").is_empty());
        assert!(lex().hints("We discussed the radiographs and the urinalysis").is_empty());
    }

    #[test]
    fn crowded_condition_names_get_no_misleading_hint() {
        // Said "otitis", heard "artitis": arthritis is closer, so offer nothing.
        assert!(lex().hints("The artitis has resolved").is_empty());
    }

    #[test]
    fn user_terms_are_added() {
        let mut l = Lexicon::parse("bravecto\tdrug\n");
        l.terms.extend(Lexicon::parse("zorbiflex\tdrug\n").terms);
        assert_eq!(l.hints("we gave zorbyflex today")[0].likely[0], "zorbiflex");
    }

    #[test]
    fn closest_word_considers_pairs_and_ignores_short_words() {
        let l = lex();
        assert_eq!(
            l.closest_word("the blood work and your analysis together, and the x rays", "urinalysis")
                .as_deref(),
            Some("your analysis")
        );
        assert_eq!(l.closest_word("The artitis has resolved", "otitis").as_deref(), Some("artitis"));
        assert_eq!(l.closest_word("thank you so much", "otitis"), None);
    }

    #[test]
    fn similarity_is_sane() {
        assert!(jaro_winkler("martha", "marhta") > 0.95);
        assert!(jaro_winkler("abc", "xyz") < 0.1);
        assert_eq!(sound_key("Phenobarbital"), sound_key("fenobarbital"));
    }
}
