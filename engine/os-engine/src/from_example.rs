//! A template proposed from a note the clinic already writes (M2 item 5).
//!
//! The model reads the example and lists its headings, what goes under each
//! and a short instruction, constrained to a small schema, so the result is
//! always a template the builder can open. Nothing from the example is kept
//! but its structure: the prompt forbids copying patient details, and the
//! example itself is never saved.

use os_llm::{ChatMessage, GenerateRequest, LlmBackend};
use serde_json::{Value, json};

use crate::EngineError;
use crate::template::{LoadedTemplate, Template};

/// Longest example accepted (characters): a long note, well inside the context.
pub const MAX_EXAMPLE: usize = 20_000;

const SYSTEM: &str = "You turn an example veterinary note into a reusable note template.\n\
Read the example and list its headings in the order they appear, keeping the clinic's own heading names \
(\"Subjective\", \"Hx\", \"Dental chart\"; expand only unclear abbreviations).\n\
For each heading choose what goes under it:\n\
- \"statements\": findings, history, plans or free text, one fact per line;\n\
- \"fields\": named measurements such as Weight, Temperature or HR; list the field names;\n\
- \"medications\": drugs with dose, route, frequency and duration;\n\
- \"parts\": the heading has its own subheadings; list them, each with its own kind.\n\
Write a short instruction for each heading saying in general terms what belongs there \
(\"Findings by body system as stated\"), only when the heading name alone isn't clear.\n\
Never copy anything about this patient into the template: no names, values, dates, findings, drugs or doses.\n\
Name the template after the kind of note (\"Dental procedure note\").\n\
Audience: \"veterinary record\" for clinical notes written by and for the veterinary team (SOAP, procedure and \
surgical reports, rechecks). Choose \"pet owner\" only when the note speaks to the owner in plain language \
(\"you\", discharge instructions, letters).";

/// The shape the model fills.
fn draft_schema() -> Value {
    let kind = |with_parts: bool| {
        let mut kinds = vec!["statements", "fields", "medications"];
        if with_parts {
            kinds.push("parts");
        }
        json!({ "type": "string", "enum": kinds })
    };
    let fields = json!({ "type": "array", "items": { "type": "string" }, "maxItems": 30 });
    let part = json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "kind": kind(false),
            "instruction": { "type": "string" },
            "fields": fields
        },
        "required": ["title", "kind"],
        "additionalProperties": false
    });
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "description": { "type": "string" },
            "audience": { "type": "string", "enum": ["veterinary record", "pet owner"] },
            "sections": {
                "type": "array",
                "minItems": 1,
                "maxItems": 30,
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "kind": kind(true),
                        "instruction": { "type": "string" },
                        "fields": fields,
                        "parts": { "type": "array", "items": part, "maxItems": 20 }
                    },
                    "required": ["title", "kind"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["name", "audience", "sections"],
        "additionalProperties": false
    })
}

/// Turn the model's draft into a template, mending what would fail validation
/// (measurements with no names become bullet points, empty subheadings too).
pub fn template_from_draft(draft: &Value) -> Result<LoadedTemplate, EngineError> {
    let text = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let names = |v: &Value| -> Vec<Value> {
        v.as_array().into_iter().flatten().filter_map(text).map(|label| json!({ "label": label })).collect()
    };
    let content = |kind: &str, fields: &[Value]| match kind {
        "fields" if !fields.is_empty() => "fields",
        "medications" => "medications",
        _ => "statements",
    };
    let mut sections = Vec::new();
    for s in draft["sections"].as_array().into_iter().flatten() {
        let Some(title) = text(&s["title"]) else { continue };
        let kind = s["kind"].as_str().unwrap_or("statements");
        let mut section = json!({ "title": title });
        if let Some(i) = text(&s["instruction"]) {
            section["instruction"] = json!(i);
        }
        let parts: Vec<Value> = s["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let label = text(&p["title"])?;
                let fields = names(&p["fields"]);
                let mut part =
                    json!({ "label": label, "kind": content(p["kind"].as_str().unwrap_or(""), &fields) });
                if !fields.is_empty() && part["kind"] == "fields" {
                    part["fields"] = json!(fields);
                }
                if let Some(i) = text(&p["instruction"]) {
                    part["instruction"] = json!(i);
                }
                Some(part)
            })
            .collect();
        if kind == "parts" && !parts.is_empty() {
            section["parts"] = json!(parts);
        } else {
            let fields = names(&s["fields"]);
            section["kind"] = json!(content(kind, &fields));
            if section["kind"] == "fields" {
                section["fields"] = json!(fields);
            }
        }
        sections.push(section);
    }
    if sections.is_empty() {
        return Err(EngineError::Template("No headings were found in the example note.".into()));
    }
    let name = text(&draft["name"]).unwrap_or_else(|| "Template from an example".into());
    let mut template = json!({ "name": name, "sections": sections });
    if let Some(d) = text(&draft["description"]) {
        template["description"] = json!(d);
    }
    if draft["audience"] == "pet owner" {
        template["document"] = json!("client");
    }
    let template: Template =
        serde_json::from_value(template).map_err(|e| EngineError::Template(e.to_string()))?;
    LoadedTemplate::new(template)
}

/// Propose a template from an example note. The result isn't saved.
pub fn template_from_example(llm: &dyn LlmBackend, example: &str) -> Result<LoadedTemplate, EngineError> {
    let example = example.trim();
    if example.chars().count() < 20 {
        return Err(EngineError::Template("Paste a whole example note, with its headings.".into()));
    }
    if example.chars().count() > MAX_EXAMPLE {
        return Err(EngineError::Template(format!(
            "The example is longer than {MAX_EXAMPLE} characters. Paste one note."
        )));
    }
    let request = GenerateRequest {
        messages: vec![
            ChatMessage::system(SYSTEM.to_string()),
            ChatMessage::user(format!("Example note:\n\n{example}")),
        ],
        max_tokens: 3000,
        temperature: 0.0,
        seed: 42,
        json_schema: Some(draft_schema().to_string()),
    };
    let output = llm.generate(&request, &mut |_| true)?;
    let draft: Value = serde_json::from_str(output.text.trim()).map_err(|e| {
        let hint = if output.stats.hit_max_tokens { " (the example may be too long)" } else { "" };
        EngineError::BadJson(format!("{e}{hint}"))
    })?;
    template_from_draft(&draft)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::PartKind;

    #[test]
    fn a_draft_becomes_a_valid_template_and_mends_gaps() {
        let draft = json!({
            "name": "Dental procedure note",
            "audience": "veterinary record",
            "sections": [
                { "title": "Indication", "kind": "statements" },
                { "title": "Anaesthesia", "kind": "medications", "instruction": "Drugs and blocks as said." },
                { "title": "Monitoring", "kind": "fields", "fields": ["HR", "SpO2", "  "] },
                { "title": "Chart", "kind": "fields", "fields": [] },
                { "title": "Plan", "kind": "parts", "parts": [
                    { "title": "Home care", "kind": "statements" },
                    { "title": "Recheck", "kind": "fields" }
                ] },
                { "title": "Empty parts", "kind": "parts", "parts": [] },
                { "title": "  ", "kind": "statements" }
            ]
        });
        let t = template_from_draft(&draft).unwrap();
        let titles: Vec<&str> = t.template.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Indication", "Anaesthesia", "Monitoring", "Chart", "Plan", "Empty parts"]);
        let s = &t.template.sections;
        assert_eq!(s[1].kind, Some(PartKind::Medications));
        assert_eq!(s[2].fields.len(), 2, "blank field names dropped");
        assert_eq!(s[3].kind, Some(PartKind::Statements), "measurements without names become bullet points");
        assert_eq!(s[4].parts[1].kind, PartKind::Statements);
        assert_eq!(s[5].kind, Some(PartKind::Statements));
        assert!(t.template.document.is_record());
    }

    #[test]
    fn a_client_draft_is_a_client_document() {
        let draft = json!({ "name": "Discharge", "audience": "pet owner", "sections": [{ "title": "Home care", "kind": "statements" }] });
        assert!(!template_from_draft(&draft).unwrap().template.document.is_record());
        let empty = json!({ "name": "x", "audience": "veterinary record", "sections": [] });
        assert!(template_from_draft(&empty).is_err());
    }
}
