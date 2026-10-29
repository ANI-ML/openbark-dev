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
//! key. When the two best terms are close, both are offered. An everyday
//! word that merely looks like a term ("certainly", Cerenia; "rabbits",
//! rabies: `packages/vocab/everyday.tsv`) is never hinted by spelling alone.
//!
//! Some slips need the words around them (ADR-0031). Only where the talk is
//! about medication (a dose, a unit, "give", "twice a day", another drug's
//! name):
//! - known mishearings (`packages/vocab/mishearings.tsv`, and a clinic's own
//!   "heard as" words) are hinted: "apple" for Apoquel;
//! - a drug name heard as two words is joined: "cyto point" for Cytopoint;
//! - names from public drug registers (`packages/vocab/regulatory.tsv`, made
//!   by scripts/vocab-import.py) are matched, with a stricter bar.
//!
//! "She ate an apple" gets no hint.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../../../packages/vocab/veterinary.tsv");
/// Brand and ingredient names from public veterinary drug registers.
const REGULATORY: &str = include_str!("../../../packages/vocab/regulatory.tsv");
/// Known mishearings: heard<TAB>term.
const MISHEARINGS: &str = include_str!("../../../packages/vocab/mishearings.tsv");
/// Everyday words that look like a hand-written term: word<TAB>term.
const EVERYDAY: &str = include_str!("../../../packages/vocab/everyday.tsv");

/// The kinds a clinic can give its own terms.
pub const KINDS: &[&str] = &["drug", "vaccine", "product", "condition", "procedure"];

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

/// Words recognition writes for a term ("apple" for apoquel).
#[derive(Debug, Clone)]
struct Alias {
    heard: Vec<String>,
    term: String,
    kind: String,
}

#[derive(Debug, Clone)]
pub struct Lexicon {
    /// The hand-written terms and the clinic's own.
    terms: Vec<Term>,
    /// Register names (drug context only, stricter bar), indexed by the first
    /// letter of their sound key.
    regulatory: Vec<Term>,
    by_first: HashMap<char, Vec<usize>>,
    /// Every register name with any trailing "s" removed, for exact matches.
    regulatory_known: HashSet<String>,
    aliases: Vec<Alias>,
    everyday: HashSet<String>,
}

fn is_drug_kind(kind: &str) -> bool {
    matches!(kind, "drug" | "vaccine" | "product")
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
pub(crate) fn jaro_winkler(a: &str, b: &str) -> f64 {
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
    "lactate",
    "flexion",
    "clinic",
    "clinics",
    "romaine",
    "definitely",
];
/// If the runner-up is within this margin, offer both.
const AMBIGUITY: f64 = 0.03;

/// Register names are many (about two thousand) and unreviewed one by one,
/// so they need a closer match than the hand-written list, in spelling too.
/// scripts/vocab-import.py uses the same two numbers.
const REGULATORY_THRESHOLD: f64 = 0.90;
const REGULATORY_SPELLING: f64 = 0.80;
/// Two heard words joined into one drug name ("cyto point"): stricter still,
/// as two ordinary words are being read as one.
const JOINED_THRESHOLD: f64 = 0.92;
/// [`Lexicon::aligned_word`]: the least resemblance between the primary's
/// word and the term the second recogniser heard in its place.
const ALIGNED_FLOOR: f64 = 0.60;

/// How many words either side of a candidate count as its context.
const CONTEXT_WINDOW: usize = 8;

/// Words that put talk about medication within reach: doses and units,
/// prescribing, and kinds of medicine.
const CUE_WORDS: &[&str] = &[
    "mg",
    "mgs",
    "milligram",
    "milligrams",
    "mcg",
    "microgram",
    "micrograms",
    "ml",
    "mls",
    "mil",
    "mils",
    "milliliter",
    "milliliters",
    "millilitre",
    "millilitres",
    "cc",
    "ccs",
    "tablet",
    "tablets",
    "tab",
    "tabs",
    "pill",
    "pills",
    "capsule",
    "capsules",
    "chew",
    "chews",
    "chewable",
    "chewables",
    "dose",
    "doses",
    "dosing",
    "dosage",
    "injection",
    "injections",
    "injectable",
    "pipette",
    "pipettes",
    "sid",
    "bid",
    "tid",
    "qid",
    "eod",
    "prn",
    "daily",
    "monthly",
    "medication",
    "medications",
    "medicine",
    "meds",
    "prescription",
    "prescribe",
    "prescribed",
    "dispense",
    "dispensed",
    "refill",
    "refills",
    "administer",
    "administered",
    "give",
    "giving",
    "gave",
    "start",
    "starting",
    "started",
    "taper",
    "tapering",
    "antibiotic",
    "antibiotics",
    "nsaid",
    "steroid",
    "steroids",
    "dewormer",
    "wormer",
    "preventative",
    "preventive",
    "sedative",
    "painkiller",
];

/// Word pairs that do the same: frequencies, "put him on", "send home",
/// "for itch".
const CUE_PAIRS: &[(&str, &str)] = &[
    ("a", "day"),
    ("per", "day"),
    ("every", "day"),
    ("a", "month"),
    ("every", "month"),
    ("per", "kilo"),
    ("per", "kg"),
    ("per", "pound"),
    ("him", "on"),
    ("her", "on"),
    ("them", "on"),
    ("send", "home"),
    ("sending", "home"),
    ("home", "with"),
    ("for", "itch"),
    ("for", "itching"),
    ("for", "pain"),
    ("for", "nausea"),
    ("for", "vomiting"),
    ("for", "fleas"),
    ("for", "ticks"),
    ("for", "worms"),
    ("for", "anxiety"),
    ("for", "seizures"),
    ("for", "arthritis"),
    ("for", "allergies"),
    ("for", "infection"),
    ("for", "diarrhea"),
    ("for", "diarrhoea"),
    ("anti", "itch"),
    ("anti", "nausea"),
    ("anti", "inflammatory"),
];

/// Whether `text` has any of the cue words or pairs (lexicon-free), for
/// choosing segments worth a second listen.
pub fn has_drug_cue(text: &str) -> bool {
    let words: Vec<String> =
        text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    words.iter().any(|w| CUE_WORDS.contains(&w.as_str()))
        || words.windows(2).any(|p| CUE_PAIRS.contains(&(p[0].as_str(), p[1].as_str())))
}

/// Register names are on unless `OPENBARK_VOCAB_REGULATORY` is "0" or "off"
/// (for comparing with and without them).
fn regulatory_enabled() -> bool {
    !matches!(std::env::var("OPENBARK_VOCAB_REGULATORY").as_deref(), Ok("0" | "off"))
}

/// The clinic's vocabulary file: `OPENBARK_VOCAB`, else
/// `<local data>/OpenBark/vocab.tsv`.
pub fn user_file() -> Option<PathBuf> {
    std::env::var_os("OPENBARK_VOCAB")
        .map(PathBuf::from)
        .or_else(|| dirs::data_local_dir().map(|d| d.join("OpenBark").join("vocab.tsv")))
}

/// `heard<TAB>term` rows, for terms in `terms` (others are ignored).
fn parse_mishearings(tsv: &str, terms: &[Term]) -> Vec<Alias> {
    tsv.lines()
        .filter_map(row)
        .filter_map(|(heard, term, _)| {
            let t = terms.iter().find(|t| t.text == term)?;
            let heard = normalise_words(&heard).split(' ').map(String::from).collect();
            Some(Alias { heard, term, kind: t.kind.clone() })
        })
        .collect()
}

/// A clinic's own term, kept in its vocabulary file ([`user_file`]) and
/// edited in Settings, Words and drug names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClinicTerm {
    pub term: String,
    pub kind: String,
    /// How speech recognition writes it ("heard as"), one to three words each.
    #[serde(default)]
    pub heard: Vec<String>,
}

/// The clinic's terms in `path`; none when there is no file.
pub fn read_clinic_terms(path: &Path) -> Vec<ClinicTerm> {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines().filter_map(row).map(|(term, kind, heard)| ClinicTerm { term, kind, heard }).collect()
        })
        .unwrap_or_default()
}

/// A term as it will be saved: lowercase, single spaces, "heard as" words
/// deduplicated. The error says what to change.
pub fn check_clinic_term(t: &ClinicTerm) -> Result<ClinicTerm, String> {
    const CHARS: &str = "Use letters, numbers, hyphens and apostrophes, up to 40 characters.";
    let plain =
        |s: &str| s.chars().count() <= 40 && s.chars().all(|c| c.is_alphanumeric() || " -'".contains(c));
    let term = normalise_words(&t.term);
    if term.is_empty() {
        return Err("Type the word or name.".into());
    }
    if !plain(&term) {
        return Err(CHARS.into());
    }
    if term.split(' ').count() > 2 {
        return Err("A term is one or two words.".into());
    }
    if !KINDS.contains(&t.kind.as_str()) {
        return Err("Choose what kind of term it is.".into());
    }
    let mut heard: Vec<String> = Vec::new();
    for h in t.heard.iter().map(|h| normalise_words(h)).filter(|h| !h.is_empty() && *h != term) {
        if !plain(&h) {
            return Err(CHARS.into());
        }
        if h.split(' ').count() > 3 {
            return Err("\"Heard as\" is one to three words.".into());
        }
        if !heard.contains(&h) {
            heard.push(h);
        }
    }
    Ok(ClinicTerm { term, kind: t.kind.clone(), heard })
}

/// Adds `t` to the clinic's file at `path`, or replaces the same term,
/// keeping every other line (comments too) as it was.
pub fn save_clinic_term(path: &Path, t: &ClinicTerm) -> Result<Vec<ClinicTerm>, String> {
    let t = check_clinic_term(t)?;
    // Two columns unless there are "heard as" words, so older versions
    // still read the row.
    let line = if t.heard.is_empty() {
        format!("{}\t{}", t.term, t.kind)
    } else {
        format!("{}\t{}\tclinic\t{}", t.term, t.kind, t.heard.join(";"))
    };
    rewrite_clinic_file(path, |lines| {
        match lines.iter().position(|l| row(l).is_some_and(|(term, _, _)| term == t.term)) {
            Some(i) => lines[i] = line,
            None => lines.push(line),
        }
    })
}

/// Removes `term` from the clinic's file at `path`.
pub fn remove_clinic_term(path: &Path, term: &str) -> Result<Vec<ClinicTerm>, String> {
    let term = normalise_words(term);
    rewrite_clinic_file(path, |lines| lines.retain(|l| row(l).is_none_or(|(t, _, _)| t != term)))
}

fn rewrite_clinic_file(
    path: &Path,
    change: impl FnOnce(&mut Vec<String>),
) -> Result<Vec<ClinicTerm>, String> {
    let failed = |e: std::io::Error| format!("Couldn't save {}: {e}", path.display());
    let mut lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(text) => text.lines().map(String::from).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![
            "# OpenBark clinic vocabulary: term<TAB>kind, then optionally <TAB>clinic<TAB>heard as;heard as."
                .into(),
            "# Edited in Settings, Words and drug names. Used for hints only; transcripts are never changed."
                .into(),
        ],
        Err(e) => return Err(failed(e)),
    };
    change(&mut lines);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let tmp = path.with_extension("tsv.tmp");
    std::fs::write(&tmp, lines.join("\n") + "\n").map_err(failed)?;
    std::fs::rename(&tmp, path).map_err(failed)?;
    Ok(read_clinic_terms(path))
}

/// One row of a vocabulary file: `term<TAB>kind[<TAB>sources[<TAB>heard as]]`.
/// Columns after the kind are optional, so a two-column file still reads,
/// and the kind is never polluted by them. "Heard as" holds words separated
/// by ";".
fn row(line: &str) -> Option<(String, String, Vec<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut cols = line.split('\t').map(str::trim);
    let text = cols.next()?.to_lowercase();
    let kind = cols.next().filter(|k| !k.is_empty()).unwrap_or("term").to_string();
    let _sources = cols.next();
    let heard = cols
        .next()
        .map(|h| h.split(';').map(normalise_words).filter(|w| !w.is_empty()).collect())
        .unwrap_or_default();
    (!text.is_empty()).then_some((text, kind, heard))
}

/// Lowercase words separated by single spaces.
fn normalise_words(s: &str) -> String {
    s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" ")
}

fn term(text: String, kind: String) -> Term {
    Term { key: sound_key(&text), words: text.split_whitespace().count(), kind, text }
}

impl Lexicon {
    /// Terms (and their "heard as" words) from a vocabulary file.
    pub fn parse(tsv: &str) -> Self {
        let mut terms = Vec::new();
        let mut aliases = Vec::new();
        for (text, kind, heard) in tsv.lines().filter_map(row) {
            for h in heard {
                aliases.push(Alias {
                    heard: h.split(' ').map(String::from).collect(),
                    term: text.clone(),
                    kind: kind.clone(),
                });
            }
            terms.push(term(text, kind));
        }
        Self {
            terms,
            regulatory: Vec::new(),
            by_first: HashMap::new(),
            regulatory_known: HashSet::new(),
            aliases,
            everyday: HashSet::new(),
        }
    }

    /// The built-in lexicon plus the user's additions, if any.
    pub fn load(user_file: Option<&Path>) -> Self {
        let mut lex = Self::parse(BUILTIN);
        lex.aliases.extend(parse_mishearings(MISHEARINGS, &lex.terms));
        lex.everyday = EVERYDAY.lines().filter_map(row).map(|(w, _, _)| w).collect();
        if regulatory_enabled() {
            lex.set_regulatory(REGULATORY);
        }
        if let Some(extra) = user_file.and_then(|p| std::fs::read_to_string(p).ok()) {
            let user = Self::parse(&extra);
            lex.terms.extend(user.terms);
            lex.aliases.extend(user.aliases);
        }
        lex
    }

    /// Built-in terms plus `<local data>/OpenBark/vocab.tsv`, if present.
    pub fn default_with_user() -> Self {
        Self::load(user_file().as_deref())
    }

    /// Register names, except those already hand-written.
    fn set_regulatory(&mut self, tsv: &str) {
        let hand: HashSet<&str> = self.terms.iter().map(|t| t.text.as_str()).collect();
        self.regulatory = tsv
            .lines()
            .filter_map(row)
            .filter(|(text, _, _)| !hand.contains(text.as_str()))
            .map(|(text, kind, _)| term(text, kind))
            .collect();
        self.by_first.clear();
        for (i, t) in self.regulatory.iter().enumerate() {
            if let Some(c) = t.key.chars().next() {
                self.by_first.entry(c).or_default().push(i);
            }
        }
        self.regulatory_known =
            self.regulatory.iter().map(|t| t.text.trim_end_matches('s').to_string()).collect();
    }

    /// How many terms each list holds: hand-written (with the clinic's), register.
    pub fn counts(&self) -> (usize, usize) {
        (self.terms.len(), self.regulatory.len())
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
            .chain(&self.regulatory)
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

    /// The words in `primary` standing where `second` has `term`, found by
    /// a neighbouring word the two readings share ("start ___ sixteen"):
    /// for a term the second recogniser heard that sounds nothing like what
    /// the primary wrote ("apple" for Apoquel), where [`Self::closest_word`]
    /// finds nothing. One or two words, not themselves a known term.
    pub fn aligned_word(&self, primary: &str, second: &str, term: &str) -> Option<String> {
        let split = |s: &str| -> Vec<String> {
            s.split(|c: char| !c.is_alphanumeric() && c != '\'')
                .filter(|w| !w.is_empty())
                .map(String::from)
                .collect()
        };
        let (p, s) = (split(primary), split(second));
        let pl: Vec<String> = p.iter().map(|w| w.to_lowercase()).collect();
        let sl: Vec<String> = s.iter().map(|w| w.to_lowercase()).collect();
        let tw: Vec<&str> = term.split(' ').collect();
        let k = (0..sl.len()).find(|&k| sl.len() >= k + tw.len() && sl[k..k + tw.len()] == tw[..])?;
        let before = k.checked_sub(1).map(|b| sl[b].as_str());
        let after = sl.get(k + tw.len()).map(String::as_str);
        let mut best: Option<(f64, String)> = None;
        for j in 0..pl.len() {
            for n in [1usize, 2] {
                if j + n > pl.len() {
                    continue;
                }
                let left = j.checked_sub(1).map(|b| pl[b].as_str());
                let right = pl.get(j + n).map(String::as_str);
                let anchored = (before.is_some() && left == before) || (after.is_some() && right == after);
                let heard = p[j..j + n].join(" ");
                let lower = pl[j..j + n].join(" ");
                let letters = lower.chars().filter(|c| c.is_alphabetic()).count();
                if !anchored
                    || letters < 3
                    || self.is_term(&lower)
                    || pl[j..j + n].iter().any(|w| COMMON.contains(&w.as_str()) || w == "a" || w == "the")
                {
                    continue;
                }
                let score = jaro_winkler(&lower.replace(' ', ""), &term.replace(' ', ""));
                if score >= ALIGNED_FLOOR && best.as_ref().is_none_or(|(b, _)| score > *b) {
                    best = Some((score, heard));
                }
            }
        }
        best.map(|(_, h)| h)
    }

    fn is_term(&self, phrase: &str) -> bool {
        let p = phrase.to_lowercase();
        self.terms.iter().any(|t| t.text == p || t.text.trim_end_matches('s') == p.trim_end_matches('s'))
            || self.regulatory_known.contains(p.trim_end_matches('s'))
    }

    /// A drug, vaccine or product name, as said.
    fn is_drug_name(&self, phrase: &str) -> bool {
        self.terms.iter().chain(&self.regulatory).any(|t| t.text == phrase && is_drug_kind(&t.kind))
    }

    /// Whether the words around `words[from..to]` are about medication: a
    /// dose or unit, a word like "give" or "prescribe", a phrase like "twice
    /// a day" or "for itch", or another drug's name, within
    /// [`CONTEXT_WINDOW`] words either side. `words` are lowercase.
    fn drug_context(&self, words: &[String], from: usize, to: usize) -> bool {
        let lo = from.saturating_sub(CONTEXT_WINDOW);
        let hi = (to + CONTEXT_WINDOW).min(words.len());
        let outside = |i: usize| i < from || i >= to;
        (lo..hi).filter(|&i| outside(i)).any(|i| {
            let w = words[i].as_str();
            CUE_WORDS.contains(&w)
                || self.is_drug_name(w)
                || (i + 1 < hi
                    && outside(i + 1)
                    && (CUE_PAIRS.contains(&(w, words[i + 1].as_str()))
                        || self.is_drug_name(&format!("{w} {}", words[i + 1]))))
        })
    }

    /// The clinic's or the built-in mishearing starting at `words[i]`, longest first.
    fn alias_at(&self, words: &[String], i: usize) -> Option<&Alias> {
        self.aliases
            .iter()
            .filter(|a| words.len() >= i + a.heard.len() && words[i..i + a.heard.len()] == a.heard[..])
            .max_by_key(|a| a.heard.len())
    }

    /// The best register name for one word, at the register bar.
    fn regulatory_match(&self, word: &str) -> Option<(f64, &Term)> {
        let key = sound_key(word);
        let first = key.chars().next()?;
        let len = word.chars().count();
        self.by_first
            .get(&first)?
            .iter()
            .map(|&i| &self.regulatory[i])
            .filter(|t| t.words == 1 && t.text.len().abs_diff(len) <= (t.text.len() / 3).max(2))
            .filter_map(|t| {
                let spelling = jaro_winkler(word, &t.text);
                let score = (spelling + jaro_winkler(&key, &t.key)) / 2.0;
                (spelling >= REGULATORY_SPELLING && score >= REGULATORY_THRESHOLD).then_some((score, t))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// The best one-word drug name (hand-written or register) for two heard
    /// words written as one: "cyto point" for cytopoint.
    fn joined_match(&self, joined: &str) -> Option<(f64, &Term)> {
        let key = sound_key(joined);
        let hand = self.terms.iter().filter(|t| t.words == 1 && is_drug_kind(&t.kind));
        let register = key
            .chars()
            .next()
            .and_then(|c| self.by_first.get(&c))
            .into_iter()
            .flatten()
            .map(|&i| &self.regulatory[i])
            .filter(|t| t.words == 1);
        hand.chain(register)
            .filter_map(|t| {
                let spelling = jaro_winkler(joined, &t.text);
                let score = (spelling + jaro_winkler(&key, &t.key)) / 2.0;
                (spelling >= REGULATORY_SPELLING && score >= JOINED_THRESHOLD).then_some((score, t))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// Whether one heard word would get a hand-written term's hint on its own.
    fn single_match(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        if lower.chars().filter(|c| c.is_alphabetic()).count() < 5
            || self.is_term(&lower)
            || self.everyday.contains(&lower)
        {
            return false;
        }
        let key = sound_key(&lower);
        self.terms.iter().filter(|t| t.words == 1).any(|t| {
            let spelling = jaro_winkler(&lower, &t.text);
            let bar =
                if matches!(t.kind.as_str(), "drug" | "vaccine") { THRESHOLD_DRUG } else { THRESHOLD_OTHER };
            spelling >= SPELLING_FLOOR && (spelling + jaro_winkler(&key, &t.key)) / 2.0 >= bar
        })
    }

    /// Hints for one line of transcript.
    pub fn hints(&self, text: &str) -> Vec<Hint> {
        let mut hints = spoken_form_hints(text);
        // "sepromazine.02": the word and the number are separate.
        let text = &split_glued_decimals(text);
        hints.extend(self.term_hints(text));
        hints
    }

    fn term_hints(&self, text: &str) -> Vec<Hint> {
        let words: Vec<&str> = text
            .split(|c: char| c.is_whitespace() || c == '-')
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
            .filter(|w| !w.is_empty())
            .collect();
        let lowered: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let mut hints: Vec<Hint> = Vec::new();
        let mut i = 0;
        while i < words.len() {
            // A known mishearing; a drug's only where the talk is about medication.
            if let Some(alias) = self.alias_at(&lowered, i) {
                let n = alias.heard.len();
                if !is_drug_kind(&alias.kind) || self.drug_context(&lowered, i, i + n) {
                    hints.push(Hint {
                        heard: words[i..i + n].join(" "),
                        likely: vec![alias.term.clone()],
                        kind: alias.kind.clone(),
                    });
                    i += n;
                    continue;
                }
            }
            // A drug's first syllable heard as "a" ("a sepromazine" for acepromazine).
            if words[i].eq_ignore_ascii_case("a")
                && let Some(next) = words.get(i + 1)
                && !self.is_term(next)
            {
                let joined = format!("a{}", next.to_lowercase());
                let key = sound_key(&joined);
                let best = self
                    .terms
                    .iter()
                    .filter(|t| t.words == 1 && matches!(t.kind.as_str(), "drug" | "vaccine"))
                    .map(|t| ((jaro_winkler(&joined, &t.text) + jaro_winkler(&key, &t.key)) / 2.0, t))
                    .filter(|(s, _)| *s >= THRESHOLD_DRUG + 0.05)
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((_, term)) = best {
                    hints.push(Hint {
                        heard: format!("{} {next}", words[i]),
                        likely: vec![term.text.clone()],
                        kind: term.kind.clone(),
                    });
                    i += 2;
                    continue;
                }
            }
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
                    // Every word must resemble its counterpart: "canine looks" isn't canine influenza.
                    .filter(|t| {
                        n == 1
                            || lower.split(' ').zip(t.text.split(' ')).all(|(a, b)| jaro_winkler(a, b) >= 0.7)
                    })
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
                // An everyday word that only looks like a term ("certainly", Cerenia).
                if n == 1 && self.everyday.contains(&lower) {
                    continue;
                }
                if let Some((top, term)) = scored.first() {
                    // A register name that fits better, in talk about medication
                    // ("forthiron": Forthyron, not Fortekor).
                    if n == 1
                        && let Some((s, t)) = self.regulatory_match(&lower)
                        && s > *top
                        && self.drug_context(&lowered, i, i + 1)
                    {
                        let hint = Hint {
                            heard: phrase.clone(),
                            likely: vec![t.text.clone()],
                            kind: t.kind.clone(),
                        };
                        best_here = Some((hint, 1));
                        break;
                    }
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
            if best_here.is_none() {
                best_here = self.context_hint(&words, &lowered, i);
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

    /// Hints that need drug context, at `words[i]` when nothing else matched
    /// there: a register name, else a drug name heard as two words.
    fn context_hint(&self, words: &[&str], lower: &[String], i: usize) -> Option<(Hint, usize)> {
        let hintable = |w: &str| !COMMON.contains(&w) && !self.is_term(w) && !CUE_WORDS.contains(&w);
        let w = lower.get(i)?.as_str();
        if w.chars().filter(|c| c.is_alphabetic()).count() >= 5
            && hintable(w)
            && !self.everyday.contains(w)
            && let Some((_, t)) = self.regulatory_match(w)
            && self.drug_context(lower, i, i + 1)
        {
            return Some((
                Hint { heard: words[i].to_string(), likely: vec![t.text.clone()], kind: t.kind.clone() },
                1,
            ));
        }
        // "cyto point": two plain words of three letters or more, the second
        // no match of its own, for a name of seven letters or more (shorter
        // ones are too easily made from two words: "call you").
        let b = lower.get(i + 1)?.as_str();
        let joined = format!("{w}{b}");
        let plain = |x: &str| x.len() >= 3 && hintable(x) && !JOIN_STOP.contains(&x);
        if !(joined.chars().all(|c| c.is_ascii_alphabetic()) && plain(w) && plain(b)) || self.single_match(b)
        {
            return None;
        }
        let (_, t) = self.joined_match(&joined).filter(|(_, t)| t.text.len() >= 7)?;
        self.drug_context(lower, i, i + 2).then(|| {
            (Hint { heard: words[i..i + 2].join(" "), likely: vec![t.text.clone()], kind: t.kind.clone() }, 2)
        })
    }
}

/// Words never read as half of a drug name.
const JOIN_STOP: &[&str] = &[
    "you", "the", "and", "for", "with", "her", "him", "his", "she", "hers", "they", "them", "our", "your",
    "its", "but", "not", "are", "was", "has", "had", "have", "this", "that", "then", "than", "there", "here",
    "what", "when", "who", "how", "all", "any", "can", "will", "would", "could", "should", "one", "two",
    "six", "ten",
];

/// Words that sound like one of the animals' names ("Arlive" for Olive), so a
/// multi-pet note can tell whose findings a line holds. Only capitalised
/// words count (recognisers capitalise names), so "alive" isn't Olive.
pub fn name_hints(text: &str, names: &[String]) -> Vec<Hint> {
    let names: Vec<(String, String)> = names
        .iter()
        .map(|n| n.trim())
        .filter(|n| n.chars().count() >= 3)
        .map(|n| (n.to_string(), n.to_lowercase()))
        .collect();
    let mut hints: Vec<Hint> = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '\'') {
        let word = word.trim_matches('\'').trim_end_matches("'s");
        if word.chars().count() < 3 || !word.starts_with(|c: char| c.is_uppercase()) {
            continue;
        }
        let lower = word.to_lowercase();
        if names.iter().any(|(_, n)| *n == lower) || hints.iter().any(|h| h.heard == word) {
            continue;
        }
        let key = sound_key(&lower);
        let best = names
            .iter()
            .map(|(name, n)| (jaro_winkler(&lower, n).max(jaro_winkler(&key, &sound_key(n))), name))
            .filter(|(s, _)| *s >= NAME_THRESHOLD)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, name)) = best {
            hints.push(Hint { heard: word.to_string(), likely: vec![name.clone()], kind: "name".into() });
        }
    }
    hints
}

/// How close a capitalised word must sound to an animal's name.
const NAME_THRESHOLD: f64 = 0.80;

/// "hydromorphone.1" → "hydromorphone .1".
fn split_glued_decimals(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 4);
    for (i, c) in chars.iter().enumerate() {
        if *c == '.'
            && i > 0
            && chars[i - 1].is_alphabetic()
            && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())
        {
            out.push(' ');
        }
        out.push(*c);
    }
    out
}

/// Recognition slips that aren't misspelt terms: a dose written as ".1"
/// (said "point one"), and routes heard as other words. Each is a visible
/// hint like any other; the transcript keeps what was heard.
fn spoken_form_hints(text: &str) -> Vec<Hint> {
    let mut hints = Vec::new();
    // ".1" with no leading digit, alone or glued to a word.
    for token in text.split_whitespace() {
        let t = token.trim_end_matches([',', ';', ':']);
        let Some(dot) = t.find('.') else { continue };
        let (before, after) = (&t[..dot], &t[dot + 1..]);
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || before.chars().last().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let rest = &after[digits.len()..];
        let likely = if before.is_empty() {
            format!("0.{digits}{rest}")
        } else {
            format!("{before} 0.{digits}{rest}")
        };
        hints.push(Hint { heard: t.to_string(), likely: vec![likely], kind: "number".into() });
    }
    // "IV" heard as the Roman numeral four.
    let lower = text.to_lowercase();
    for heard in ["roman 4", "roman four", "roman iv"] {
        if let Some(at) = lower.find(heard) {
            let original = &text[at..at + heard.len()];
            hints.push(Hint { heard: original.to_string(), likely: vec!["IV".into()], kind: "route".into() });
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dose_written_without_a_leading_zero_is_hinted() {
        let h = lex().hints("Premed is hydromorphone.1 mg per kilo with a sepromazine.02 mg per kilo IM.");
        let heard: Vec<(&str, &str)> = h.iter().map(|h| (h.heard.as_str(), h.likely[0].as_str())).collect();
        assert!(heard.contains(&("hydromorphone.1", "hydromorphone 0.1")), "{heard:?}");
        assert!(heard.contains(&("sepromazine.02", "sepromazine 0.02")), "{heard:?}");
        // The glued word still gets its drug hint.
        assert!(heard.iter().any(|(_, l)| *l == "acepromazine"), "{heard:?}");
        // Ordinary decimals are left alone.
        assert!(lex().hints("Weight 24.5 kilos, temperature 38.6.").iter().all(|h| h.kind != "number"));
    }

    #[test]
    fn two_word_hints_need_both_words_to_match() {
        assert!(lex().hints("four o four The lower right canine looks fine.").is_empty());
    }

    #[test]
    fn misheard_pet_names_are_hinted() {
        let names = ["Olive".to_string(), "Pickles".to_string()];
        let h = name_hints("Let's start with Arlive since the easy one, and then pickles.", &names);
        assert_eq!(h.len(), 1, "{h:?}");
        assert_eq!((h[0].heard.as_str(), h[0].likely[0].as_str()), ("Arlive", "Olive"));
        // Ordinary words and other names stay alone.
        assert!(
            name_hints("She's alive and well. Dr. Akafer will see you. Thank you, Jordan.", &names)
                .is_empty()
        );
        assert!(name_hints("Olive is three point nine kilos", &names).is_empty());
    }

    #[test]
    fn iv_heard_as_roman_four_is_hinted() {
        let h = lex().hints("Pentabarbital six Mills Roman four through the catheter.");
        assert!(h.iter().any(|h| h.heard == "Roman four" && h.likely == ["IV"]), "{h:?}");
    }

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

    fn heard(h: &[Hint]) -> Vec<(&str, &str)> {
        h.iter().map(|h| (h.heard.as_str(), h.likely[0].as_str())).collect()
    }

    #[test]
    fn apple_is_apoquel_only_when_the_talk_is_about_medication() {
        let l = lex();
        for said in [
            "Let's start her on apple, sixteen milligrams twice a day.",
            "We'll put him on apple for the itch.",
            "Give the apple with food.",
        ] {
            assert_eq!(heard(&l.hints(said)), [("apple", "apoquel")], "{said}");
        }
        for said in ["She ate an apple this morning.", "An apple a week is fine as a treat."] {
            assert!(l.hints(said).is_empty(), "{said}: {:?}", l.hints(said));
        }
    }

    #[test]
    fn a_drug_name_heard_as_two_words_is_joined_in_drug_context() {
        let l = lex();
        assert_eq!(heard(&l.hints("We can give cyto point for the itch.")), [("cyto point", "cytopoint")]);
        assert_eq!(heard(&l.hints("Start meta cam at the usual dose.")), [("meta cam", "metacam")]);
        assert!(l.hints("The meta data and the cyto point were reviewed.").is_empty());
    }

    #[test]
    fn everyday_look_alikes_are_never_hinted_by_spelling() {
        let l = lex();
        // "certainly" resembles Cerenia, "rabbits" rabies: neither is hinted,
        // even in talk about medication.
        assert!(l.hints("She's certainly better, so give the rest of the tablets.").is_empty());
        assert!(l.hints("Both rabbits get the injection today.").is_empty());
    }

    #[test]
    fn register_names_need_drug_context() {
        let l = lex();
        let (_, register) = l.counts();
        assert!(register > 1000, "{register}");
        // Incurin (estriol) is a register name, not hand-written.
        assert!(l.regulatory.iter().any(|t| t.text == "incurin"));
        assert_eq!(heard(&l.hints("Send home incurine, one tablet once a day.")), [("incurine", "incurin")]);
        assert!(l.hints("The incurine report came back.").is_empty());
        // A register name heard correctly is no hint.
        assert!(l.hints("Send home incurin, one tablet once a day.").is_empty());
        // One that fits better than a hand-written term, in talk about
        // medication: Forthyron (levothyroxine), not Fortekor.
        assert_eq!(heard(&l.hints("Start forthiron twice a day.")), [("forthiron", "forthyron")]);
    }

    #[test]
    fn extra_columns_never_change_the_kind() {
        // Sources and "heard as" columns: "zorbiflex" stays a drug.
        let l = Lexicon::parse("zorbiflex\tdrug\tclinic\tsorry flex;zorby flecks\n");
        assert_eq!(l.terms[0].kind, "drug");
        assert_eq!(heard(&l.hints("we gave zorbyflex today")), [("zorbyflex", "zorbiflex")]);
        assert_eq!(heard(&l.hints("give sorry flex twice a day")), [("sorry flex", "zorbiflex")]);
        assert!(l.hints("sorry flex, I was late").is_empty());
    }

    #[test]
    fn every_mishearing_points_at_a_hand_written_term() {
        let l = lex();
        let terms: Vec<&str> = l.terms.iter().map(|t| t.text.as_str()).collect();
        for (heard, term, _) in MISHEARINGS.lines().filter_map(row) {
            assert!(terms.contains(&term.as_str()), "{heard} -> {term}: not in veterinary.tsv");
            assert!(!l.is_term(&heard), "{heard} is itself a term");
            assert!(heard.split(' ').count() <= 3, "{heard}");
        }
        for line in REGULATORY.lines().filter(|l| !l.starts_with('#')) {
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 3, "{line}");
            assert!(cols[0].split(' ').count() <= 2 && KINDS.contains(&cols[1]), "{line}");
        }
    }

    #[test]
    fn clinic_terms_are_saved_beside_other_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vocab.tsv");
        std::fs::write(&path, "# our list\nzorbiflex\tdrug\n").unwrap();
        let t = |term: &str, kind: &str, heard: &[&str]| ClinicTerm {
            term: term.into(),
            kind: kind.into(),
            heard: heard.iter().map(|h| h.to_string()).collect(),
        };
        let terms =
            save_clinic_term(&path, &t("  Nobivac  Lepto ", "vaccine", &["nobi vac lepto", ""])).unwrap();
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[1], t("nobivac lepto", "vaccine", &["nobi vac lepto"]));
        // Replacing keeps one row; no "heard as" words writes two columns.
        save_clinic_term(&path, &t("Zorbiflex", "product", &[])).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with(
                "# our list\nzorbiflex\tproduct\nnobivac lepto\tvaccine\tclinic\tnobi vac lepto\n"
            ),
            "{text}"
        );
        assert_eq!(remove_clinic_term(&path, "ZORBIFLEX").unwrap().len(), 1);
        // The engine reads what was saved.
        let lex = Lexicon::load(Some(path.as_path()));
        assert!(lex.terms.iter().any(|t| t.text == "nobivac lepto" && t.kind == "vaccine"));
        // Refused: three words, a separator, an unknown kind. Tabs and line
        // breaks are only spaces.
        assert!(check_clinic_term(&t("one two three", "drug", &[])).is_err());
        assert!(check_clinic_term(&t("zorbi;flex", "drug", &[])).is_err());
        assert_eq!(check_clinic_term(&t("zorbi\tflex\n", "drug", &[])).unwrap().term, "zorbi flex");
        assert!(check_clinic_term(&t("zorbiflex", "food", &[])).is_err());
        assert!(check_clinic_term(&t("zorbiflex", "drug", &["a b c d"])).is_err());
    }

    /// packages/eval/fixtures/vocab/hints.tsv: what recognisers write for drug
    /// names they don't know, and ordinary sentences (docs/eval/vocabulary.md).
    #[test]
    fn labelled_hint_set() {
        let l = lex();
        let set = include_str!("../../../packages/eval/fixtures/vocab/hints.tsv");
        let (mut found, mut drugs, mut false_hints, mut quiet, mut plain) = (0, 0, Vec::new(), 0, 0);
        for line in set.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let (text, want) = line.split_once('\t').expect("text<TAB>drug");
            let hints = l.hints(text);
            if want == "-" {
                plain += 1;
                quiet += usize::from(hints.is_empty());
            } else {
                drugs += 1;
                found += usize::from(hints.iter().any(|h| h.likely.iter().any(|x| x == want)));
            }
            false_hints.extend(
                hints
                    .iter()
                    .filter(|h| !h.likely.iter().any(|x| x == want))
                    .map(|h| format!("{text}: {h:?}")),
            );
        }
        let summary = format!("recall {found}/{drugs}, quiet {quiet}/{plain}, false {false_hints:#?}");
        assert!(found * 100 >= drugs * 90, "{summary}");
        assert!(false_hints.len() <= 1 && quiet + 1 >= plain, "{summary}");
    }

    #[test]
    fn drug_cues_are_found_without_a_lexicon() {
        assert!(has_drug_cue("We'll send home some pills"));
        assert!(has_drug_cue("twice a day with food"));
        assert!(!has_drug_cue("She ate an apple this morning"));
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
