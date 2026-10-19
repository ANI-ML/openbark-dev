//! Warnings while typing (ADR-0024 section 4.4): instructions, rules and
//! heading instructions that ask for something the rules that always apply
//! forbid. They warn and never block: the rules come first in every prompt,
//! and the checks after drafting are the real guard.

use serde::Serialize;

use crate::template::Template;

/// Why a phrase conflicts with the rules that always apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LintReason {
    /// Findings that weren't said ("assume normal").
    UnsaidFindings,
    /// Numbers that weren't said ("calculate the dose").
    UnsaidNumbers,
    /// Doses that weren't said ("usual dose").
    UnsaidDoses,
    /// Takes away "Not discussed" ("never leave empty").
    RemovesNotDiscussed,
    /// Inventing ("make up", "guess").
    Inventing,
    /// Drops the evidence links ("skip citations").
    Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintWarning {
    /// Where: "instructions", "rules/2", or "headings/plan/medications".
    pub path: String,
    /// The words that conflict, as written.
    pub text: String,
    pub reason: LintReason,
}

/// Words that, just before a trigger, turn it around ("never guess").
const NEGATIONS: &[&str] =
    &["never", "not", "don't", "dont", "do not", "no", "without", "avoid", "rather than"];

/// Conditions that, with "normal", mean filling in findings that weren't said.
const UNSAID: &[&str] = &[
    "assume",
    "default",
    "not mentioned",
    "not said",
    "weren't said",
    "wasn't said",
    "were not said",
    "was not said",
    "not stated",
    "not discussed",
    "if missing",
    "otherwise",
];

const NUMBER_WORDS: &[&str] = &[
    "dose",
    "doses",
    "dosage",
    "bsa",
    "body surface",
    "price",
    "prices",
    "cost",
    "costs",
    "total",
    "withdrawal",
    "rate",
    "volume",
];

fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['.', '!', '?', ';', '\n']).map(str::trim).filter(|s| !s.is_empty())
}

/// Lowercase words with surrounding punctuation and quotes removed.
fn words(sentence: &str) -> Vec<String> {
    sentence
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '/')
                .trim_matches('\'')
                .to_lowercase()
                .replace('\u{2019}', "'")
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Is the phrase starting at word `at` negated by one of the two words before it?
fn negated(ws: &[String], at: usize) -> bool {
    let before = &ws[at.saturating_sub(2)..at];
    let joined = before.join(" ");
    NEGATIONS.iter().any(|n| before.iter().any(|w| w == n) || joined.ends_with(n))
}

/// Where a phrase (one or more words) starts in `ws`, not negated.
fn find(ws: &[String], phrase: &str) -> Option<usize> {
    let p: Vec<&str> = phrase.split(' ').collect();
    (0..ws.len().saturating_sub(p.len() - 1))
        .find(|&i| p.iter().enumerate().all(|(j, w)| ws[i + j] == *w) && !negated(ws, i))
}

fn contains(ws: &[String], phrase: &str) -> bool {
    let p: Vec<&str> = phrase.split(' ').collect();
    (0..ws.len().saturating_sub(p.len() - 1)).any(|i| p.iter().enumerate().all(|(j, w)| ws[i + j] == *w))
}

/// The conflicts in one piece of text.
fn check(text: &str) -> Vec<(String, LintReason)> {
    let mut out = Vec::new();
    for sentence in sentences(text) {
        let ws = words(sentence);
        let mut hit = |phrase: &str, reason: LintReason| {
            if !out.iter().any(|(_, r)| *r == reason) {
                out.push((phrase.to_string(), reason));
            }
        };
        // "normal" values with a condition about what wasn't said.
        let normal = ["within normal limits", "normal", "wnl", "normal values"]
            .iter()
            .find(|p| find(&ws, p).is_some());
        if let Some(p) = normal
            && UNSAID.iter().any(|u| contains(&ws, u))
        {
            hit(p, LintReason::UnsaidFindings);
        }
        // Working out a number near a dose, area, price or withdrawal.
        for verb in ["calculate", "work out", "estimate", "compute", "figure out", "add up", "derive"] {
            if let Some(i) = find(&ws, verb) {
                let near = ws[i..ws.len().min(i + 8)].join(" ");
                if NUMBER_WORDS.iter().any(|w| near.split(' ').any(|x| x == *w) || near.contains(w)) {
                    hit(verb, LintReason::UnsaidNumbers);
                }
            }
        }
        // Doses that come from habit, not the recording.
        for p in [
            "standard dose",
            "usual dose",
            "typical dose",
            "recommended dose",
            "typical value",
            "typical values",
        ] {
            if find(&ws, p).is_some() {
                hit(p, LintReason::UnsaidDoses);
            }
        }
        // Filling every heading.
        for p in ["fill in", "complete every", "never leave", "always fill", "fill every", "every field must"]
        {
            // "never leave" is itself the trigger, so check it without the negation rule.
            let found = if p == "never leave" {
                contains(&ws, "never leave") && (contains(&ws, "empty") || contains(&ws, "blank"))
            } else {
                find(&ws, p).is_some()
            };
            if found {
                hit(p, LintReason::RemovesNotDiscussed);
            }
        }
        // Inventing.
        for p in ["guess", "infer the species", "make up", "invent", "fabricate", "assume the species"] {
            if find(&ws, p).is_some() {
                hit(p, LintReason::Inventing);
            }
        }
        // Evidence links.
        for p in [
            "no evidence needed",
            "skip citations",
            "skip the citations",
            "without evidence",
            "no citations",
            "skip evidence",
            "ignore evidence",
        ] {
            if contains(&ws, p) {
                hit(p, LintReason::Evidence);
            }
        }
    }
    out
}

/// Every warning for a template: its instructions, its own rules, and each
/// heading's instruction. Fixed rules aren't checked (they only restrict).
pub fn lint_template(t: &Template) -> Vec<LintWarning> {
    let mut out = Vec::new();
    let mut add = |path: String, text: &str| {
        for (phrase, reason) in check(text) {
            out.push(LintWarning { path: path.clone(), text: phrase, reason });
        }
    };
    add("instructions".into(), &t.instructions);
    for (i, rule) in t.rules.iter().enumerate() {
        add(format!("rules/{i}"), rule);
    }
    for s in &t.sections {
        if let Some(i) = &s.instruction {
            add(format!("headings/{}", s.id), i);
        }
        for p in &s.parts {
            if let Some(i) = &p.instruction {
                add(format!("headings/{}/{}", s.id, p.key), i);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::LoadedTemplate;

    fn reasons(text: &str) -> Vec<LintReason> {
        check(text).into_iter().map(|(_, r)| r).collect()
    }

    /// One warning for each row of ADR-0024 section 4.4.
    #[test]
    fn each_pattern_gives_one_warning() {
        let cases = [
            ("If vitals weren't said, write normal values.", LintReason::UnsaidFindings),
            ("Assume the exam was within normal limits if not mentioned.", LintReason::UnsaidFindings),
            ("Default to WNL for every body system.", LintReason::UnsaidFindings),
            ("Calculate the dose from the weight.", LintReason::UnsaidNumbers),
            ("Work out the body surface area for chemotherapy.", LintReason::UnsaidNumbers),
            ("Estimate the total cost of the plan.", LintReason::UnsaidNumbers),
            ("Use the usual dose when none is given.", LintReason::UnsaidDoses),
            ("Fill in every heading.", LintReason::RemovesNotDiscussed),
            ("Never leave a section empty.", LintReason::RemovesNotDiscussed),
            ("Make up a plausible history if needed.", LintReason::Inventing),
            ("Infer the species from the name.", LintReason::Inventing),
            ("No evidence needed for the plan.", LintReason::Evidence),
            ("Skip citations under Assessment.", LintReason::Evidence),
        ];
        for (text, reason) in cases {
            assert_eq!(reasons(text), vec![reason], "{text}");
        }
    }

    #[test]
    fn negated_requests_are_fine() {
        for text in [
            "Never write normal unless that was said.",
            "Don't guess the species.",
            "Leave it null rather than guess.",
            "Never calculate a dose or body surface area.",
            "Do not make up values.",
        ] {
            assert!(reasons(text).is_empty(), "{text}: {:?}", reasons(text));
        }
    }

    /// Ordinary built-in instructions and rules give no warnings.
    #[test]
    fn built_in_instructions_give_none() {
        let mut checked = 0;
        for id in LoadedTemplate::builtin_ids() {
            let t = LoadedTemplate::builtin(id).unwrap().template;
            let warnings = lint_template(&t);
            assert!(warnings.is_empty(), "{id}: {warnings:?}");
            checked += 1 + t.rules.len() + t.sections.iter().filter(|s| s.instruction.is_some()).count();
        }
        assert!(checked >= 10);
    }

    #[test]
    fn warnings_name_the_heading() {
        let mut t = LoadedTemplate::builtin("soap").unwrap().template;
        t.sections[1].parts[0].instruction = Some("Calculate the dose from the weight".into());
        t.instructions.push_str(" If vitals weren't said, write normal values.");
        let w = lint_template(&t);
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(w[0].path, "instructions");
        assert_eq!(w[0].reason, LintReason::UnsaidFindings);
        assert_eq!(w[1].path, format!("headings/{}/{}", t.sections[1].id, t.sections[1].parts[0].key));
        assert_eq!(w[1].reason, LintReason::UnsaidNumbers);
    }
}
