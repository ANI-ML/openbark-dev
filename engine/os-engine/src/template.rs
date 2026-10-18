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
    /// A text box: connected sentences shown as a paragraph. Each sentence
    /// still cites its evidence, so the evidence link works inside it.
    Text,
}

/// How much the model writes (the team's preference for this template).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// The fewest words that keep every clinical fact.
    Concise,
    #[default]
    Standard,
    /// Every relevant detail that was said, with its context.
    Thorough,
}

impl Detail {
    pub fn is_standard(&self) -> bool {
        *self == Detail::Standard
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldDef {
    /// JSON key; derived from the label when empty.
    #[serde(default)]
    pub key: String,
    pub label: String,
}

/// Who a template's output is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Document {
    /// The medical record: signed by a veterinarian, then locked.
    #[default]
    Record,
    /// A document for the pet owner (discharge instructions, a summary). It can
    /// be drafted after the record is signed, and follows the vet's note.
    Client,
}

impl Document {
    pub fn is_record(&self) -> bool {
        *self == Document::Record
    }
}

/// Species OpenBark recognises: (key, plain name, words that mean it). The
/// keys are what templates store (`when: { species: [feline] }`).
pub const SPECIES: &[(&str, &str, &[&str])] = &[
    ("canine", "dogs", &["canine", "dog", "dogs", "puppy", "pup", "k9"]),
    ("feline", "cats", &["feline", "cat", "cats", "kitten"]),
    ("equine", "horses", &["equine", "horse", "horses", "pony", "foal", "donkey", "mule"]),
    ("bovine", "cattle", &["bovine", "cow", "cows", "cattle", "calf", "heifer", "bull", "steer"]),
    (
        "small_ruminant",
        "sheep and goats",
        &[
            "small ruminant",
            "sheep",
            "ewe",
            "lamb",
            "ram",
            "goat",
            "kid",
            "ovine",
            "caprine",
            "camelid",
            "alpaca",
            "llama",
        ],
    ),
    ("porcine", "pigs", &["porcine", "pig", "pigs", "swine", "sow", "piglet", "boar"]),
    (
        "avian",
        "birds",
        &[
            "avian",
            "bird",
            "birds",
            "parrot",
            "budgie",
            "cockatiel",
            "chicken",
            "hen",
            "duck",
            "pigeon",
            "raptor",
        ],
    ),
    (
        "reptile",
        "reptiles",
        &[
            "reptile",
            "lizard",
            "snake",
            "python",
            "gecko",
            "tortoise",
            "turtle",
            "bearded dragon",
            "iguana",
            "chameleon",
        ],
    ),
    ("rabbit", "rabbits", &["rabbit", "bunny", "lagomorph"]),
    (
        "rodent",
        "small rodents",
        &["rodent", "guinea pig", "hamster", "rat", "mouse", "gerbil", "chinchilla", "degu"],
    ),
    ("ferret", "ferrets", &["ferret"]),
];

/// The species key for what someone typed or chose ("Cat", "domestic shorthair cat", "feline").
pub fn species_key(text: &str) -> Option<&'static str> {
    let lower = text.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    let padded = format!(
        " {} ",
        lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
    );
    if let Some((key, _, _)) = SPECIES.iter().find(|(key, _, _)| *key == lower) {
        return Some(key);
    }
    // The longest phrase wins: "guinea pig" is a rodent, not a pig.
    SPECIES
        .iter()
        .flat_map(|(key, _, words)| words.iter().map(move |w| (*key, *w)))
        .filter(|(_, w)| padded.contains(&format!(" {w} ")))
        .max_by_key(|(_, w)| w.len())
        .map(|(key, _)| key)
}

/// When a heading applies (M2 item 4). Empty: always.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Condition {
    /// Shown only for these species (keys of [`SPECIES`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub species: Vec<String>,
}

impl Condition {
    pub fn is_empty(&self) -> bool {
        self.species.is_empty()
    }

    /// Does the heading apply to an animal of this species key?
    pub fn allows(&self, species: &str) -> bool {
        self.species.is_empty() || self.species.iter().any(|s| s == species)
    }
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
    /// Example requests a vet might make under this heading, shown as
    /// one-click suggestions when re-prompting it. Never sent to the model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    /// When the heading applies ("show when species is feline"); always when empty.
    #[serde(default, skip_serializing_if = "Condition::is_empty")]
    pub when: Condition,
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
    /// Example requests for this heading (see [`Part::examples`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    /// When the heading applies ("show when species is feline"); always when empty.
    #[serde(default, skip_serializing_if = "Condition::is_empty")]
    pub when: Condition,
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
    /// The medical record (default) or a document for the client.
    #[serde(default, skip_serializing_if = "Document::is_record")]
    pub document: Document,
    /// Icon shown for this kind of visit ("stethoscope", "phone"; a Phosphor name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// How much the model writes (concise, standard, thorough).
    #[serde(default, skip_serializing_if = "Detail::is_standard")]
    pub detail: Detail,
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

const BUILTIN: &[(&str, &str)] = &[
    ("sick-visit", include_str!("../../../packages/templates/sick-visit/template.yaml")),
    ("recheck", include_str!("../../../packages/templates/recheck/template.yaml")),
    ("wellness", include_str!("../../../packages/templates/wellness/template.yaml")),
    ("callback", include_str!("../../../packages/templates/callback/template.yaml")),
    ("results", include_str!("../../../packages/templates/results/template.yaml")),
    ("rounds", include_str!("../../../packages/templates/rounds/template.yaml")),
    ("soap", include_str!("../../../packages/templates/soap/template.yaml")),
    ("emergency", include_str!("../../../packages/templates/emergency/template.yaml")),
    ("dental", include_str!("../../../packages/templates/dental/template.yaml")),
    ("surgery", include_str!("../../../packages/templates/surgery/template.yaml")),
    ("dermatology", include_str!("../../../packages/templates/dermatology/template.yaml")),
    ("oncology", include_str!("../../../packages/templates/oncology/template.yaml")),
    ("exotics", include_str!("../../../packages/templates/exotics/template.yaml")),
    ("equine", include_str!("../../../packages/templates/equine/template.yaml")),
    ("production-animal", include_str!("../../../packages/templates/production-animal/template.yaml")),
    ("euthanasia", include_str!("../../../packages/templates/euthanasia/template.yaml")),
    ("discharge", include_str!("../../../packages/templates/discharge/template.yaml")),
    ("client-summary", include_str!("../../../packages/templates/client-summary/template.yaml")),
    ("estimate-explanation", include_str!("../../../packages/templates/estimate-explanation/template.yaml")),
    ("euthanasia-aftercare", include_str!("../../../packages/templates/euthanasia-aftercare/template.yaml")),
];

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

    /// The template as drafted for one animal: headings shown only for other
    /// species are removed, so neither the prompt nor the schema asks for
    /// them (M2 item 4). With no species, or one OpenBark doesn't recognise,
    /// every heading stays.
    pub fn for_species(&self, species: Option<&str>) -> Self {
        let Some(key) = species.and_then(species_key) else { return self.clone() };
        if self.template.schema.is_some() {
            return self.clone(); // a hand-written schema can't be trimmed
        }
        let mut t = self.template.clone();
        let before: usize = t.sections.iter().map(|s| 1 + s.parts.len()).sum();
        t.sections.retain_mut(|s| {
            s.parts.retain(|p| p.when.allows(key));
            s.when.allows(key) && (s.kind.is_some() || !s.parts.is_empty())
        });
        let after: usize = t.sections.iter().map(|s| 1 + s.parts.len()).sum();
        if after == before || t.sections.is_empty() {
            return self.clone();
        }
        Self::new(t).unwrap_or_else(|_| self.clone())
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

/// At most a few short examples per heading.
const MAX_EXAMPLES: usize = 8;

fn normalize_examples(examples: &mut Vec<String>, heading: &str) -> Result<(), EngineError> {
    for e in examples.iter_mut() {
        *e = e.trim().to_string();
    }
    examples.retain(|e| !e.is_empty());
    if examples.len() > MAX_EXAMPLES {
        return Err(bad(format!("\"{heading}\" has more than {MAX_EXAMPLES} examples.")));
    }
    for e in examples.iter() {
        check_text(&format!("An example for \"{heading}\""), e)?;
    }
    Ok(())
}

fn normalize_condition(when: &mut Condition, heading: &str) -> Result<(), EngineError> {
    let mut keys: Vec<String> = Vec::new();
    for s in &when.species {
        let key = species_key(s).ok_or_else(|| {
            let known: Vec<&str> = SPECIES.iter().map(|(_, name, _)| *name).collect();
            bad(format!(
                "\"{heading}\" is shown for '{s}', which isn't a species OpenBark knows ({}).",
                known.join(", ")
            ))
        })?;
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
        }
    }
    when.species = keys;
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
    t.icon = t.icon.trim().to_lowercase();
    if t.icon.len() > 32 || !t.icon.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(bad(format!("'{}' can't be an icon name (use a-z, 0-9 and -).", t.icon)));
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
        normalize_examples(&mut s.examples, &s.title)?;
        normalize_condition(&mut s.when, &s.title)?;
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
            normalize_examples(&mut p.examples, &p.label)?;
            normalize_condition(&mut p.when, &p.label)?;
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
        // A text box holds the same cited sentences, shown as a paragraph.
        PartKind::Statements | PartKind::Text => json!({ "$ref": "#/$defs/statements" }),
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
            PartKind::Statements | PartKind::Medications | PartKind::Text => json!([]),
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

    #[test]
    fn every_builtin_loads_and_its_empty_note_validates() {
        for id in LoadedTemplate::builtin_ids() {
            let t = LoadedTemplate::builtin(id).unwrap();
            assert_eq!(t.template.id, id);
            let note = empty_note(&t.template);
            assert!(jsonschema::validator_for(&t.schema).unwrap().is_valid(&note), "{id}");
        }
        assert_eq!(LoadedTemplate::builtin("discharge").unwrap().template.document, Document::Client);
        assert_eq!(LoadedTemplate::builtin("client-summary").unwrap().template.document, Document::Client);
        assert_eq!(LoadedTemplate::builtin("callback").unwrap().template.document, Document::Record);
    }

    #[test]
    fn species_words_are_recognised() {
        assert_eq!(species_key("Cat"), Some("feline"));
        assert_eq!(species_key("domestic shorthair cat"), Some("feline"));
        assert_eq!(species_key("Bearded dragon"), Some("reptile"));
        assert_eq!(species_key("guinea pig"), Some("rodent"));
        assert_eq!(species_key("small_ruminant"), Some("small_ruminant"));
        assert_eq!(species_key("catfish"), None);
        assert_eq!(species_key(""), None);
    }

    #[test]
    fn headings_for_other_species_are_left_out_of_prompt_and_schema() {
        let yaml = r#"
name: Species test
sections:
  - title: History
    kind: statements
  - title: Retrovirus status
    kind: statements
    when: { species: [cat] }
  - title: Heartworm test
    kind: statements
    when: { species: [canine] }
  - title: Plan
    parts:
      - { label: Medications, kind: medications }
      - { label: Hoof care, kind: statements, when: { species: [horse] } }
"#;
        let t = LoadedTemplate::from_yaml(yaml).unwrap();
        // Words are stored as keys.
        assert_eq!(t.template.sections[1].when.species, ["feline"]);
        let cat = t.for_species(Some("Cat"));
        let ids: Vec<&str> = cat.template.sections.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["history", "retrovirus_status", "plan"]);
        assert!(cat.schema["properties"].get("heartworm_test").is_none());
        assert!(cat.schema["properties"]["plan"]["properties"].get("hoof_care").is_none());
        assert!(!crate::note::system_prompt(&cat).contains("Heartworm"));
        // Unknown or missing species: everything stays.
        assert_eq!(t.for_species(None).template.sections.len(), 4);
        assert_eq!(t.for_species(Some("axolotl")).template.sections.len(), 4);
        // Unknown species in a template is refused.
        let err = LoadedTemplate::from_yaml(&yaml.replace("[cat]", "[unicorn]")).unwrap_err();
        assert!(err.to_string().contains("unicorn"), "{err}");
    }

    #[test]
    fn heading_examples_load_but_stay_out_of_the_prompt() {
        let t = LoadedTemplate::builtin("dental").unwrap();
        let teeth = t.template.sections.iter().find(|s| s.id == "findings_by_tooth").unwrap();
        assert!(teeth.examples.iter().any(|e| e.contains("Triadan")));
        let prompt = crate::note::system_prompt(&t);
        assert!(!prompt.contains("Use the Triadan number for every tooth"));
        assert!(!t.schema_text.contains("Use the Triadan number"));
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
