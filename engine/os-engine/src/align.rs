//! Claim-level evidence alignment (M2 item 1).
//!
//! The number check catches invented numbers. This catches invented words:
//! each statement's content words must appear in the transcript lines it
//! cites. Lexical and deterministic, no extra model:
//! - stopwords and charting filler ("patient", "noted", "reported") are ignored;
//! - negations ("no", "not", "never") count as content, so "no vomiting"
//!   needs a "no" in the cited lines;
//! - veterinary abbreviations and their spoken forms are folded together
//!   (SC / subcutaneous / under the skin, LRS / lactated Ringer's, PO / by mouth);
//! - spelling differences pass through a fuzzy match (diarrhoea / diarrhea);
//! - vocabulary hints' likely terms and the second recogniser's words count
//!   as said in that line.
//!
//! A statement whose words aren't in its cited lines is flagged for checking
//! (never a hard fail: paraphrase exists). When the words are found on other
//! lines, those lines are suggested as the right citation.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::transcript::Transcript;

/// Words that carry no claim.
const STOPWORDS: &[&str] = &[
    "a",
    "about",
    "above",
    "after",
    "again",
    "all",
    "also",
    "am",
    "an",
    "and",
    "any",
    "are",
    "around",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "being",
    "both",
    "but",
    "by",
    "can",
    "could",
    "did",
    "do",
    "does",
    "doing",
    "during",
    "each",
    "few",
    "for",
    "from",
    "further",
    "had",
    "has",
    "have",
    "having",
    "he",
    "her",
    "here",
    "hers",
    "herself",
    "him",
    "himself",
    "his",
    "how",
    "i",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "itself",
    "just",
    "me",
    "more",
    "most",
    "my",
    "myself",
    "of",
    "off",
    "on",
    "once",
    "only",
    "or",
    "other",
    "our",
    "ours",
    "out",
    "over",
    "own",
    "same",
    "she",
    "should",
    "so",
    "some",
    "such",
    "than",
    "that",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "through",
    "to",
    "too",
    "under",
    "until",
    "up",
    "very",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "while",
    "who",
    "whom",
    "why",
    "will",
    "with",
    "would",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
    "s",
    "t",
    "d",
    "ll",
    "m",
    "re",
    "ve",
    "y",
    "ok",
    "okay",
    "yes",
    "yeah",
    "um",
    "uh",
    "so",
    "well",
    "like",
    "get",
    "got",
    "go",
    "going",
    "let",
    "lets",
    "may",
    "might",
    "must",
    "shall",
    "per",
    "via",
    "etc",
    "e",
    "g",
    "ie",
    "eg",
    "x",
];

/// French words that carry no claim, accents folded ("deja" for "déjà").
/// Words that are also English content words are left out ("pour", "plus",
/// "son", "car", "comment", "par"), so an English statement keeps them.
const FR_STOPWORDS: &[&str] = &[
    "le",
    "la",
    "les",
    "en",
    "un",
    "une",
    "des",
    "du",
    "de",
    "au",
    "aux",
    "et",
    "ou",
    "mais",
    "donc",
    "que",
    "qui",
    "quoi",
    "ce",
    "cet",
    "cette",
    "ces",
    "ca",
    "cela",
    "ceci",
    "se",
    "sa",
    "ses",
    "mon",
    "ma",
    "mes",
    "ton",
    "ta",
    "tes",
    "notre",
    "nos",
    "votre",
    "vos",
    "leur",
    "leurs",
    "il",
    "elle",
    "ils",
    "elles",
    "nous",
    "vous",
    "je",
    "tu",
    "lui",
    "ne",
    "est",
    "sont",
    "ai",
    "ont",
    "ete",
    "etre",
    "avoir",
    "fait",
    "faire",
    "avec",
    "dans",
    "sur",
    "sous",
    "chez",
    "tres",
    "bien",
    "aussi",
    "comme",
    "si",
    "oui",
    "alors",
    "ici",
    "quand",
    "tout",
    "tous",
    "toute",
    "toutes",
    "encore",
    "deja",
    "puis",
    "ensuite",
    "apres",
    "avant",
    "pendant",
    "depuis",
    "entre",
    "vers",
    "autre",
    "meme",
    "peu",
    "moins",
    "etait",
    "sera",
    "va",
    "vais",
    "peut",
    "doit",
    "faut",
    "voila",
    "euh",
    "hum",
    "bah",
    "aujourd'hui",
];

/// Words a model adds when charting that nobody needs to have said.
const FILLER: &[&str] = &[
    "patient",
    "pet",
    "owner",
    "client",
    "noted",
    "note",
    "reported",
    "reports",
    "report",
    "states",
    "stated",
    "state",
    "says",
    "said",
    "per",
    "today",
    "currently",
    "current",
    "presented",
    "presents",
    "presenting",
    "seen",
    "observed",
    "described",
    "describes",
    "mentioned",
    "advised",
    "recommended",
    "recommend",
    "recommendation",
    "discussed",
    "discuss",
    "plan",
    "planned",
    "will",
    "given",
    "noticed",
    "notice",
    "visit",
    "history",
    "appointment",
    "veterinarian",
    "vet",
    "dr",
    "doctor",
    "consultation",
    "exam",
    "examination",
    "assessment",
    "findings",
    "finding",
    "within",
    "approximately",
    "approx",
    "estimated",
    "estimate",
    "possible",
    "possibly",
    "likely",
    "suspected",
    "suspect",
    "concern",
    "concerned",
    "consistent",
    "including",
    "include",
    "includes",
    "due",
    "since",
    "ago",
    "per",
    "old",
    "year",
    "years",
    "aged",
    "age",
    // Units sit beside numbers, which the number check covers.
    "bpm",
    "min",
    "mins",
    "kg",
    "kgs",
    "mg",
    "ml",
    // The same in French, accents folded.
    "proprietaire",
    "cliente",
    "rapporte",
    "signale",
    "veterinaire",
    "docteur",
    "madame",
    "monsieur",
    "examen",
    "visite",
    "recommande",
    "conseille",
    "prevu",
    "discute",
    "environ",
    "probablement",
];

/// Abbreviations, spoken forms and synonyms folded to one token.
const FOLD: &[(&str, &str)] = &[
    ("sc", "subcutaneous"),
    ("sq", "subcutaneous"),
    ("subq", "subcutaneous"),
    ("subcut", "subcutaneous"),
    ("subcutaneously", "subcutaneous"),
    ("skin", "subcutaneous"),
    ("po", "oral"),
    ("orally", "oral"),
    ("mouth", "oral"),
    ("iv", "intravenous"),
    ("intravenously", "intravenous"),
    ("vein", "intravenous"),
    ("im", "intramuscular"),
    ("intramuscularly", "intramuscular"),
    ("muscle", "intramuscular"),
    ("bid", "twice"),
    ("sid", "daily"),
    ("q24h", "daily"),
    ("q12h", "twice"),
    ("tid", "thrice"),
    ("q8h", "thrice"),
    ("hr", "heart"),
    ("pulse", "heart"),
    ("rr", "respiratory"),
    ("breathing", "respiratory"),
    ("breaths", "respiratory"),
    ("mm", "mucous"),
    ("gums", "mucous"),
    ("gum", "mucous"),
    ("crt", "capillary"),
    ("bcs", "condition"),
    ("temp", "temperature"),
    ("lrs", "lactated"),
    ("ringers", "lactated"),
    ("ringer", "lactated"),
    ("xray", "radiograph"),
    ("xrays", "radiograph"),
    ("radiographs", "radiograph"),
    ("rads", "radiograph"),
    ("bloodwork", "blood"),
    ("labwork", "blood"),
    ("cbc", "blood"),
    ("chem", "blood"),
    ("vax", "vaccine"),
    ("vaccination", "vaccine"),
    ("vaccinated", "vaccine"),
    ("booster", "vaccine"),
    ("kilos", "kg"),
    ("kilo", "kg"),
    ("kilograms", "kg"),
    ("kilogram", "kg"),
    ("pounds", "lb"),
    ("lbs", "lb"),
    ("ml", "ml"),
    ("millilitres", "ml"),
    ("milliliters", "ml"),
    ("cc", "ml"),
    ("mg", "mg"),
    ("milligrams", "mg"),
    ("milligram", "mg"),
    ("emesis", "vomit"),
    ("vomiting", "vomit"),
    ("vomited", "vomit"),
    ("vomits", "vomit"),
    ("throwing", "vomit"),
    ("anorexia", "eat"),
    ("inappetence", "eat"),
    ("inappetent", "eat"),
    ("eating", "eat"),
    ("appetite", "eat"),
    ("lethargy", "lethargic"),
    ("lethargic", "lethargic"),
    ("tired", "lethargic"),
    ("dx", "diagnosis"),
    ("hx", "history"),
    ("rx", "prescription"),
    ("sx", "surgery"),
    ("dvm", "veterinarian"),
    ("tech", "technician"),
    ("recheck", "recheck"),
    ("revisit", "recheck"),
    ("ear", "ear"),
    ("ears", "ear"),
    ("otic", "ear"),
    ("aural", "ear"),
    ("ocular", "eye"),
    ("eyes", "eye"),
    ("dental", "teeth"),
    ("tooth", "teeth"),
    ("dentition", "teeth"),
    ("abdominal", "abdomen"),
    ("belly", "abdomen"),
    ("tummy", "abdomen"),
    ("stomach", "abdomen"),
    ("dehydration", "dehydrated"),
    ("hydration", "dehydrated"),
    ("diarrhoea", "diarrhea"),
    ("faeces", "stool"),
    ("feces", "stool"),
    ("poop", "stool"),
    ("stools", "stool"),
    ("urination", "urine"),
    ("urinating", "urine"),
    ("pee", "urine"),
    ("peeing", "urine"),
    ("palpation", "palpate"),
    ("palpated", "palpate"),
    ("palpable", "palpate"),
    ("murmurs", "murmur"),
    ("auscultation", "listen"),
    ("ausculted", "listen"),
    ("sounds", "sound"),
    ("lungs", "lung"),
    ("pulmonary", "lung"),
    ("cardiac", "heart"),
    ("hearts", "heart"),
    ("declined", "decline"),
    ("declines", "decline"),
    ("deferred", "decline"),
    ("defer", "decline"),
    ("negative", "no"),
    ("denies", "no"),
    ("without", "no"),
    ("none", "no"),
    ("nothing", "no"),
    ("n't", "no"),
    ("nt", "no"),
    ("not", "no"),
    ("never", "no"),
    ("feel", "palpate"),
    ("felt", "palpate"),
    ("feels", "palpate"),
    ("initial", "first"),
    ("initially", "first"),
    ("food", "diet"),
    ("foods", "diet"),
    ("fed", "diet"),
    ("feeding", "diet"),
    ("hold", "decline"),
    ("wait", "decline"),
    ("postpone", "decline"),
    ("resp", "respiratory"),
    ("respiration", "respiratory"),
    ("respirations", "respiratory"),
    ("intake", "drink"),
    ("drinking", "drink"),
    ("drinks", "drink"),
    ("thirst", "drink"),
    ("increased", "more"),
    ("increase", "more"),
    ("decreased", "less"),
    ("decrease", "less"),
    // French negations, accents folded: "pas de vomissements" needs a
    // negation in the cited line, as "no vomiting" does.
    ("pas", "no"),
    ("aucun", "no"),
    ("aucune", "no"),
    ("ni", "no"),
    ("sans", "no"),
    ("jamais", "no"),
    ("rien", "no"),
    ("negatif", "no"),
    ("negatifs", "no"),
    ("negatives", "no"),
];

/// Negations that must be supported like any content word.
const NEGATIONS: &[&str] = &["no"];

fn fold(word: &str) -> String {
    let w = word.trim_matches('\'');
    FOLD.iter().find(|(from, _)| *from == w).map(|(_, to)| to.to_string()).unwrap_or_else(|| stem(w))
}

/// Lowercase letters without their accents ("hyperhémie" is "hyperhemie",
/// "œil" is "oeil"), so a word matches however it was spelled or heard.
pub fn fold_accents(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => out.push('a'),
            'ç' => out.push('c'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' | 'í' | 'ì' => out.push('i'),
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => out.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => out.push('u'),
            'ÿ' => out.push('y'),
            'ñ' => out.push('n'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            _ => out.push(c),
        }
    }
    out
}

/// A French word without its elided article or pronoun: "l'oeil" is
/// "oeil", "d'ecoulement" is "ecoulement", "n'a" is "a".
fn without_elision(w: &str) -> &str {
    for prefix in ["qu'", "l'", "d'", "n'", "j'", "s'", "c'", "m'", "t'"] {
        if let Some(rest) = w.strip_prefix(prefix)
            && rest.chars().next().is_some_and(char::is_alphanumeric)
        {
            return rest;
        }
    }
    w
}

/// A crude English stem: plurals and common verb endings.
fn stem(w: &str) -> String {
    let w = w.strip_suffix("'s").unwrap_or(w);
    for (suffix, min, replace) in [
        ("ies", 4, "y"),
        ("ing", 5, ""),
        ("edly", 6, ""),
        ("ed", 4, ""),
        ("es", 4, ""),
        ("ly", 5, ""),
        ("s", 3, ""),
    ] {
        if w.len() > min
            && let Some(base) = w.strip_suffix(suffix)
        {
            if suffix == "s" && (base.ends_with('s') || base.ends_with('u') || base.ends_with('i')) {
                return w.to_string(); // "status", "virus", "pancreatitis"
            }
            // "trimming" and "trimmed" are "trim"; "falling" stays "fall".
            let b = base.as_bytes();
            if (suffix == "ing" || suffix == "ed")
                && b.len() >= 3
                && b[b.len() - 1] == b[b.len() - 2]
                && !b"aeioulsfz".contains(&b[b.len() - 1])
            {
                return base[..base.len() - 1].to_string();
            }
            return format!("{base}{replace}");
        }
    }
    w.to_string()
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(|w| fold_accents(&w.to_lowercase()).replace("n't", " not"))
        .flat_map(|w| w.split_whitespace().map(|w| without_elision(w).to_string()).collect::<Vec<_>>())
}

/// The words of a statement that make a claim.
pub fn content_tokens(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for w in words(text) {
        if w.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue; // numbers have their own check
        }
        let folded = fold(&w);
        let negation = NEGATIONS.contains(&folded.as_str());
        let ignored = STOPWORDS.contains(&w.as_str())
            || FR_STOPWORDS.contains(&w.as_str())
            || FILLER.contains(&w.as_str())
            || folded.len() < 2;
        if !negation && ignored {
            continue;
        }
        if seen.insert(folded.clone()) {
            out.push(folded);
        }
    }
    out
}

/// Tokens a transcript line supports: its words, folded, plus hints' likely
/// terms and what the second recogniser heard.
fn line_tokens(seg: &crate::transcript::Segment) -> HashSet<String> {
    let mut text = seg.text.clone();
    for h in &seg.hints {
        for l in &h.likely {
            text.push(' ');
            text.push_str(l);
        }
    }
    if let Some(check) = &seg.check {
        text.push(' ');
        text.push_str(&check.text);
    }
    words(&text).map(|w| fold(&w)).collect()
}

fn matches(token: &str, line: &HashSet<String>) -> bool {
    if line.contains(token) {
        return true;
    }
    token.len() >= 5
        && line.iter().any(|w| {
            w.len() >= 5
                && (w.starts_with(token)
                    || token.starts_with(w.as_str())
                    || crate::vocab::jaro_winkler(token, w) >= 0.90)
        })
}

/// A statement whose words aren't in the lines it cites.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnsupportedStatement {
    pub path: String,
    /// Content words not found in the cited lines.
    pub missing: Vec<String>,
    pub cited: Vec<u32>,
    /// Lines that do contain the words, when there are some.
    pub suggested: Vec<u32>,
}

pub struct Aligner {
    lines: HashMap<u32, HashSet<String>>,
    order: Vec<u32>,
}

impl Aligner {
    pub fn new(transcript: &Transcript) -> Self {
        let lines = transcript.segments.iter().map(|s| (s.id, line_tokens(s))).collect();
        let order = transcript.segments.iter().map(|s| s.id).collect();
        Self { lines, order }
    }

    /// Check one statement's text against its cited lines.
    pub fn check(&self, path: &str, text: &str, cited: &[u32]) -> Option<UnsupportedStatement> {
        let tokens = content_tokens(text);
        if tokens.is_empty() {
            return None;
        }
        let in_cited = |t: &String| cited.iter().filter_map(|id| self.lines.get(id)).any(|l| matches(t, l));
        let missing: Vec<String> = tokens.iter().filter(|t| !in_cited(t)).cloned().collect();
        // Most words present: a paraphrase. A missing negation always counts.
        let negation_missing = missing.iter().any(|m| NEGATIONS.contains(&m.as_str()));
        let allowed = tokens.len() * 2 / 5; // up to 40% of the words may be rewording
        if missing.len() <= allowed && !negation_missing {
            return None;
        }
        Some(UnsupportedStatement {
            path: path.to_string(),
            suggested: self.suggest(&tokens),
            missing,
            cited: cited.to_vec(),
        })
    }

    /// Up to three lines that together contain most of the words.
    fn suggest(&self, tokens: &[String]) -> Vec<u32> {
        let mut left: Vec<&String> = tokens.iter().collect();
        let mut picked = Vec::new();
        while picked.len() < 3 && !left.is_empty() {
            let best = self
                .order
                .iter()
                .filter(|id| !picked.contains(*id))
                .map(|id| (*id, left.iter().filter(|t| matches(t, &self.lines[id])).count()))
                .max_by_key(|(id, n)| (*n, std::cmp::Reverse(*id)));
            match best {
                Some((id, n)) if n > 0 => {
                    picked.push(id);
                    left.retain(|t| !matches(t, &self.lines[&id]));
                }
                _ => break,
            }
        }
        let covered = tokens.len() - left.len();
        if covered * 4 >= tokens.len() * 3 {
            picked.sort_unstable();
            picked
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Segment;

    fn transcript() -> Transcript {
        let seg = |id, text: &str| Segment {
            id,
            start: 0.0,
            end: 1.0,
            speaker: Some(0),
            text: text.into(),
            hints: Vec::new(),
            check: None,
        };
        Transcript {
            segments: vec![
                seg(0, "She started throwing up on Monday, at least once a day."),
                seg(1, "Her gums are pink but a little tacky, and the skin tent is slow."),
                seg(2, "We gave ten milliliters of lactated Ringer's under the skin."),
                seg(3, "I don't feel any mass in her belly."),
                seg(4, "Let's recheck her on Sunday."),
            ],
        }
    }

    #[test]
    fn supported_and_paraphrased_statements_pass() {
        let a = Aligner::new(&transcript());
        assert!(a.check("/s/0", "Vomiting since Monday, at least once daily", &[0]).is_none());
        assert!(a.check("/s/1", "Mucous membranes pink and tacky; skin tent slow", &[1]).is_none());
        assert!(a.check("/m/0", "Lactated Ringer's SC", &[2]).is_none());
        assert!(a.check("/s/2", "No abdominal mass palpated", &[3]).is_none(), "negation and synonyms");
        assert!(a.check("/s/3", "Patient noted to recheck Sunday", &[4]).is_none(), "filler words ignored");
    }

    #[test]
    fn invented_claims_are_flagged() {
        let a = Aligner::new(&transcript());
        let f = a.check("/s/0", "Heart murmur grade 3 detected on auscultation", &[1]).unwrap();
        assert!(f.missing.contains(&"murmur".to_string()), "{f:?}");
        assert!(f.suggested.is_empty(), "nobody said it anywhere");
        // A flipped negation is never a paraphrase.
        let neg = a.check("/s/1", "Gums not pink", &[1]).unwrap();
        assert_eq!(neg.missing, vec!["no"]);
    }

    #[test]
    fn wrong_citations_get_a_suggestion() {
        let a = Aligner::new(&transcript());
        let f = a.check("/s/0", "Gums pink but tacky, skin tent slow", &[4]).unwrap();
        assert_eq!(f.suggested, vec![1]);
    }

    #[test]
    fn hints_and_second_opinions_count_as_said() {
        let mut t = transcript();
        t.segments[2].text = "We gave marapitin under the skin.".into();
        t.segments[2].hints = vec![crate::vocab::Hint {
            heard: "marapitin".into(),
            likely: vec!["maropitant".into()],
            kind: "drug".into(),
        }];
        t.segments[4].check = Some(crate::crosscheck::SecondOpinion {
            text: "The otitis has resolved.".into(),
            primary_only: vec![],
            second_only: vec![],
        });
        let a = Aligner::new(&t);
        assert!(a.check("/m/0", "Maropitant", &[2]).is_none());
        assert!(a.check("/a/0", "Otitis resolved", &[4]).is_none());
    }

    #[test]
    fn french_statements_fold_accents_elisions_and_negations() {
        let seg = |id, text: &str| Segment {
            id,
            start: 0.0,
            end: 1.0,
            speaker: Some(0),
            text: text.into(),
            hints: Vec::new(),
            check: None,
        };
        let t = Transcript {
            segments: vec![
                seg(0, "L'œil gauche est rouge, et il y a un peu d'écoulement purulent."),
                seg(1, "Le test à la fluorescéine est négatif, il n'y a pas d'ulcère."),
                seg(2, "Elle ne vomit pas et elle mange bien."),
            ],
        };
        let a = Aligner::new(&t);
        assert!(a.check("/o/0", "Écoulement purulent de l'oeil gauche", &[0]).is_none(), "accents, elisions");
        assert!(a.check("/o/1", "Test à la fluorescéine négatif, pas d'ulcère", &[1]).is_none());
        assert!(a.check("/s/0", "Ne vomit pas, mange bien", &[2]).is_none());
        // A dropped negation is never a paraphrase, in French either.
        let f = a.check("/o/2", "Pas d'écoulement", &[0]).unwrap();
        assert_eq!(f.missing, vec!["no"]);
        assert_eq!(content_tokens("Aucune douleur, sans boiterie"), vec!["no", "douleur", "boiterie"]);
    }

    #[test]
    fn english_keeps_words_that_are_french_stopwords_elsewhere() {
        assert_eq!(
            content_tokens("Pour the drops in, plus the comment by the son, then the car"),
            vec!["pour", "drop", "plus", "comment", "son", "car"]
        );
        // Unchanged English: accents and elisions don't touch it.
        assert_eq!(content_tokens("The owner didn't see any blood"), vec!["no", "see", "blood"]);
    }

    #[test]
    fn tokens_and_stems() {
        assert_eq!(content_tokens("The patient was vomiting twice daily"), vec!["vomit", "twice", "daily"]);
        assert_eq!(content_tokens("Doesn't want to eat"), vec!["no", "want", "eat"]);
        assert_eq!(stem("pancreatitis"), "pancreatitis");
        assert_eq!(stem("allergies"), "allergy");
    }
}
