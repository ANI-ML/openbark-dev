//! Note templates: headings, what goes under each, and instructions.
//!
//! A template is only headings. The JSON Schema that constrains the model is
//! generated from them ([`generate_schema`]), so a veterinary team can write
//! its own template without knowing JSON Schema, and every template gets the
//! same evidence citations, "Not discussed" handling and faithfulness checks.
//!
//! ```yaml
//! id: dental
//! name: Dental procedure note
//! instructions: Write the record of a dental procedure.
//! sections:
//!   - title: Anaesthesia
//!     kind: medications          # bullet points | measurements | medications
//!   - title: Findings
//!     parts:
//!       - { title: Oral exam, kind: statements, instruction: "By quadrant." }
//!       - { title: Charting, kind: fields, fields: [{ label: Missing teeth }] }
//! ```

use std::collections::HashSet;
use std::path::Path;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::EngineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Required,
    Optional,
    #[default]
    IfDiscussed,
}

/// What goes under a heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// Bullet points, one clinical fact each.
    Statements,
    /// Named values with units ("Weight", "Temperature").
    Fields,
    /// Drug, dose, route, frequency, duration.
    Medications,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldDef {
    /// JSON key; derived from the label when empty.
    #[serde(default)]
    pub key: String,
    pub label: String,
}

/// A subheading inside a section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Part {
    #[serde(default)]
    pub key: String,
    #[serde(alias = "title")]
    pub label: String,
    pub kind: PartKind,
    #[serde(default)]
    pub presence: Presence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldDef>,
}

/// A top-level heading: either holds content itself (`kind`) or has parts.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Section {
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub presence: Presence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PartKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
}

/// A request a veterinarian saved while re-prompting a heading, applied to
/// every later draft with this template.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Standard {
    pub id: String,
    /// "plan", "plan/medications", or "" for the whole note.
    pub heading: String,
    pub text: String,
    pub added_at: DateTime<Local>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Template {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_version")]
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Optional hand-written schema file (advanced). Normally generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_instructions")]
    pub instructions: String,
    /// Rules on top of [`CORE_RULES`], which every template gets.
    #[serde(default)]
    pub rules: Vec<String>,
    pub sections: Vec<Section>,
    /// Saved re-prompts (stored beside the template, merged in on load).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standards: Vec<Standard>,
}

fn default_language() -> String {
    "en".into()
}

fn default_version() -> String {
    "1".into()
}

fn default_instructions() -> String {
    "Write the medical record for this veterinary consultation under the headings below.\n\
     Use concise clinical veterinary English as a veterinarian would chart it.\n\
     The transcript is from a recording; speakers are labelled by number, not role. Infer roles from context, \
     but never put words in a speaker's mouth."
        .into()
}

/// Rules every template gets, so a team template can't loosen faithfulness.
pub const CORE_RULES: &[&str] = &[
    "Use only information stated in the transcript. Do not add findings, diagnoses, vitals, doses or plans that were not said.",
    "If something was not discussed, leave the array empty or the field null. Never write \"normal\" or \"within normal limits\" unless that was said.",
    "Copy numbers (vitals, weights, doses, durations) exactly as stated, always with their units (kg, °C, bpm, breaths/min, mg/kg, mL, days).",
    "Patient details (name, species, breed, age, sex) only if someone said them. A pet's name said anywhere counts; never guess the species from the name or the complaint.",
    "Each evidence id must point to the line where that fact was actually said.",
    "Lines may end with speech-recognition hints such as [heard \"marapitin\", likely maropitant]. Use the likely spelling for a drug or term only when it fits what was said; otherwise keep the words as heard. Never add a drug because of a hint alone.",
    "Every statement and field must cite the transcript segment ids it comes from in \"evidence\".",
    "Keep each statement to one clinical fact or action.",
];

/// Limits that keep a template's prompt and grammar a sensible size.
const MAX_SECTIONS: usize = 30;
const MAX_PARTS: usize = 20;
const MAX_FIELDS: usize = 30;
const MAX_HEADINGS: usize = 80;
const MAX_TEXT: usize = 1500;

#[derive(Debug, Clone)]
pub struct LoadedTemplate {
    pub template: Template,
    pub schema: Value,
    pub schema_text: String,
}

const BUILTIN: &[(&str, &str)] = &[("soap", include_str!("../../../packages/templates/soap/template.yaml"))];

impl LoadedTemplate {
    pub fn builtin_ids() -> impl Iterator<Item = &'static str> {
        BUILTIN.iter().map(|(id, _)| *id)
    }

    pub fn is_builtin(id: &str) -> bool {
        BUILTIN.iter().any(|(i, _)| *i == id)
    }

    pub fn builtin(id: &str) -> Option<Self> {
        let (_, yaml) = BUILTIN.iter().find(|(i, _)| *i == id)?;
        Some(Self::from_yaml(yaml).expect("built-in templates are valid"))
    }

    /// Load `<dir>/template.yaml` (and its schema file, if it names one).
    pub fn from_dir(dir: &Path) -> Result<Self, EngineError> {
        let read = |p: &Path| {
            std::fs::read_to_string(p).map_err(|e| EngineError::Template(format!("{}: {e}", p.display())))
        };
        let template = parse_yaml(&read(&dir.join("template.yaml"))?)?;
        match &template.schema {
            Some(file) => {
                let text = read(&dir.join(file))?;
                Self::with_schema(template, &text)
            }
            None => Self::new(template),
        }
    }

    /// A template snapshot as saved with each note run.
    pub fn from_yaml(yaml: &str) -> Result<Self, EngineError> {
        Self::new(parse_yaml(yaml)?)
    }

    /// Normalise and validate a template, and generate its schema.
    pub fn new(mut template: Template) -> Result<Self, EngineError> {
        normalize(&mut template)?;
        let schema = generate_schema(&template);
        let schema_text = serde_json::to_string(&schema).expect("schema serialises");
        Ok(Self { template, schema, schema_text })
    }

    fn with_schema(mut template: Template, schema_text: &str) -> Result<Self, EngineError> {
        normalize(&mut template)?;
        let schema: Value =
            serde_json::from_str(schema_text).map_err(|e| EngineError::Template(e.to_string()))?;
        jsonschema::validator_for(&schema).map_err(|e| EngineError::Template(format!("schema: {e}")))?;
        let loaded = Self { template, schema, schema_text: schema_text.to_string() };
        loaded.check_sections_match_schema()?;
        Ok(loaded)
    }

    /// A built-in id, a template directory, or a team template id.
    pub fn resolve(spec: &str) -> Result<Self, EngineError> {
        if let Some(t) = Self::builtin(spec) {
            return Ok(t);
        }
        let path = Path::new(spec);
        if path.is_dir() {
            return Self::from_dir(path);
        }
        crate::template_store::TemplateStore::default().load(spec)
    }

    /// Every section/part/field the template renders must exist in the schema.
    fn check_sections_match_schema(&self) -> Result<(), EngineError> {
        let props = |v: &Value| v.get("properties").cloned().unwrap_or_default();
        let root = props(&self.schema);
        for section in &self.template.sections {
            let sec = root
                .get(&section.id)
                .ok_or_else(|| EngineError::Template(format!("section '{}' not in schema", section.id)))?;
            for field in &section.fields {
                if props(sec).get(&field.key).is_none() {
                    return Err(EngineError::Template(format!(
                        "field '{}.{}' not in schema",
                        section.id, field.key
                    )));
                }
            }
            for part in &section.parts {
                let p = props(sec).get(&part.key).cloned().ok_or_else(|| {
                    EngineError::Template(format!("part '{}.{}' not in schema", section.id, part.key))
                })?;
                for field in &part.fields {
                    if props(&p).get(&field.key).is_none() {
                        return Err(EngineError::Template(format!(
                            "field '{}.{}.{}' not in schema",
                            section.id, part.key, field.key
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        format!("{}@{}", self.template.id, self.template.version)
    }

    /// The template as YAML, standards included (the run snapshot).
    pub fn to_yaml(&self) -> String {
        serde_yaml_ng::to_string(&self.template).expect("template serialises")
    }

    /// Standards that apply to a heading path ("" = the whole note).
    pub fn standards_for<'a>(&'a self, heading: &'a str) -> impl Iterator<Item = &'a Standard> + 'a {
        self.template.standards.iter().filter(move |s| s.heading == heading)
    }

    /// The title of a heading path: "Plan", "Plan > Medications".
    pub fn heading_title(&self, heading: &str) -> Option<String> {
        let (sec, part) = split_heading(heading);
        let section = self.template.sections.iter().find(|s| s.id == sec)?;
        match part {
            None => Some(section.title.clone()),
            Some(key) => {
                let part = section.parts.iter().find(|p| p.key == key)?;
                Some(format!("{} > {}", section.title, part.label))
            }
        }
    }

    /// The schema of one heading, standing alone (for redrafting just it).
    pub fn heading_schema(&self, heading: &str) -> Option<Value> {
        let (sec, part) = split_heading(heading);
        let mut node = self.schema["properties"].get(sec)?.clone();
        if let Some(key) = part {
            node = node["properties"].get(key)?.clone();
        }
        let defs = self.schema.get("$defs").cloned().unwrap_or_else(|| json!({}));
        // Inline a top-level $ref so the node is a schema in its own right.
        if let Some(name) = node.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/"))
        {
            node = defs.get(name)?.clone();
        }
        node.as_object_mut()?.insert("$defs".into(), defs);
        Some(node)
    }
}

/// "plan/medications" → ("plan", Some("medications")).
pub fn split_heading(heading: &str) -> (&str, Option<&str>) {
    match heading.split_once('/') {
        Some((s, p)) => (s, Some(p)),
        None => (heading, None),
    }
}

fn parse_yaml(yaml: &str) -> Result<Template, EngineError> {
    serde_yaml_ng::from_str(yaml).map_err(|e| EngineError::Template(e.to_string()))
}

/// JSON key from a heading: "Problems / diagnoses" → "problems_diagnoses".
pub fn key_from(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out: String = out.trim_end_matches('_').chars().take(40).collect();
    match out.chars().next() {
        None => "heading".into(),
        Some(c) if c.is_ascii_digit() => format!("h_{out}"),
        Some(_) => out,
    }
}

/// Template id from a name: "Dental procedure" → "dental-procedure".
pub fn id_from(name: &str) -> String {
    key_from(name).replace('_', "-")
}

fn valid_key(key: &str) -> bool {
    key.len() <= 48
        && key.starts_with(|c: char| c.is_ascii_lowercase())
        && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A key unique among `taken`: "history", "history_2", …
fn unique(key: String, taken: &mut HashSet<String>) -> String {
    let mut candidate = key.clone();
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{key}_{n}");
        n += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::Template(msg.into())
}

fn check_text(what: &str, text: &str) -> Result<(), EngineError> {
    if text.chars().count() > MAX_TEXT {
        return Err(bad(format!("{what} is longer than {MAX_TEXT} characters. Shorten it.")));
    }
    Ok(())
}

fn normalize_fields(fields: &mut [FieldDef], heading: &str) -> Result<(), EngineError> {
    if fields.is_empty() {
        return Err(bad(format!("\"{heading}\" holds measurements but lists none. Add at least one.")));
    }
    if fields.len() > MAX_FIELDS {
        return Err(bad(format!("\"{heading}\" has more than {MAX_FIELDS} measurements.")));
    }
    let mut taken = HashSet::new();
    for f in fields.iter_mut() {
        f.label = f.label.trim().to_string();
        if f.label.is_empty() {
            return Err(bad(format!("A measurement under \"{heading}\" has no name.")));
        }
        let key = if f.key.trim().is_empty() { key_from(&f.label) } else { f.key.trim().to_string() };
        if !valid_key(&key) {
            return Err(bad(format!("'{key}' can't be used as a key (use a-z, 0-9 and _).")));
        }
        f.key = unique(key, &mut taken);
    }
    Ok(())
}

/// Fill in keys, trim text, and check the template can be drafted.
fn normalize(t: &mut Template) -> Result<(), EngineError> {
    t.name = t.name.trim().to_string();
    if t.name.is_empty() {
        return Err(bad("Give the template a name."));
    }
    if t.id.trim().is_empty() {
        t.id = id_from(&t.name);
    }
    t.id = t.id.trim().to_string();
    let id_ok = t.id.len() <= 48
        && t.id.starts_with(|c: char| c.is_ascii_lowercase())
        && t.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err(bad(format!("'{}' can't be a template id (use a-z, 0-9 and -).", t.id)));
    }
    check_text("The instructions", &t.instructions)?;
    t.rules.retain(|r| !r.trim().is_empty());
    for r in &t.rules {
        check_text("A rule", r)?;
    }
    if t.sections.is_empty() {
        return Err(bad("Add at least one heading."));
    }
    if t.sections.len() > MAX_SECTIONS {
        return Err(bad(format!("A template can have at most {MAX_SECTIONS} headings.")));
    }
    let mut headings = 0;
    let mut taken = HashSet::new();
    for s in &mut t.sections {
        s.title = s.title.trim().to_string();
        if s.title.is_empty() {
            return Err(bad("A heading has no title."));
        }
        let id = if s.id.trim().is_empty() { key_from(&s.title) } else { s.id.trim().to_string() };
        if !valid_key(&id) {
            return Err(bad(format!("'{id}' can't be used as a key (use a-z, 0-9 and _).")));
        }
        s.id = unique(id, &mut taken);
        s.instruction = s.instruction.take().map(|i| i.trim().to_string()).filter(|i| !i.is_empty());
        if let Some(i) = &s.instruction {
            check_text(&format!("The instruction for \"{}\"", s.title), i)?;
        }
        headings += 1;
        match s.kind {
            Some(kind) => {
                if !s.parts.is_empty() {
                    return Err(bad(format!(
                        "\"{}\" has both its own content and subheadings. Choose one.",
                        s.title
                    )));
                }
                if kind == PartKind::Fields {
                    normalize_fields(&mut s.fields, &s.title)?;
                } else {
                    s.fields.clear();
                }
            }
            None => {
                if s.parts.is_empty() {
                    return Err(bad(format!(
                        "\"{}\" is empty. Choose what goes under it, or add subheadings.",
                        s.title
                    )));
                }
                if s.parts.len() > MAX_PARTS {
                    return Err(bad(format!("\"{}\" has more than {MAX_PARTS} subheadings.", s.title)));
                }
                s.fields.clear();
            }
        }
        let mut part_keys = HashSet::new();
        for p in &mut s.parts {
            p.label = p.label.trim().to_string();
            if p.label.is_empty() {
                return Err(bad(format!("A subheading under \"{}\" has no title.", s.title)));
            }
            let key = if p.key.trim().is_empty() { key_from(&p.label) } else { p.key.trim().to_string() };
            if !valid_key(&key) {
                return Err(bad(format!("'{key}' can't be used as a key (use a-z, 0-9 and _).")));
            }
            p.key = unique(key, &mut part_keys);
            p.instruction = p.instruction.take().map(|i| i.trim().to_string()).filter(|i| !i.is_empty());
            if let Some(i) = &p.instruction {
                check_text(&format!("The instruction for \"{}\"", p.label), i)?;
            }
            if p.kind == PartKind::Fields {
                normalize_fields(&mut p.fields, &p.label)?;
            } else {
                p.fields.clear();
            }
            headings += 1;
        }
    }
    if headings > MAX_HEADINGS {
        return Err(bad(format!("A template can have at most {MAX_HEADINGS} headings and subheadings.")));
    }
    Ok(())
}

fn kind_schema(kind: PartKind, fields: &[FieldDef]) -> Value {
    match kind {
        PartKind::Statements => json!({ "$ref": "#/$defs/statements" }),
        PartKind::Medications => json!({ "type": "array", "items": { "$ref": "#/$defs/medication" } }),
        PartKind::Fields => {
            let props: serde_json::Map<String, Value> =
                fields.iter().map(|f| (f.key.clone(), json!({ "$ref": "#/$defs/field" }))).collect();
            let required: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
            json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
        }
    }
}

/// The JSON Schema the model fills: every heading present, empty when not
/// discussed, and every item citing transcript segment ids.
pub fn generate_schema(t: &Template) -> Value {
    let mut props = serde_json::Map::new();
    for s in &t.sections {
        let node = match s.kind {
            Some(kind) => kind_schema(kind, &s.fields),
            None => {
                let parts: serde_json::Map<String, Value> =
                    s.parts.iter().map(|p| (p.key.clone(), kind_schema(p.kind, &p.fields))).collect();
                let required: Vec<&str> = s.parts.iter().map(|p| p.key.as_str()).collect();
                json!({ "type": "object", "properties": parts, "required": required, "additionalProperties": false })
            }
        };
        props.insert(s.id.clone(), node);
    }
    let required: Vec<&str> = t.sections.iter().map(|s| s.id.as_str()).collect();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": format!("{} (OpenBark template {}@{})", t.name, t.id, t.version),
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
        "$defs": {
            "evidence": { "type": "array", "items": { "type": "integer", "minimum": 0 }, "minItems": 1 },
            "field": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "properties": {
                            "value": { "anyOf": [{ "type": "number" }, { "type": "string" }] },
                            "unit": { "anyOf": [{ "type": "null" }, { "type": "string" }] },
                            "evidence": { "$ref": "#/$defs/evidence" }
                        },
                        "required": ["value", "unit", "evidence"],
                        "additionalProperties": false
                    }
                ]
            },
            "statement": {
                "type": "object",
                "properties": { "text": { "type": "string" }, "evidence": { "$ref": "#/$defs/evidence" } },
                "required": ["text", "evidence"],
                "additionalProperties": false
            },
            "statements": { "type": "array", "items": { "$ref": "#/$defs/statement" } },
            "medication": {
                "type": "object",
                "properties": {
                    "drug": { "type": "string" },
                    "dose": { "anyOf": [{ "type": "null" }, { "type": "string" }] },
                    "route": { "anyOf": [{ "type": "null" }, { "type": "string" }] },
                    "frequency": { "anyOf": [{ "type": "null" }, { "type": "string" }] },
                    "duration": { "anyOf": [{ "type": "null" }, { "type": "string" }] },
                    "evidence": { "$ref": "#/$defs/evidence" }
                },
                "required": ["drug", "dose", "route", "frequency", "duration", "evidence"],
                "additionalProperties": false
            }
        }
    })
}

/// A note with every heading empty: what "nothing discussed" looks like.
pub fn empty_note(t: &Template) -> Value {
    fn empty(kind: PartKind, fields: &[FieldDef]) -> Value {
        match kind {
            PartKind::Statements | PartKind::Medications => json!([]),
            PartKind::Fields => fields.iter().map(|f| (f.key.clone(), Value::Null)).collect(),
        }
    }
    t.sections
        .iter()
        .map(|s| {
            let v = match s.kind {
                Some(kind) => empty(kind, &s.fields),
                None => s.parts.iter().map(|p| (p.key.clone(), empty(p.kind, &p.fields))).collect(),
            };
            (s.id.clone(), v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_soap_loads() {
        let t = LoadedTemplate::builtin("soap").unwrap();
        assert_eq!(t.template.id, "soap");
        assert_eq!(t.label(), "soap@0.1.0");
        assert!(t.template.sections.iter().any(|s| s.id == "plan"));
        assert!(t.template.rules.iter().any(|r| r.contains("Differentials")));
    }

    /// The SOAP schema is generated from its headings; the snapshot in
    /// packages/templates/soap/schema.json must match, so changes to the
    /// generator (which change what the model is asked for) are deliberate.
    #[test]
    fn generated_soap_schema_matches_snapshot() {
        let t = LoadedTemplate::builtin("soap").unwrap();
        let snapshot: Value =
            serde_json::from_str(include_str!("../../../packages/templates/soap/schema.json")).unwrap();
        for key in ["properties", "required", "additionalProperties", "$defs"] {
            assert_eq!(t.schema[key], snapshot[key], "schema '{key}' differs from the snapshot");
        }
    }

    #[test]
    fn keys_come_from_headings() {
        assert_eq!(key_from("Problems / diagnoses"), "problems_diagnoses");
        assert_eq!(key_from("3-month recheck"), "h_3_month_recheck");
        assert_eq!(key_from("  "), "heading");
        assert_eq!(id_from("Dental procedure note"), "dental-procedure-note");
    }

    #[test]
    fn team_template_from_headings_only() {
        let yaml = "name: Dental procedure\nsections:\n  - title: Anaesthesia\n    kind: medications\n  - title: Findings\n    parts:\n      - { title: Oral exam, kind: statements, instruction: By quadrant }\n      - { title: Oral exam, kind: fields, fields: [{ label: Teeth extracted }, { label: Calculus grade }] }\n  - title: Follow-up\n    kind: statements\n";
        let t = LoadedTemplate::from_yaml(yaml).unwrap();
        assert_eq!(t.template.id, "dental-procedure");
        assert_eq!(t.template.version, "1");
        let ids: Vec<&str> = t.template.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["anaesthesia", "findings", "follow_up"]);
        let parts: Vec<&str> = t.template.sections[1].parts.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(parts, ["oral_exam", "oral_exam_2"], "duplicate headings get distinct keys");
        assert_eq!(t.template.sections[1].parts[1].fields[0].key, "teeth_extracted");
        // The empty note validates against the generated schema.
        let note = empty_note(&t.template);
        assert!(jsonschema::validator_for(&t.schema).unwrap().is_valid(&note));
        // A heading's schema stands alone.
        let meds = t.heading_schema("anaesthesia").unwrap();
        assert!(jsonschema::validator_for(&meds).unwrap().is_valid(&json!([])));
        let exam = t.heading_schema("findings/oral_exam").unwrap();
        assert_eq!(exam["type"], "array");
        assert!(exam["$defs"]["statement"].is_object());
        assert_eq!(t.heading_title("findings/oral_exam_2").unwrap(), "Findings > Oral exam");
        assert!(t.heading_schema("nope").is_none());
    }

    #[test]
    fn helpful_errors_for_unfinished_templates() {
        let err = |yaml: &str| LoadedTemplate::from_yaml(yaml).unwrap_err().to_string();
        assert!(err("name: X\nsections: []\n").contains("at least one heading"));
        assert!(err("name: X\nsections:\n  - title: Plan\n").contains("\"Plan\" is empty"));
        assert!(err("name: X\nsections:\n  - { title: Vitals, kind: fields }\n").contains("lists none"));
        assert!(err("name: ''\nsections:\n  - { title: A, kind: statements }\n").contains("name"));
    }

    #[test]
    fn snapshot_round_trips_with_standards() {
        let mut t = LoadedTemplate::builtin("soap").unwrap();
        t.template.standards.push(Standard {
            id: "s1".into(),
            heading: "plan/medications".into(),
            text: "List fluids with rate and volume".into(),
            added_at: Local::now(),
            added_by: Some("Dr Patel".into()),
        });
        let back = LoadedTemplate::from_yaml(&t.to_yaml()).unwrap();
        assert_eq!(back.template.standards, t.template.standards);
        assert_eq!(back.schema, t.schema);
        assert_eq!(back.standards_for("plan/medications").count(), 1);
    }

    #[test]
    fn mismatched_handwritten_schema_is_rejected() {
        let yaml = "id: x\nversion: '1'\nname: X\nschema: s.json\ninstructions: hi\nsections:\n  - {id: nope, title: Nope, kind: statements}\n";
        let schema = r#"{"type":"object","properties":{"yes":{"type":"object"}}}"#;
        let err = LoadedTemplate::with_schema(parse_yaml(yaml).unwrap(), schema).unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn unknown_template_lists_builtins() {
        let tmp = tempfile::tempdir().unwrap();
        let err =
            crate::template_store::TemplateStore::new(tmp.path().into()).load("does-not-exist").unwrap_err();
        assert!(err.to_string().contains("soap"));
    }
}
