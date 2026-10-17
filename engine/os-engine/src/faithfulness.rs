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
    /// No invalid evidence and no unsupported numbers.
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
            .collect()
    }
}

pub fn check(note: &Value, transcript: &Transcript) -> FaithfulnessReport {
    let mut report = FaithfulnessReport::default();
    walk(note, String::new(), transcript, &mut report);
    report.passed = report.invalid_evidence.is_empty() && report.unsupported_numbers.is_empty();
    report
}

fn walk(value: &Value, path: String, transcript: &Transcript, report: &mut FaithfulnessReport) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(evidence)) = map.get("evidence") {
                check_statement(map, &path, evidence, transcript, report);
                return;
            }
            for (k, v) in map {
                walk(v, format!("{path}/{k}"), transcript, report);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, format!("{path}/{i}"), transcript, report);
            }
        }
        _ => {}
    }
}

fn check_statement(
    map: &serde_json::Map<String, Value>,
    path: &str,
    evidence: &[Value],
    transcript: &Transcript,
    report: &mut FaithfulnessReport,
) {
    report.statements_checked += 1;
    let cited: Vec<u32> = evidence.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
    if cited.is_empty() || cited.iter().any(|id| transcript.get(*id).is_none()) {
        report.invalid_evidence.push(path.to_string());
    }
    let supported: HashSet<String> = cited
        .iter()
        .filter_map(|id| transcript.get(*id))
        .flat_map(|s| extract_numbers(&normalize_spoken(&s.text)))
        .collect();
    for (key, v) in map {
        if key == "evidence" {
            continue;
        }
        let text = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        {
            for number in extract_numbers(&text) {
                report.numbers_checked += 1;
                if !supported.contains(&number) {
                    let found_in: Vec<u32> = transcript
                        .segments
                        .iter()
                        .filter(|s| extract_numbers(&normalize_spoken(&s.text)).contains(&number))
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
    fn missing_or_unknown_evidence_is_flagged() {
        let note = json!({"subjective": {"history": [
            {"text": "Vomiting", "evidence": [99]},
            {"text": "Lethargic", "evidence": []}
        ]}});
        let r = check(&note, &transcript());
        assert_eq!(r.invalid_evidence, vec!["/subjective/history/0", "/subjective/history/1"]);
        assert_eq!(r.flagged_paths().len(), 2);
    }
}
