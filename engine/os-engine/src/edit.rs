//! A veterinarian's own edits to a draft (M1 item 6).
//!
//! The vet replaces the content of one heading. Every item that differs from
//! the draft it came from is marked `entered_by: <vet>`: it is the vet's own
//! statement, kept with its original citation if it had one, and the
//! faithfulness pass lists it instead of checking it (a corrected dose must
//! not block signing). Unchanged items keep their checks. The result is a new
//! run; the draft before the edit stays on disk.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::EngineError;
use crate::faithfulness;
use crate::note::NoteResult;
use crate::render;
use crate::template::{LoadedTemplate, split_heading};
use crate::transcript::Transcript;

/// What a hand edit changed, kept with the run it produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edit {
    /// Heading path that was edited: "plan" or "plan/medications".
    pub heading: String,
    pub by: String,
    /// The run that was edited ("runs/002_…").
    #[serde(default)]
    pub from_run: Option<String>,
    /// Paths of items the vet wrote or changed.
    pub changed: Vec<String>,
    /// Items that were in the heading before and are gone now.
    pub removed: usize,
    /// The heading's content before the edit.
    pub before: Value,
}

/// The template's schema, loosened for hand-written items: evidence may be
/// empty, and items may carry `entered_by`.
pub fn editable_schema(template: &LoadedTemplate) -> Value {
    let mut schema = template.schema.clone();
    if let Some(defs) = schema.get_mut("$defs") {
        if let Some(evidence) = defs.get_mut("evidence").and_then(Value::as_object_mut) {
            evidence.remove("minItems");
        }
        for pointer in ["/statement/properties", "/medication/properties", "/field/anyOf/1/properties"] {
            if let Some(props) = defs.pointer_mut(pointer).and_then(Value::as_object_mut) {
                props.insert("entered_by".into(), json!({ "type": "string" }));
            }
        }
    }
    schema
}

pub fn validate_edited(template: &LoadedTemplate, note: &Value) -> Result<(), EngineError> {
    let schema = editable_schema(template);
    let validator = jsonschema::validator_for(&schema).map_err(|e| EngineError::Template(e.to_string()))?;
    let errors: Vec<String> =
        validator.iter_errors(note).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    if errors.is_empty() { Ok(()) } else { Err(EngineError::SchemaViolation(errors.join("; "))) }
}

fn is_item(v: &Value) -> bool {
    v.as_object().is_some_and(|m| m.contains_key("evidence"))
}

/// Count the items (objects with evidence) under a value.
fn items(v: &Value) -> Vec<&Value> {
    match v {
        _ if is_item(v) => vec![v],
        Value::Object(m) => m.values().flat_map(items).collect(),
        Value::Array(a) => a.iter().flat_map(items).collect(),
        _ => Vec::new(),
    }
}

/// Mark every item in `new` that isn't unchanged from `old`.
fn mark(old: &Value, new: &mut Value, by: &str, path: &str, changed: &mut Vec<String>) {
    if is_item(new) {
        if old != new {
            new["entered_by"] = Value::String(by.into());
            changed.push(path.to_string());
        }
        return;
    }
    match new {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                mark(old.get(k).unwrap_or(&Value::Null), v, by, &format!("{path}/{k}"), changed);
            }
        }
        Value::Array(list) => {
            let before = old.as_array().map(Vec::as_slice).unwrap_or_default();
            for (i, item) in list.iter_mut().enumerate() {
                // Unchanged items may have moved; compare against all of them.
                if !before.contains(item) && is_item(item) {
                    item["entered_by"] = Value::String(by.into());
                    changed.push(format!("{path}/{i}"));
                }
            }
        }
        _ => {}
    }
}

/// Replace one heading of `note` with the vet's version.
pub fn apply_edit(
    template: &LoadedTemplate,
    transcript: &Transcript,
    note: &Value,
    heading: &str,
    value: Value,
    by: &str,
    model: &str,
) -> Result<NoteResult, EngineError> {
    let by = by.trim();
    if by.is_empty() {
        return Err(EngineError::Template("Say who made the edit.".into()));
    }
    if template.heading_title(heading).is_none() {
        return Err(EngineError::Template(format!("{} has no heading '{heading}'.", template.template.name)));
    }
    let (sec, part) = split_heading(heading);
    let before = match part {
        Some(key) => note[sec][key].clone(),
        None => note[sec].clone(),
    };
    let mut value = value;
    let mut changed = Vec::new();
    mark(&before, &mut value, by, &format!("/{heading}"), &mut changed);
    let kept = items(&value).len() - changed.len();
    let removed = items(&before).len().saturating_sub(kept);
    let mut note = note.clone();
    match part {
        Some(key) => note[sec][key] = value,
        None => note[sec] = value,
    }
    validate_edited(template, &note)?;
    let faithfulness = faithfulness::check(&note, transcript);
    let markdown = render::markdown(template, &note, transcript, &faithfulness);
    Ok(NoteResult {
        status: "draft",
        template: template.label(),
        model: model.to_string(),
        note,
        faithfulness,
        stats: Default::default(),
        refinement: None,
        edit: Some(Edit {
            heading: heading.to_string(),
            by: by.to_string(),
            from_run: None,
            changed,
            removed,
            before,
        }),
        markdown,
    })
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
                seg(0, "We gave 10 mL of lactated ringers under the skin."),
                seg(1, "Recheck on Sunday."),
            ],
        }
    }

    fn note() -> Value {
        let mut n = render::tests::empty_note();
        n["plan"]["medications"] = json!([
            {"drug": "LRS", "dose": "10 mL", "route": "SC", "frequency": "once", "duration": null, "evidence": [0]}
        ]);
        n["plan"]["follow_up"] = json!([{"text": "Recheck on Sunday", "evidence": [1]}]);
        n
    }

    #[test]
    fn edited_items_are_the_vets_and_unchanged_items_keep_their_checks() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        // The vet corrects the volume (ASR heard 10 for 100) and adds a line.
        let meds = json!([
            {"drug": "LRS", "dose": "100 mL", "route": "SC", "frequency": "once", "duration": null, "evidence": [0]}
        ]);
        let r =
            apply_edit(&soap, &transcript(), &note(), "plan/medications", meds, "Dr Patel", "gemma").unwrap();
        assert_eq!(r.note["plan"]["medications"][0]["entered_by"], "Dr Patel");
        assert!(r.faithfulness.passed, "a vet's correction must not block signing: {:?}", r.faithfulness);
        assert_eq!(r.faithfulness.vet_entered, vec!["/plan/medications/0"]);
        let edit = r.edit.as_ref().unwrap();
        assert_eq!(edit.changed, vec!["/plan/medications/0"]);
        assert_eq!(edit.removed, 1, "the 10 mL line was replaced");
        assert_eq!(edit.before[0]["dose"], "10 mL");
        assert!(r.markdown.contains("- LRS: 100 mL, SC, once [entered by Dr Patel]"), "{}", r.markdown);

        // A new hand-written line needs no evidence; reordering doesn't mark the old one.
        let follow = json!([
            {"text": "Call with blood results tomorrow", "evidence": []},
            {"text": "Recheck on Sunday", "evidence": [1]}
        ]);
        let r2 =
            apply_edit(&soap, &transcript(), &r.note, "plan/follow_up", follow, "Dr Patel", "gemma").unwrap();
        assert_eq!(r2.edit.as_ref().unwrap().changed, vec!["/plan/follow_up/0"]);
        assert!(r2.note["plan"]["follow_up"][1].get("entered_by").is_none());
        assert_eq!(r2.note["plan"]["medications"][0]["entered_by"], "Dr Patel", "earlier edits carry over");
        assert_eq!(r2.faithfulness.vet_entered.len(), 2);
    }

    #[test]
    fn edits_must_fit_the_heading() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let err =
            apply_edit(&soap, &transcript(), &note(), "plan/follow_up", json!("free text"), "Dr P", "m");
        assert!(matches!(err, Err(EngineError::SchemaViolation(_))));
        assert!(apply_edit(&soap, &transcript(), &note(), "plan/nope", json!([]), "Dr P", "m").is_err());
        assert!(apply_edit(&soap, &transcript(), &note(), "plan/follow_up", json!([]), " ", "m").is_err());
        // The model's schema still demands evidence; the editable one doesn't.
        assert_eq!(soap.schema["$defs"]["evidence"]["minItems"], 1);
    }
}
