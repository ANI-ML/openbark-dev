//! Faithfulness pass (M0 version).
//!
//! Checks a generated note against its transcript:
//! - every cited evidence id must exist in the transcript;
//! - every number in a statement/field (vitals, doses, durations…) must
//!   appear in the transcript segments that statement cites.
//!
//! Numbers are where hallucinations do the most harm, so an unsupported number
//! is flagged in the draft and fails the eval harness (master prompt §4.8).
//! Claim-level semantic alignment arrives in M2.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use crate::numbers::{extract_numbers, normalize_spoken};
use crate::transcript::Transcript;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnsupportedNumber {
    /// JSON pointer to the statement or field, e.g. `/objective/vitals/temperature`.
    pub path: String,
    pub number: String,
    pub cited: Vec<u32>,
    /// Segments where the number does occur. Empty: absent from the transcript.
    pub found_in: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DisputedNumber {
    pub path: String,
    pub number: String,
    /// What the second recogniser heard instead in the cited lines.
    pub heard_instead: Vec<String>,
    pub cited: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FaithfulnessReport {
    pub statements_checked: u32,
    pub numbers_checked: u32,
    /// Paths whose evidence cites ids that are not in the transcript.
    pub invalid_evidence: Vec<String>,
    /// Numbers absent from the whole transcript (possible hallucinations).
    pub unsupported_numbers: Vec<UnsupportedNumber>,
    /// Numbers present in the transcript, but not in the cited segments.
    pub citation_mismatches: Vec<UnsupportedNumber>,
    /// Numbers the second recogniser heard differently in the cited lines
    /// ("10" vs "100"): the recording should be checked before signing.
    #[serde(default)]
    pub disputed_numbers: Vec<DisputedNumber>,
    /// Patient details (species, breed, sex, age, name) whose words don't
    /// appear in the cited lines, e.g. a species guessed from the complaint.
    #[serde(default)]
    pub unverified_values: Vec<String>,
    /// Items a veterinarian wrote or edited by hand (`entered_by`). They are
    /// the vet's own statements, so they are listed here instead of checked.
    #[serde(default)]
    pub vet_entered: Vec<String>,
    /// Statements whose words aren't in the lines they cite (claim-level
    /// alignment, M2): flagged for checking, never a hard fail.
    #[serde(default)]
    pub unsupported_statements: Vec<crate::align::UnsupportedStatement>,
    /// Flags on charted teeth (ADR-0021 decision 7): a number and a name that
    /// mean different teeth, a finding with no word for it in the tooth's
    /// clause or said about another tooth, a finding said to be absent or only
    /// planned, fields that don't fit, findings that contradict each other, a
    /// finding listed twice. Soft, except a tooth outside the chart's mouth or
    /// a dog-only tooth with no dentition chosen, which block signing. A tooth
    /// said nowhere is a hard fail in `unsupported_numbers` instead.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tooth_flags: Vec<crate::dental::ToothFlag>,
    /// No invalid evidence, no unsupported numbers and no tooth flag that
    /// blocks signing (vet-entered items aside).
    pub passed: bool,
}

impl FaithfulnessReport {
    /// Paths of statements that should be visibly flagged in the draft.
    pub fn flagged_paths(&self) -> HashSet<String> {
        self.invalid_evidence
            .iter()
            .cloned()
            .chain(self.unsupported_numbers.iter().map(|u| u.path.clone()))
            .chain(self.citation_mismatches.iter().map(|u| u.path.clone()))
            .chain(self.disputed_numbers.iter().map(|d| d.path.clone()))
            .chain(self.unverified_values.iter().cloned())
            .chain(self.unsupported_statements.iter().map(|s| s.path.clone()))
            .chain(self.tooth_flags.iter().map(|f| f.path.clone()))
            .collect()
    }
}

pub fn check(note: &Value, transcript: &Transcript) -> FaithfulnessReport {
    check_with(note, transcript, &HashSet::new())
}

/// Like [`check`], but numbers a veterinarian entered in their own note
/// (`vet_numbers`) also count as supported. Used for client documents, which
/// follow the vet's note where it corrects the transcript.
pub fn check_with(
    note: &Value,
    transcript: &Transcript,
    vet_numbers: &HashSet<String>,
) -> FaithfulnessReport {
    check_opts(note, transcript, vet_numbers, true)
}

/// The full check. `align` turns claim-level alignment on; it's off when the
/// note is written in a different language from the consultation, where
/// words can't match. A tooth chart is checked with no dentition recorded;
/// see [`check_template`].
pub fn check_opts(
    note: &Value,
    transcript: &Transcript,
    vet_numbers: &HashSet<String>,
    align: bool,
) -> FaithfulnessReport {
    check_full(note, transcript, vet_numbers, align, None)
}

/// [`check`] for a note drafted with `template`: its tooth chart is checked
/// against the chart's dentition.
pub fn check_template(
    note: &Value,
    transcript: &Transcript,
    template: &crate::template::LoadedTemplate,
) -> FaithfulnessReport {
    check_full(note, transcript, &HashSet::new(), true, template.chart().and_then(|(_, d)| d))
}

/// Everything, with the tooth chart's dentition (None: not recorded, so a
/// tooth only a dog has blocks signing until dog or cat is chosen).
pub fn check_full(
    note: &Value,
    transcript: &Transcript,
    vet_numbers: &HashSet<String>,
    align: bool,
    dentition: Option<crate::dental::Dentition>,
) -> FaithfulnessReport {
    check_for_pet(note, transcript, vet_numbers, align, dentition, &[])
}

/// [`check_full`] for one animal of a multi-pet visit (`other_pets`: the
/// other animals' names): a charted tooth whose cited clause names another
/// animal ("Rex has a fractured 108, Bella doesn't") is flagged.
pub fn check_for_pet(
    note: &Value,
    transcript: &Transcript,
    vet_numbers: &HashSet<String>,
    align: bool,
    dentition: Option<crate::dental::Dentition>,
    other_pets: &[String],
) -> FaithfulnessReport {
    let mut report = FaithfulnessReport::default();
    let ctx = Ctx { transcript, dentition, other_pets };
    walk(note, String::new(), &ctx, &mut report);
    if align {
        let aligner = crate::align::Aligner::new(transcript);
        let invalid: HashSet<String> = report.invalid_evidence.iter().cloned().collect();
        align_walk(note, String::new(), &aligner, &invalid, &mut report);
    }
    let from_vet = |n: &UnsupportedNumber| vet_numbers.contains(&n.number);
    report.unsupported_numbers.retain(|n| !from_vet(n));
    report.citation_mismatches.retain(|n| !from_vet(n));
    report.passed = report.invalid_evidence.is_empty()
        && report.unsupported_numbers.is_empty()
        && !report.tooth_flags.iter().any(|f| f.kind.blocks_signing());
    report
}

/// What the walk checks against.
struct Ctx<'a> {
    transcript: &'a Transcript,
    dentition: Option<crate::dental::Dentition>,
    /// The other animals of a multi-pet visit.
    other_pets: &'a [String],
}

/// Numbers in the items a veterinarian wrote or edited (`entered_by`).
pub fn vet_entered_numbers(note: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    fn walk(v: &Value, out: &mut HashSet<String>) {
        match v {
            Value::Object(m)
                if m.get("entered_by").and_then(Value::as_str).is_some_and(|b| !b.is_empty()) =>
            {
                for (k, x) in m {
                    let text = match x {
                        Value::String(s) if k != "entered_by" => s.clone(),
                        Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    out.extend(extract_numbers(&text));
                }
            }
            Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(note, &mut out);
    out
}

/// The words of an item that make its claim: a statement's text, a
/// medication's drug name, a field's description (numbers are checked apart).
fn claim_text(map: &serde_json::Map<String, Value>, path: &str) -> Option<String> {
    if let Some(Value::String(t)) = map.get("text") {
        return Some(t.clone());
    }
    if let Some(Value::String(d)) = map.get("drug") {
        return Some(d.clone());
    }
    if path.starts_with("/patient/") {
        return None; // patient details have their own check
    }
    match map.get("value") {
        Some(Value::String(v)) => Some(v.clone()),
        _ => None,
    }
}

fn align_walk(
    value: &Value,
    path: String,
    aligner: &crate::align::Aligner,
    invalid: &HashSet<String>,
    report: &mut FaithfulnessReport,
) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(evidence)) = map.get("evidence") {
                let vet = map.get("entered_by").and_then(Value::as_str).is_some_and(|b| !b.is_empty());
                if vet || invalid.contains(&path) {
                    return;
                }
                // A charted tooth prints its finding's words, not its `text`,
                // and the cue-word check (`dental::check_item`) is its word
                // check ("come out" is an extraction). Items printed as said
                // are also aligned: `other`, and items whose fields don't fit
                // their finding (the record prints their `text`).
                if crate::dental::is_tooth_item(map)
                    && map.get("finding").and_then(Value::as_str) != Some("other")
                    && crate::dental::fits(value).is_ok()
                {
                    return;
                }
                let cited: Vec<u32> = evidence.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
                if let Some(text) = claim_text(map, &path)
                    && let Some(flag) = aligner.check(&path, &text, &cited)
                {
                    report.unsupported_statements.push(flag);
                }
                return;
            }
            for (k, v) in map {
                align_walk(v, format!("{path}/{k}"), aligner, invalid, report);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                align_walk(v, format!("{path}/{i}"), aligner, invalid, report);
            }
        }
        _ => {}
    }
}

fn walk(value: &Value, path: String, ctx: &Ctx, report: &mut FaithfulnessReport) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(evidence)) = map.get("evidence") {
                check_statement(map, &path, evidence, ctx, report);
                return;
            }
            for (k, v) in map {
                walk(v, format!("{path}/{k}"), ctx, report);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, format!("{path}/{i}"), ctx, report);
            }
            // A tooth chart: the same finding twice on one tooth, or findings
            // that contradict each other.
            if items.iter().any(|v| v.as_object().is_some_and(crate::dental::is_tooth_item)) {
                report.tooth_flags.extend(crate::dental::duplicates(items, &path));
            }
        }
        _ => {}
    }
}

/// Words that count as saying a species.
const SPECIES: &[(&str, &[&str])] = &[
    ("dog", &["dog", "dogs", "canine", "puppy", "pup", "bitch"]),
    ("canine", &["dog", "dogs", "canine", "puppy"]),
    ("cat", &["cat", "cats", "feline", "kitten", "kitty"]),
    ("feline", &["cat", "cats", "feline", "kitten"]),
    ("horse", &["horse", "equine", "mare", "gelding", "stallion", "foal", "pony"]),
    ("rabbit", &["rabbit", "bunny"]),
];

/// Is a short patient detail actually said in the cited lines?
fn value_supported(value: &str, cited_text: &str) -> bool {
    let said: Vec<String> = cited_text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_end_matches("'s").to_string())
        .collect();
    let words: Vec<String> = value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(String::from)
        .collect();
    if words.is_empty() {
        return true; // numbers and short tokens are covered by the number check
    }
    words.iter().any(|w| {
        let alternatives: Vec<&str> = SPECIES
            .iter()
            .find(|(k, _)| k == w)
            .map(|(_, alts)| alts.to_vec())
            .unwrap_or_else(|| vec![w.as_str()]);
        alternatives.iter().any(|a| said.iter().any(|s| s == a || s.starts_with(a)))
    })
}

fn check_statement(
    map: &serde_json::Map<String, Value>,
    path: &str,
    evidence: &[Value],
    ctx: &Ctx,
    report: &mut FaithfulnessReport,
) {
    let transcript = ctx.transcript;
    if map.get("entered_by").and_then(Value::as_str).is_some_and(|b| !b.is_empty()) {
        report.vet_entered.push(path.to_string());
        // The vet's own tooth still has to be in the chart's mouth.
        if crate::dental::is_tooth_item(map) {
            report.tooth_flags.extend(crate::dental::check_item(map, path, transcript, ctx.dentition).flags);
        }
        return;
    }
    report.statements_checked += 1;
    let cited: Vec<u32> = evidence.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
    if cited.is_empty() || cited.iter().any(|id| transcript.get(*id).is_none()) {
        report.invalid_evidence.push(path.to_string());
    }
    let mut supported: HashSet<String> = cited
        .iter()
        .filter_map(|id| transcript.get(*id))
        .flat_map(|s| extract_numbers(&normalize_spoken(&s.text)))
        .collect();
    // A charted tooth (ADR-0021 decision 7): the tooth check replaces the
    // number check for the `tooth` key, and a tooth said by name, range or
    // group in the cited lines supports its number in `text` too.
    let tooth_item = crate::dental::is_tooth_item(map);
    if tooth_item {
        let teeth = crate::dental::check_item(map, path, transcript, ctx.dentition);
        report.numbers_checked += teeth.checked;
        report.unsupported_numbers.extend(teeth.unsupported);
        report.citation_mismatches.extend(teeth.mismatches);
        report.disputed_numbers.extend(teeth.disputed);
        report.tooth_flags.extend(teeth.flags);
        if let Some(tooth) = map.get("tooth").and_then(Value::as_str)
            && !ctx.other_pets.is_empty()
            && cited
                .iter()
                .filter_map(|id| transcript.get(*id))
                .any(|s| crate::dental::clause_names_animal(&s.text, tooth, ctx.other_pets))
        {
            report.tooth_flags.push(crate::dental::ToothFlag {
                path: path.to_string(),
                tooth: tooth.to_string(),
                kind: crate::dental::ToothFlagKind::OtherPet,
                other: None,
                expected: Vec::new(),
            });
        }
        supported.extend(
            cited
                .iter()
                .filter_map(|id| transcript.get(*id))
                .flat_map(|s| crate::dental::teeth_in(&s.text))
                .flat_map(|m| m.teeth),
        );
    }
    if path.starts_with("/patient/")
        && let Some(Value::String(value)) = map.get("value")
    {
        let cited_text: String = cited
            .iter()
            .filter_map(|id| transcript.get(*id))
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if !value_supported(value, &cited_text) {
            report.unverified_values.push(path.to_string());
        }
    }
    // Teeth said in the cited lines by number, name, range or group, read as
    // the chart's own check reads them ("the lower incisors" is 301 to 303
    // and 401 to 403).
    let said_teeth: HashSet<String> = cited
        .iter()
        .filter_map(|id| transcript.get(*id))
        .flat_map(|s| crate::dental::teeth_in(&s.text))
        .flat_map(|m| m.teeth)
        .collect();
    for (key, v) in map {
        if key == "evidence" || key == "entered_by" || (tooth_item && key == "tooth") {
            continue;
        }
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        // Tooth numbers the statement itself names ("Extraction of 301, 302,
        // 303"): supported when the cited lines say those teeth in any way,
        // in any heading, not only the chart. Never a dose or a vital: the
        // statement must read the number as a tooth.
        let as_teeth: HashSet<String> = crate::dental::teeth_in(&text)
            .into_iter()
            .filter(|m| m.said_as != crate::dental::SaidAs::Group)
            .flat_map(|m| m.teeth)
            .collect();
        {
            for number in extract_numbers(&text) {
                if as_teeth.contains(&number) && said_teeth.contains(&number) {
                    report.numbers_checked += 1;
                    continue;
                }
                report.numbers_checked += 1;
                let disputed: Vec<String> = cited
                    .iter()
                    .filter_map(|id| transcript.get(*id))
                    .filter_map(|s| s.check.as_ref())
                    .filter(|c| c.primary_only.contains(&number))
                    .flat_map(|c| c.second_only.clone())
                    .collect();
                if !disputed.is_empty() {
                    report.disputed_numbers.push(DisputedNumber {
                        path: path.to_string(),
                        number: number.clone(),
                        heard_instead: disputed,
                        cited: cited.clone(),
                    });
                }
                if !supported.contains(&number) {
                    let found_in: Vec<u32> = transcript
                        .segments
                        .iter()
                        .filter(|s| {
                            extract_numbers(&normalize_spoken(&s.text)).contains(&number)
                                || ((tooth_item || as_teeth.contains(&number))
                                    && crate::dental::teeth_in(&s.text).iter().any(|m| m.covers(&number)))
                        })
                        .map(|s| s.id)
                        .collect();
                    let entry =
                        UnsupportedNumber { path: path.to_string(), number, cited: cited.clone(), found_in };
                    if entry.found_in.is_empty() {
                        report.unsupported_numbers.push(entry);
                    } else {
                        report.citation_mismatches.push(entry);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Segment;
    use serde_json::json;

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
                seg(0, "Her temperature is thirty-nine point two."),
                seg(1, "She weighs 4.2 kilos."),
                seg(2, "Give maropitant one milligram per kilo once daily for 3 days."),
            ],
        }
    }

    #[test]
    fn supported_numbers_pass() {
        let note = json!({
            "objective": {"vitals": {
                "temperature": {"value": "39.2 °C", "evidence": [0]},
                "weight": {"value": "4.2 kg", "evidence": [1]},
                "heart_rate": null
            }},
            "plan": {"medications": [
                {"drug": "Maropitant", "dose": "1 mg/kg", "route": null, "frequency": "SID", "duration": "3 days", "evidence": [2]}
            ]}
        });
        let r = check(&note, &transcript());
        assert!(r.passed, "{r:?}");
        assert_eq!(r.statements_checked, 3);
        assert_eq!(r.numbers_checked, 4);
    }

    #[test]
    fn invented_vital_is_flagged() {
        let note = json!({"objective": {"vitals": {"heart_rate": {"value": "120 bpm", "evidence": [0]}}}});
        let r = check(&note, &transcript());
        assert!(!r.passed);
        assert_eq!(r.unsupported_numbers[0].path, "/objective/vitals/heart_rate");
        assert_eq!(r.unsupported_numbers[0].number, "120");
    }

    #[test]
    fn number_from_wrong_segment_is_a_citation_mismatch() {
        // 4.2 is in the transcript, but not in the cited segment.
        let note = json!({"objective": {"vitals": {"weight": {"value": "4.2 kg", "evidence": [0]}}}});
        let r = check(&note, &transcript());
        assert!(r.passed, "a correct value with a wrong citation is not a hallucination");
        assert!(r.unsupported_numbers.is_empty());
        assert_eq!(r.citation_mismatches[0].found_in, vec![1]);
        assert!(r.flagged_paths().contains("/objective/vitals/weight"), "still flagged in the draft");
    }

    #[test]
    fn number_the_second_recogniser_heard_differently_is_disputed() {
        let mut t = transcript();
        t.segments.push(Segment {
            id: 3,
            start: 0.0,
            end: 1.0,
            speaker: None,
            text: "Also give her 10 milliliters of fluids.".into(),
            hints: Vec::new(),
            check: Some(crate::crosscheck::compare(
                "Also give her 10 milliliters of fluids.",
                "Also give her 100 milliliters of fluids.",
            )),
        });
        let note = json!({"plan": {"medications": [
            {"drug": "LRS", "dose": "10 mL", "route": "SC", "frequency": null, "duration": null, "evidence": [3]}
        ]}});
        let r = check(&note, &t);
        assert_eq!(r.disputed_numbers.len(), 1);
        assert_eq!(r.disputed_numbers[0].heard_instead, vec!["100"]);
        assert!(r.flagged_paths().contains("/plan/medications/0"));
    }

    #[test]
    fn guessed_species_is_unverified() {
        let mut t = transcript();
        t.segments.push(Segment {
            id: 3,
            start: 0.0,
            end: 1.0,
            speaker: None,
            text: "So this is Pepper's recheck for the ear infection.".into(),
            hints: Vec::new(),
            check: None,
        });
        let note = json!({"patient": {
            "name": {"value": "Pepper", "unit": null, "evidence": [3]},
            "species": {"value": "dog", "unit": null, "evidence": [3]}
        }});
        let r = check(&note, &t);
        assert_eq!(r.unverified_values, vec!["/patient/species"]);
        assert!(r.passed, "a guessed word is flagged for review, not a hard fail");
        assert!(value_supported("Feline", "Her cat has been vomiting"));
        assert!(value_supported("Golden Retriever", "Maple is a golden retriever"));
    }

    #[test]
    fn missing_or_unknown_evidence_is_flagged() {
        let note = json!({"subjective": {"history": [
            {"text": "Vomiting", "evidence": [99]},
            {"text": "Lethargic", "evidence": []}
        ]}});
        let r = check(&note, &transcript());
        assert_eq!(r.invalid_evidence, vec!["/subjective/history/0", "/subjective/history/1"]);
        assert_eq!(r.flagged_paths().len(), 2);
    }

    #[test]
    fn numbers_the_vet_entered_are_supported_in_client_documents() {
        let vet_note = json!({"plan": {"medications": [
            {"drug": "LRS", "dose": "100 mL", "evidence": [0], "entered_by": "Dr Patel"},
            {"drug": "Maropitant", "dose": "1 mg/kg", "evidence": [2]}
        ]}});
        let numbers = vet_entered_numbers(&vet_note);
        assert!(numbers.contains("100") && !numbers.contains("1"));
        let discharge =
            json!({"treatment": [{"drug": "Fluids under the skin", "dose": "100 mL", "evidence": [0]}]});
        assert!(!check(&discharge, &transcript()).passed);
        assert!(check_with(&discharge, &transcript(), &numbers).passed);
    }

    fn dental_transcript() -> Transcript {
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
                seg(0, "One oh one through one oh three are missing."),
                seg(1, "The upper right canine is normal."),
                seg(2, "Three oh nine has stage three periodontal disease, a seven millimetre pocket."),
                seg(3, "One oh eight, upper left fourth premolar, has a slab fracture."),
                seg(4, "Overall I'd call her stage two across the mouth."),
            ],
        }
    }

    fn tooth(id: &str, finding: &str, grade: Option<u8>, mm: Option<f64>, evidence: &[u32]) -> Value {
        json!({"tooth": id, "text": format!("{id} {finding}"), "finding": finding, "detail": null,
               "grade": grade, "mm": mm, "planned": false, "evidence": evidence})
    }

    /// ADR-0021 decision 7 through the faithfulness pass: teeth said by name or
    /// range pass, numbers in `text` that are those teeth pass, grades and
    /// millimetres go through the number check.
    #[test]
    fn charted_teeth_said_by_number_name_or_range_pass() {
        let note = json!({"dental_chart": [
            tooth("102", "missing", None, None, &[0]),
            tooth("104", "normal", None, None, &[1]),
            tooth("309", "periodontal_stage", Some(3), None, &[2]),
            tooth("309", "probing_depth", None, Some(7.0), &[2]),
        ]});
        let r = check(&note, &dental_transcript());
        assert!(r.passed, "{r:?}");
        assert!(r.unsupported_numbers.is_empty() && r.citation_mismatches.is_empty(), "{r:?}");
        assert!(r.tooth_flags.is_empty(), "{:?}", r.tooth_flags);
    }

    #[test]
    fn an_invented_tooth_blocks_signing_and_a_wrong_line_is_a_mismatch() {
        let note = json!({"dental_chart": [
            tooth("105", "missing", None, None, &[1]),
            tooth("309", "periodontal_stage", Some(3), None, &[0]),
        ]});
        let r = check(&note, &dental_transcript());
        assert!(!r.passed, "a tooth said nowhere is a hard fail");
        assert!(
            r.unsupported_numbers.iter().any(|u| u.path == "/dental_chart/0" && u.number == "105"),
            "{r:?}"
        );
        assert!(
            r.citation_mismatches.iter().any(|u| u.path == "/dental_chart/1" && u.number == "309"),
            "{r:?}"
        );
        // An invented stage is an unsupported number, as for any number.
        let invented = json!({"dental_chart": [tooth("309", "probing_depth", None, Some(9.0), &[2])]});
        let r = check(&invented, &dental_transcript());
        assert!(r.unsupported_numbers.iter().any(|u| u.number == "9"), "{r:?}");
    }

    #[test]
    fn soft_tooth_flags_name_conflicts_missing_cues_and_duplicates() {
        let note = json!({"dental_chart": [
            tooth("108", "fracture", None, None, &[3]),
            tooth("104", "missing", None, None, &[1]),
            tooth("104", "missing", None, None, &[1]),
        ]});
        let r = check(&note, &dental_transcript());
        assert!(r.passed, "soft flags never block signing: {r:?}");
        let kinds: Vec<(String, crate::dental::ToothFlagKind)> =
            r.tooth_flags.iter().map(|f| (f.path.clone(), f.kind)).collect();
        use crate::dental::ToothFlagKind::*;
        assert!(kinds.contains(&("/dental_chart/0".into(), NameConflict)), "{kinds:?}");
        assert_eq!(
            r.tooth_flags.iter().find(|f| f.kind == NameConflict).unwrap().other.as_deref(),
            Some("208")
        );
        assert!(kinds.contains(&("/dental_chart/1".into(), CueMissing)), "{kinds:?}");
        assert!(kinds.contains(&("/dental_chart/2".into(), Duplicate)), "{kinds:?}");
        assert!(r.flagged_paths().contains("/dental_chart/0"));
        // As the recogniser wrote it in the eval fixture: digits, an ordinal, a misspelling.
        let mut t = dental_transcript();
        t.segments[3].text =
            "Alright charting. 108 Upper Left 4th Primolar has a slab fracture with pulp exposure.".into();
        let r = check(&json!({"dental_chart": [tooth("108", "fracture", None, None, &[3])]}), &t);
        assert_eq!(
            r.tooth_flags.iter().find(|f| f.kind == NameConflict).and_then(|f| f.other.as_deref()),
            Some("208")
        );
        // Old reports without the list still serialise without it.
        let plain = check(&json!({"a": []}), &dental_transcript());
        assert!(serde_json::to_value(&plain).unwrap().get("tooth_flags").is_none());
    }

    /// "The lower incisors come out" charted as extraction: the cue check
    /// passes ("out"), so the word check doesn't also flag "extraction". An
    /// `other` finding prints its words as said, so they are still aligned.
    #[test]
    fn charted_findings_use_the_cue_check_not_the_word_check() {
        let mut t = dental_transcript();
        t.segments.push(Segment {
            id: 5,
            start: 0.0,
            end: 1.0,
            speaker: Some(0),
            text: "The lower incisors come out closed, no sectioning.".into(),
            hints: Vec::new(),
            check: None,
        });
        let mut out = tooth("301", "extraction", None, None, &[5]);
        out["text"] = json!("extraction");
        out["detail"] = json!("closed");
        let mut other = tooth("301", "other", None, None, &[5]);
        other["text"] = json!("gold crown placed");
        let r = check_opts(&json!({"dental_chart": [out, other]}), &t, &HashSet::new(), true);
        let flagged: Vec<&str> = r.unsupported_statements.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(flagged, ["/dental_chart/1"], "{r:?}");
        assert!(r.tooth_flags.iter().all(|f| f.path != "/dental_chart/0"), "{:?}", r.tooth_flags);
    }

    /// The chart's mouth (ADR-0021 decision 4): a tooth only a dog has blocks
    /// signing until dog or cat is chosen, and a tooth outside the chosen mouth
    /// blocks it too; with a dog chosen it passes.
    #[test]
    fn the_charts_dentition_decides_which_teeth_pass() {
        let mut t = dental_transcript();
        t.segments.push(Segment {
            id: 5,
            start: 0.0,
            end: 1.0,
            speaker: Some(0),
            text: "One oh five is missing.".into(),
            hints: Vec::new(),
            check: None,
        });
        let note = json!({"dental_chart": [tooth("105", "missing", None, None, &[5])]});
        use crate::dental::{Dentition, ToothFlagKind};
        let unknown = check_full(&note, &t, &HashSet::new(), true, None);
        assert!(!unknown.passed, "{unknown:?}");
        assert_eq!(unknown.tooth_flags[0].kind, ToothFlagKind::ChooseDentition);
        assert!(unknown.flagged_paths().contains("/dental_chart/0"));
        let cat = check_full(&note, &t, &HashSet::new(), true, Some(Dentition::Feline));
        assert!(!cat.passed);
        assert_eq!(cat.tooth_flags[0].kind, ToothFlagKind::NotInMouth);
        let dog = check_full(&note, &t, &HashSet::new(), true, Some(Dentition::Canine));
        assert!(dog.passed && dog.tooth_flags.is_empty(), "{dog:?}");
        // Through a template: the cat's chart.
        let cat_template =
            crate::template::LoadedTemplate::builtin("dental").unwrap().for_species(Some("cat"));
        assert!(!check_template(&note, &t, &cat_template).passed);
    }

    /// An item whose fields don't fit its finding prints its free text, so
    /// that text is word-checked like an `other` finding.
    #[test]
    fn a_misfit_chart_item_prints_its_text_and_so_is_word_checked() {
        let t = dental_transcript();
        let mut odd = tooth("309", "fracture", None, None, &[2]);
        odd["detail"] = json!("open");
        odd["text"] = json!("complicated crown-root fracture, extracted surgically");
        let r = check_full(&json!({"dental_chart": [odd]}), &t, &HashSet::new(), true, None);
        assert!(r.unsupported_statements.iter().any(|s| s.path == "/dental_chart/0"), "{r:?}");
    }

    #[test]
    fn vet_entered_items_are_listed_not_failed() {
        // The vet corrected an ASR error: 120 isn't in the transcript, but it's theirs.
        let note = json!({"objective": {"vitals": {
            "heart_rate": {"value": "120 bpm", "unit": null, "evidence": [0], "entered_by": "Dr Patel"},
            "weight": {"value": 4.2, "unit": "kg", "evidence": [1]}
        }}, "plan": {"follow_up": [{"text": "Recheck in 10 days", "evidence": [], "entered_by": "Dr Patel"}]}});
        let r = check(&note, &transcript());
        assert!(r.passed, "{r:?}");
        assert_eq!(r.vet_entered, vec!["/objective/vitals/heart_rate", "/plan/follow_up/0"]);
        assert_eq!(r.statements_checked, 1);
        assert!(r.flagged_paths().is_empty());
    }
}
