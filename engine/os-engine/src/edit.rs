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
/// empty, items may carry `entered_by`, and a tooth chart has no item limit
/// (the limit is the drafting budget, not a limit on the record).
pub fn editable_schema(template: &LoadedTemplate) -> Value {
    let mut schema = template.schema.clone();
    fn unlimit_charts(v: &mut Value) {
        match v {
            Value::Object(m) => {
                if m.get("items").and_then(|i| i.get("$ref")).and_then(Value::as_str)
                    == Some("#/$defs/tooth_finding")
                {
                    m.remove("maxItems");
                }
                m.values_mut().for_each(unlimit_charts);
            }
            Value::Array(a) => a.iter_mut().for_each(unlimit_charts),
            _ => {}
        }
    }
    unlimit_charts(&mut schema);
    if let Some(defs) = schema.get_mut("$defs") {
        if let Some(evidence) = defs.get_mut("evidence").and_then(Value::as_object_mut) {
            evidence.remove("minItems");
        }
        for pointer in [
            "/statement/properties",
            "/medication/properties",
            "/field/anyOf/1/properties",
            "/tooth_finding/properties",
        ] {
            if let Some(props) = defs.pointer_mut(pointer).and_then(Value::as_object_mut) {
                props.insert("entered_by".into(), json!({ "type": "string" }));
            }
        }
    }
    schema
}

/// Where an edited note doesn't fit the editable schema: JSON pointers.
fn misfit_paths(template: &LoadedTemplate, note: &Value) -> Vec<String> {
    let schema = editable_schema(template);
    let Ok(validator) = jsonschema::validator_for(&schema) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for e in validator.iter_errors(note) {
        let path = e.instance_path().to_string();
        let path = if path.is_empty() { "/".to_string() } else { path };
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
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

/// A hand edit of a tooth chart (ADR-0021 decision 8): every changed or
/// added item must fit its finding and use a tooth of the dentition, or the
/// edit is refused with a plain message. The model's own items, unchanged,
/// are never refused here; the faithfulness pass flags them instead. The
/// saved array is ordered by tooth, then by the finding table's order.
fn check_chart_edit(
    before: &Value,
    value: Value,
    dentition: Option<crate::dental::Dentition>,
) -> Result<Value, EngineError> {
    let Value::Array(mut items) = value else {
        return Err(EngineError::SchemaViolation("A tooth chart is a list of findings.".into()));
    };
    let old = before.as_array().map(Vec::as_slice).unwrap_or_default();
    for item in items.iter().filter(|i| !old.contains(i)) {
        let tooth = item["tooth"].as_str().unwrap_or_default();
        if let Some(misfit) = crate::dental::tooth_misfit(tooth, dentition) {
            return Err(EngineError::Template(misfit.to_string()));
        }
        crate::dental::fits(item).map_err(|m| EngineError::Template(format!("{tooth}: {m}")))?;
    }
    let order = |item: &Value| {
        let finding = item["finding"].as_str().unwrap_or_default();
        let rank = crate::dental::chart().findings.iter().position(|f| f.id == finding).unwrap_or(usize::MAX);
        (crate::dental::triadan_order(item["tooth"].as_str().unwrap_or_default()), rank)
    };
    items.sort_by_key(order);
    Ok(Value::Array(items))
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
    if let Some((chart, dentition)) = template.chart()
        && chart == heading
    {
        value = check_chart_edit(&before, value, dentition)?;
    }
    let mut changed = Vec::new();
    mark(&before, &mut value, by, &format!("/{heading}"), &mut changed);
    let kept = items(&value).len() - changed.len();
    let removed = items(&before).len().saturating_sub(kept);
    let mut note = note.clone();
    match part {
        Some(key) => note[sec][key] = value,
        None => note[sec] = value,
    }
    // A hand edit that doesn't fit is the vet's to correct, in plain words
    // (never a schema dump that blames the model).
    match validate_edited(template, &note) {
        Ok(()) => {}
        Err(EngineError::SchemaViolation(_)) => {
            let paths = misfit_paths(template, &note);
            return Err(EngineError::Template(format!(
                "This edit doesn't fit the template at {}, so it wasn't saved. Check those items and save again.",
                if paths.is_empty() { format!("/{heading}") } else { paths.join(", ") }
            )));
        }
        Err(e) => return Err(e),
    }
    let faithfulness = faithfulness::check_template(&note, transcript, template);
    let markdown = render::markdown(template, &note, transcript, &faithfulness);
    Ok(NoteResult {
        status: "draft",
        template: template.label(),
        model: model.to_string(),
        note,
        faithfulness,
        stats: Default::default(),
        refinement: None,
        by: None,
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
        // Plain words naming where, never a schema dump that blames the model.
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("doesn't fit the template at /plan/follow_up"), "{msg}");
        assert!(!msg.contains("model output"), "{msg}");
        assert!(apply_edit(&soap, &transcript(), &note(), "plan/nope", json!([]), "Dr P", "m").is_err());
        assert!(apply_edit(&soap, &transcript(), &note(), "plan/follow_up", json!([]), " ", "m").is_err());
        // The model's schema still demands evidence; the editable one doesn't.
        assert_eq!(soap.schema["$defs"]["evidence"]["minItems"], 1);
    }

    fn dental_note(t: &LoadedTemplate) -> Value {
        let mut n = crate::template::empty_note(&t.template);
        n["dental_chart"] = json!([
            {"tooth": "309", "text": "309 fractured", "finding": "fracture", "detail": null, "grade": null,
             "mm": null, "planned": false, "evidence": [0]},
            {"tooth": "104", "text": "104 normal", "finding": "normal", "detail": null, "grade": null,
             "mm": null, "planned": false, "evidence": [1]}
        ]);
        n
    }

    fn dental_transcript() -> Transcript {
        let seg = |id, text: &str| crate::transcript::Segment {
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
                seg(0, "Three oh nine is fractured."),
                seg(1, "The upper right canine is normal."),
            ],
        }
    }

    #[test]
    fn chart_edits_are_the_vets_own_and_must_fit_the_finding_table() {
        let dental = LoadedTemplate::builtin("dental").unwrap().for_species(Some("cat"));
        let note = dental_note(&dental);
        let old = note["dental_chart"].as_array().unwrap().clone();
        // Change 309's fracture type, keep 104, add 209 missing.
        let mut changed = old[0].clone();
        changed["detail"] = json!("complicated_crown");
        let added = json!({"tooth": "209", "text": "Missing", "finding": "missing", "detail": null,
            "grade": null, "mm": null, "planned": false, "evidence": []});
        let value = json!([added, old[1].clone(), changed]);
        let r =
            apply_edit(&dental, &dental_transcript(), &note, "dental_chart", value, "Dr Patel", "m").unwrap();
        let chart = r.note["dental_chart"].as_array().unwrap();
        // Ordered by tooth.
        let teeth: Vec<&str> = chart.iter().map(|i| i["tooth"].as_str().unwrap()).collect();
        assert_eq!(teeth, ["104", "209", "309"]);
        assert!(chart[0].get("entered_by").is_none(), "unchanged items keep their link");
        assert_eq!(chart[1]["entered_by"], "Dr Patel");
        assert_eq!(chart[2]["entered_by"], "Dr Patel");
        assert!(r.faithfulness.passed, "{:?}", r.faithfulness);
        assert_eq!(r.edit.as_ref().unwrap().removed, 1, "the old 309 item was replaced");

        // A tooth a cat doesn't have, and a grade outside the table, are refused.
        let bad_tooth = json!([{"tooth": "105", "text": "Missing", "finding": "missing", "detail": null,
            "grade": null, "mm": null, "planned": false, "evidence": []}]);
        let err = apply_edit(&dental, &dental_transcript(), &note, "dental_chart", bad_tooth, "Dr P", "m");
        assert!(err.unwrap_err().to_string().contains("105 isn't a cat's tooth"));
        let bad_grade = json!([{"tooth": "309", "text": "Mobility 4", "finding": "mobility", "detail": null,
            "grade": 4, "mm": null, "planned": false, "evidence": []}]);
        let err = apply_edit(&dental, &dental_transcript(), &note, "dental_chart", bad_grade, "Dr P", "m");
        assert!(err.unwrap_err().to_string().contains("from 0 to 3"));
        // A persistent deciduous tooth on a permanent number is refused.
        let wrong_kind = json!([{"tooth": "104", "text": "Persistent deciduous", "finding": "persistent_deciduous",
            "detail": null, "grade": null, "mm": null, "planned": false, "evidence": []}]);
        let err = apply_edit(&dental, &dental_transcript(), &note, "dental_chart", wrong_kind, "Dr P", "m");
        assert!(err.unwrap_err().to_string().contains("only for deciduous teeth"));
    }

    /// The drafting budget (96 findings) is not a limit on the vet's record: a
    /// full mouth charted by hand saves.
    #[test]
    fn a_full_mouth_charted_by_hand_has_no_item_limit() {
        let dental = LoadedTemplate::builtin("dental").unwrap().for_species(Some("dog"));
        let note = dental_note(&dental);
        let mut items = Vec::new();
        for id in
            crate::dental::ids(Some(crate::dental::Dentition::Canine)).into_iter().filter(|id| id < &"500")
        {
            for (finding, grade, mm) in [
                ("periodontal_stage", json!(1), Value::Null),
                ("probing_depth", Value::Null, json!(2)),
                ("gingival_recession", Value::Null, json!(1)),
            ] {
                items.push(
                    json!({"tooth": id, "text": "", "finding": finding, "detail": null, "grade": grade,
                    "mm": mm, "planned": false, "evidence": []}),
                );
            }
        }
        items.push(json!({"tooth": "309", "text": "", "finding": "mobility", "detail": null, "grade": 1,
            "mm": null, "planned": false, "evidence": []}));
        assert!(items.len() > crate::template::MAX_CHART_ITEMS);
        let n = items.len();
        let r = apply_edit(
            &dental,
            &dental_transcript(),
            &note,
            "dental_chart",
            Value::Array(items),
            "Dr P",
            "m",
        )
        .unwrap();
        assert_eq!(r.note["dental_chart"].as_array().unwrap().len(), n);
    }
}
