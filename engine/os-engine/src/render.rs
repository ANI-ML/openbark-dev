//! Render a structured note to Markdown for review and copy-out.
//!
//! Empty parts render as "Not discussed." so absence is explicit, never
//! silently filled. Statements failing the faithfulness pass are marked.

use std::collections::HashSet;

use serde_json::Value;

use crate::faithfulness::FaithfulnessReport;
use crate::template::{FieldDef, LoadedTemplate, PartKind};
use crate::transcript::Transcript;

const NOT_DISCUSSED: &str = "Not discussed.";
const FLAG: &str = " [check against recording]";

pub fn markdown(
    template: &LoadedTemplate,
    note: &Value,
    _transcript: &Transcript,
    faithfulness: &FaithfulnessReport,
) -> String {
    let flagged = faithfulness.flagged_paths();
    let status = match template.template.document {
        crate::template::Document::Record => "*Draft: not reviewed or signed by a veterinarian.*",
        crate::template::Document::Client => "*Draft for the client: check it before handing it over.*",
    };
    let mut out = format!("# {}\n\n{status}\n", template.template.name);
    if !flagged.is_empty() {
        out.push_str(&format!(
            "\n> {} item(s) could not be matched to the transcript and are marked [check against recording].\n",
            flagged.len()
        ));
    }
    for section in &template.template.sections {
        out.push_str(&format!("\n## {}\n", section.title));
        let sec_value = &note[&section.id];
        let sec_path = format!("/{}", section.id);
        if let Some(kind) = section.kind {
            out.push('\n');
            out.push_str(&match kind {
                PartKind::Statements => statements(sec_value, &sec_path, &flagged),
                PartKind::Fields => fields(&section.fields, sec_value, &sec_path, &flagged),
                PartKind::Medications => medications(sec_value, &sec_path, &flagged),
            });
            continue;
        }
        for part in &section.parts {
            let path = format!("{sec_path}/{}", part.key);
            let value = &sec_value[&part.key];
            out.push_str(&format!("\n**{}**\n\n", part.label));
            out.push_str(&match part.kind {
                PartKind::Statements => statements(value, &path, &flagged),
                PartKind::Fields => fields(&part.fields, value, &path, &flagged),
                PartKind::Medications => medications(value, &path, &flagged),
            });
        }
    }
    out
}

fn cite(value: &Value) -> String {
    if let Some(by) = value["entered_by"].as_str().filter(|b| !b.is_empty()) {
        return format!(" [entered by {by}]");
    }
    let ids: Vec<String> = value["evidence"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).map(|id| id.to_string()).collect())
        .unwrap_or_default();
    if ids.is_empty() { String::new() } else { format!(" [{}]", ids.join(", ")) }
}

fn flag(path: &str, flagged: &HashSet<String>) -> &'static str {
    if flagged.contains(path) { FLAG } else { "" }
}

fn statements(value: &Value, path: &str, flagged: &HashSet<String>) -> String {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if items.is_empty() {
        return format!("{NOT_DISCUSSED}\n");
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let p = format!("{path}/{i}");
            format!("- {}{}{}\n", item["text"].as_str().unwrap_or_default(), cite(item), flag(&p, flagged))
        })
        .collect()
}

fn fields(defs: &[FieldDef], value: &Value, path: &str, flagged: &HashSet<String>) -> String {
    let mut out = String::new();
    let mut missing = Vec::new();
    for def in defs {
        let field = &value[&def.key];
        match field_text(field) {
            Some(v) => {
                let p = format!("{path}/{}", def.key);
                out.push_str(&format!("- {}: {}{}{}\n", def.label, v, cite(field), flag(&p, flagged)));
            }
            None => missing.push(def.label.as_str()),
        }
    }
    if out.is_empty() {
        return format!("{NOT_DISCUSSED}\n");
    }
    if !missing.is_empty() {
        out.push_str(&format!("- Not discussed: {}\n", missing.join(", ")));
    }
    out
}

/// A field's display text: "32.4 kg", "pink but tacky".
pub fn field_text(field: &Value) -> Option<String> {
    let value = match &field["value"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    match field["unit"].as_str().filter(|u| !u.is_empty() && !value.ends_with(*u)) {
        Some(unit) => Some(format!("{value} {unit}")),
        None => Some(value),
    }
}

fn medications(value: &Value, path: &str, flagged: &HashSet<String>) -> String {
    let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
    if items.is_empty() {
        return format!("{NOT_DISCUSSED}\n");
    }
    items
        .iter()
        .enumerate()
        .map(|(i, med)| {
            let details: Vec<&str> =
                ["dose", "route", "frequency", "duration"].iter().filter_map(|k| med[*k].as_str()).collect();
            let details = if details.is_empty() {
                " (dose/route/frequency not stated)".to_string()
            } else {
                format!(": {}", details.join(", "))
            };
            let p = format!("{path}/{i}");
            format!(
                "- {}{}{}{}\n",
                med["drug"].as_str().unwrap_or("?"),
                details,
                cite(med),
                flag(&p, flagged)
            )
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::faithfulness;
    use crate::transcript::Segment;
    use serde_json::json;

    pub fn empty_note() -> Value {
        json!({
            "patient": {"name": null, "species": null, "breed": null, "age": null, "sex": null},
            "subjective": {"chief_complaint": [], "history": [], "current_medications_and_diet": []},
            "objective": {
                "vitals": {"weight": null, "temperature": null, "heart_rate": null, "respiratory_rate": null,
                           "mucous_membranes": null, "capillary_refill_time": null, "hydration": null,
                           "body_condition_score": null, "pain_score": null},
                "physical_exam": [], "diagnostic_results": []
            },
            "assessment": {"problems": [], "differentials": []},
            "plan": {"diagnostics": [], "medications": [], "procedures_and_treatments": [],
                     "client_communication": [], "follow_up": []}
        })
    }

    fn transcript() -> Transcript {
        Transcript {
            segments: vec![
                Segment {
                    id: 0,
                    start: 0.0,
                    end: 1.0,
                    speaker: Some(1),
                    text: "Milo has been vomiting for two days.".into(),
                    hints: Vec::new(),
                    check: None,
                },
                Segment {
                    id: 1,
                    start: 1.0,
                    end: 2.0,
                    speaker: Some(0),
                    text: "Temperature is 39.4.".into(),
                    hints: Vec::new(),
                    check: None,
                },
            ],
        }
    }

    #[test]
    fn empty_note_says_not_discussed_everywhere() {
        let template = LoadedTemplate::builtin("soap").unwrap();
        let note = empty_note();
        let report = faithfulness::check(&note, &transcript());
        let md = markdown(&template, &note, &transcript(), &report);
        assert!(md.starts_with("# SOAP note\n\n*Draft"));
        // patient + 13 parts across S/O/A/P
        assert_eq!(md.matches(NOT_DISCUSSED).count(), 14, "{md}");
        assert!(!md.contains("check against recording"));
    }

    #[test]
    fn renders_statements_fields_medications_and_flags() {
        let template = LoadedTemplate::builtin("soap").unwrap();
        let mut note = empty_note();
        note["subjective"]["chief_complaint"] = json!([{"text": "Vomiting for 2 days", "evidence": [0]}]);
        note["objective"]["vitals"]["temperature"] = json!({"value": "39.4 °C", "evidence": [1]});
        note["objective"]["vitals"]["heart_rate"] = json!({"value": "140 bpm", "evidence": [1]});
        note["plan"]["medications"] = json!([{"drug": "Maropitant", "dose": null, "route": null, "frequency": null, "duration": null, "evidence": [0]}]);
        let report = faithfulness::check(&note, &transcript());
        let md = markdown(&template, &note, &transcript(), &report);
        assert!(md.contains("- Vomiting for 2 days [0]\n"), "{md}");
        assert!(md.contains("- Temperature: 39.4 °C [1]\n"));
        assert!(
            md.contains("- HR: 140 bpm [1] [check against recording]"),
            "invented HR must be flagged:\n{md}"
        );
        assert!(md.contains("- Not discussed: Weight, RR, MM, CRT, Hydration, BCS, Pain score\n"));
        // Nobody said "maropitant" in this transcript: the word check flags it.
        assert!(
            md.contains("- Maropitant (dose/route/frequency not stated) [0] [check against recording]\n"),
            "{md}"
        );
        assert!(md.contains("2 item(s) could not be matched"));
    }
}
