//! Normal-exam defaults (ADR-0032).
//!
//! A bullet-point heading of a medical record may list normal findings by
//! system ("Eyes: Normal, no discharge"), used only for a system that wasn't
//! discussed. The model still follows the rule that nothing is normal unless
//! it was said: the default text never reaches it. The prompt names the
//! systems at most, and the drafting grammar has the model tag each statement
//! under the heading with the system it is about ([`tag_systems`]). After
//! drafting, code decides ([`undiscussed`]): a system with any finding loses
//! its default, whole, and a finding is never merged into one; a system with
//! none gets its default as a separate line, marked as the template's, with
//! no evidence. When in doubt, the default is left out.
//!
//! Those lines are kept beside the note, never in it (`template_defaults` in
//! `note.json`). Older versions, the faithfulness pass, the eval, Copy, the
//! PDF and client documents read only the note, so they never take one for a
//! finding. A veterinarian confirms each line, which makes it their own line
//! in the note, or removes it ([`settle`]); the engine won't sign a note while
//! one is waiting ([`crate::session::Session::sign_latest_by`]).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::EngineError;
use crate::note::NoteResult;
use crate::template::{DefaultLine, LoadedTemplate, headings_with_defaults, split_heading};
use crate::transcript::Transcript;

/// The key the model tags a statement with under a heading with defaults.
pub const SYSTEM_KEY: &str = "system";

/// A normal finding from the template, waiting for a veterinarian to confirm
/// or remove it. It is not from the recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDefault {
    /// The heading path: "physical_exam" or "exam/systems".
    pub heading: String,
    pub label: String,
    pub text: String,
}

impl PendingDefault {
    /// The line as it reads once confirmed: "Eyes: Normal, no discharge".
    pub fn line(&self) -> String {
        format!("{}: {}", self.label, self.text)
    }
}

/// The defaults waiting in a saved run (`template_defaults` in `note.json`).
pub fn pending_in(saved: &Value) -> Vec<PendingDefault> {
    saved.get("template_defaults").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// Every default of a template, as if no system had been discussed.
pub fn all(template: &LoadedTemplate) -> Vec<PendingDefault> {
    let mut out = Vec::new();
    for (heading, lines) in headings_with_defaults(&template.template) {
        for l in lines {
            let (label, text) = (l.label.clone(), l.text.clone());
            out.push(PendingDefault { heading: heading.clone(), label, text });
        }
    }
    out
}

/// The prompt's words for a heading with defaults: its systems, and how to
/// tag statements. Never the default text.
pub fn systems_note(template: &LoadedTemplate, path: &str) -> Option<String> {
    let (_, lines) = headings_with_defaults(&template.template).into_iter().find(|(p, _)| p == path)?;
    Some(format!(
        " Systems to report on, only if they were discussed: {}. Give each statement the system it is about in \"system\" (null if none of these).",
        labels(lines).join(", ")
    ))
}

/// The labels of a heading's defaults, each once, in order.
fn labels(lines: &[DefaultLine]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for l in lines {
        if !out.contains(&l.label.as_str()) {
            out.push(&l.label);
        }
    }
    out
}

/// A list of statements whose items name their system.
fn tagged_statements(lines: &[DefaultLine]) -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "system": { "anyOf": [{ "type": "null" }, { "type": "string", "enum": labels(lines) }] },
                "evidence": { "$ref": "#/$defs/evidence" }
            },
            "required": ["text", "system", "evidence"],
            "additionalProperties": false
        }
    })
}

/// Is this schema node a list of plain statements?
fn is_statements(node: &Value) -> bool {
    node.get("$ref").and_then(Value::as_str) == Some("#/$defs/statements")
        || node.pointer("/items/$ref").and_then(Value::as_str) == Some("#/$defs/statement")
}

/// Give every statement under a heading with defaults a `system`, in a
/// drafting schema: the template's (`only` None), or one heading's
/// ([`LoadedTemplate::heading_schema`], `only` its path). The template's own
/// schema is never changed, so saved notes never hold a tag.
pub fn tag_systems(template: &LoadedTemplate, schema: &mut Value, only: Option<&str>) {
    for (path, lines) in headings_with_defaults(&template.template) {
        let (sec, part) = split_heading(&path);
        let node = match (only, part) {
            (None, None) => schema.pointer_mut(&format!("/properties/{sec}")),
            (None, Some(key)) => schema.pointer_mut(&format!("/properties/{sec}/properties/{key}")),
            (Some(o), _) if o == path => Some(&mut *schema),
            (Some(o), Some(key)) if o == sec => schema.pointer_mut(&format!("/properties/{key}")),
            _ => None,
        };
        // A hand-written schema may hold something else here.
        let Some(node) = node.filter(|n| is_statements(n)) else { continue };
        let mut tagged = tagged_statements(lines);
        if let Some(defs) = node.get("$defs") {
            tagged["$defs"] = defs.clone();
        }
        *node = tagged;
    }
}

/// Lower case, with the accents of Western European languages folded
/// ("Système" and "systeme" match).
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'ß' => out.push_str("ss"),
            c => out.push(c),
        }
    }
    out
}

fn words(text: &str) -> impl Iterator<Item = String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(fold)
}

/// Does `word` (from what was said or written) name the label's `part`?
/// Long words match on their start, so "oreille" and "oreilles", or
/// "abdomen" and "abdominal", match; short words match whole, without a
/// plural "s" or "x".
fn names(part: &str, word: &str) -> bool {
    let n = part.chars().count();
    if n >= 5 {
        let keep = (n - 2).max(5);
        let stem: String = part.chars().take(keep).collect();
        word.starts_with(&stem)
    } else {
        let bare = |w: &str| w.trim_end_matches(['s', 'x']).to_string();
        let (p, w) = (bare(part), bare(word));
        if p.is_empty() { part == word } else { p == w }
    }
}

/// Is the label mentioned in these words? Any word of four letters or more
/// in it counts ("Nœuds lymphatiques": "lymphatique"); a label with none
/// counts by all its words ("T", "FC").
fn mentioned(label: &str, said: &HashSet<String>) -> bool {
    let parts: Vec<String> = words(label).collect();
    let long: Vec<&String> = parts.iter().filter(|p| p.chars().count() >= 4).collect();
    let check: Vec<&String> = if long.is_empty() { parts.iter().collect() } else { long };
    !check.is_empty() && check.iter().any(|p| said.iter().any(|w| names(p, w)))
}

/// Every text of the note's items: statements, values, drugs.
fn note_words(v: &Value, out: &mut HashSet<String>) {
    match v {
        Value::String(s) => out.extend(words(s)),
        Value::Array(a) => a.iter().for_each(|x| note_words(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                if k != "evidence" && k != SYSTEM_KEY {
                    note_words(x, out);
                }
            }
        }
        _ => {}
    }
}

/// After drafting or redrafting: take the model's `system` tags out of
/// `note`, and keep only the `candidates` that nothing was said about. A
/// default is left out when a statement under its heading is tagged with its
/// system, when anything in the note mentions its system, or (given the
/// transcript) when the system was named in the recording. The text of a
/// finding is never merged into a default: the finding replaces it.
pub fn undiscussed(
    template: &LoadedTemplate,
    note: &mut Value,
    transcript: Option<&Transcript>,
    candidates: Vec<PendingDefault>,
) -> Vec<PendingDefault> {
    let headings = headings_with_defaults(&template.template);
    let mut tagged: Vec<(String, String)> = Vec::new();
    for (path, _) in &headings {
        let (sec, part) = split_heading(path);
        let node = match part {
            None => note.get_mut(sec),
            Some(key) => note.get_mut(sec).and_then(|s| s.get_mut(key)),
        };
        for item in node.and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(Value::String(system)) = item.as_object_mut().and_then(|m| m.remove(SYSTEM_KEY)) {
                tagged.push((path.clone(), fold(system.trim())));
            }
        }
    }
    let mut said = HashSet::new();
    note_words(note, &mut said);
    for s in transcript.map(|t| t.segments.as_slice()).unwrap_or_default() {
        said.extend(words(&s.text));
    }
    candidates
        .into_iter()
        .filter(|c| {
            let lines = headings.iter().find(|(p, _)| *p == c.heading).map(|(_, l)| *l).unwrap_or_default();
            let in_template = lines.iter().any(|l| l.label == c.label);
            let label = fold(c.label.trim());
            let tagged_here = tagged.iter().any(|(h, s)| *h == c.heading && *s == label);
            in_template && !tagged_here && !mentioned(&c.label, &said)
        })
        .collect()
}

/// What a veterinarian does with defaults waiting under one heading.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettleRequest {
    /// The heading path ("physical_exam").
    pub heading: String,
    /// Which systems; every one waiting under the heading when empty.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Confirm (true) or remove (false).
    pub confirm: bool,
    /// The vet's own wording of one confirmed finding (the label stays).
    #[serde(default)]
    pub text: Option<String>,
}

/// Confirm or remove defaults waiting under one heading, as `by`. A confirmed
/// line becomes the vet's own line in the note, through a hand edit
/// ([`crate::edit::apply_edit`]: marked `entered_by`, no evidence, listed by
/// the faithfulness pass instead of checked). A removed line is dropped. The
/// result is a new draft that carries the defaults still waiting.
pub fn settle(
    template: &LoadedTemplate,
    transcript: &Transcript,
    note: &Value,
    pending: &[PendingDefault],
    request: &SettleRequest,
    by: &str,
    model: &str,
) -> Result<NoteResult, EngineError> {
    let chosen: Vec<&PendingDefault> = pending
        .iter()
        .filter(|p| p.heading == request.heading)
        .filter(|p| request.labels.is_empty() || request.labels.contains(&p.label))
        .collect();
    if chosen.is_empty() {
        return Err(EngineError::Session(
            "There are no normal findings from the template waiting under this heading.".into(),
        ));
    }
    let text = request.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if text.is_some() && chosen.len() != 1 {
        return Err(EngineError::Session("Change the wording of one normal finding at a time.".into()));
    }
    let (sec, part) = split_heading(&request.heading);
    let before = match part {
        Some(key) => note[sec][key].clone(),
        None => note[sec].clone(),
    };
    let mut items = before.as_array().cloned().unwrap_or_default();
    if request.confirm {
        for p in &chosen {
            let line = match text {
                Some(t) => format!("{}: {t}", p.label),
                None => p.line(),
            };
            items.push(json!({ "text": line, "evidence": [] }));
        }
    }
    let value = Value::Array(items);
    let mut result = crate::edit::apply_edit(template, transcript, note, &request.heading, value, by, model)?;
    let labels: Vec<String> = chosen.iter().map(|p| p.label.clone()).collect();
    if let Some(edit) = result.edit.as_mut() {
        if request.confirm {
            edit.defaults_confirmed = labels;
        } else {
            edit.defaults_removed = labels;
        }
    }
    result.template_defaults = pending.iter().filter(|p| !chosen.contains(p)).cloned().collect();
    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::transcript::Segment;

    pub(crate) const EXAM: &str = r#"
name: Examen
language: fr
sections:
  - title: Motif
    kind: statements
  - title: Examen physique
    kind: statements
    instruction: List the findings the veterinarian states, by system.
    defaults:
      - { label: Yeux, text: "Normal, aucun écoulement" }
      - { label: Oreilles, text: "Propres, pas d'odeur" }
      - { label: Cœur, text: "Rythme régulier, pas de souffle" }
      - { label: Tartre, text: "Absent" }
"#;

    fn template() -> LoadedTemplate {
        LoadedTemplate::from_yaml(EXAM).unwrap()
    }

    fn transcript(lines: &[&str]) -> Transcript {
        Transcript {
            segments: lines
                .iter()
                .enumerate()
                .map(|(i, text)| Segment {
                    id: i as u32,
                    start: i as f32,
                    end: i as f32 + 1.0,
                    speaker: Some(0),
                    text: text.to_string(),
                    hints: Vec::new(),
                    check: None,
                })
                .collect(),
        }
    }

    fn labels_of(p: &[PendingDefault]) -> Vec<&str> {
        p.iter().map(|d| d.label.as_str()).collect()
    }

    #[test]
    fn a_discussed_system_drops_its_default_and_an_undiscussed_one_adds_it() {
        let t = template();
        let mut note = json!({
            "motif": [{"text": "Écoulement oculaire depuis trois jours", "evidence": [0]}],
            "examen_physique": [
                {"text": "Écoulement séreux à l'œil gauche", "system": "Yeux", "evidence": [1]}
            ]
        });
        let said = transcript(&["Un écoulement à l'œil depuis trois jours", "Écoulement séreux à gauche"]);
        let pending = undiscussed(&t, &mut note, Some(&said), all(&t));
        assert_eq!(labels_of(&pending), ["Oreilles", "Cœur", "Tartre"]);
        assert!(pending.iter().all(|p| p.heading == "examen_physique"));
        assert_eq!(pending[0].line(), "Oreilles: Propres, pas d'odeur");
        // The tag is gone from the note, and the finding is as written.
        assert!(note["examen_physique"][0].get(SYSTEM_KEY).is_none());
        assert_eq!(note["examen_physique"][0]["text"], "Écoulement séreux à l'œil gauche");
        assert_eq!(note["examen_physique"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_finding_is_never_merged_into_a_default() {
        let t = template();
        let mut note = json!({
            "motif": [],
            "examen_physique": [{"text": "Souffle 3/6", "system": "Cœur", "evidence": [0]}]
        });
        let pending = undiscussed(&t, &mut note, None, all(&t));
        assert!(!labels_of(&pending).contains(&"Cœur"));
        let text = note.to_string();
        assert!(!text.contains("Rythme régulier"), "the default isn't in the note: {text}");
        assert!(text.contains("Souffle 3/6"));
        assert_eq!(note["examen_physique"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn when_unsure_the_default_is_left_out() {
        let t = template();
        // No tag (an untagged backend), but the system is named in the note.
        let mut note = json!({
            "motif": [],
            "examen_physique": [{"text": "Oreilles : cérumen abondant", "system": null, "evidence": [0]}]
        });
        let pending = undiscussed(&t, &mut note, None, all(&t));
        assert!(!labels_of(&pending).contains(&"Oreilles"));
        // Named in the recording but left out of the draft: still no default.
        let mut note = json!({ "motif": [], "examen_physique": [] });
        let said = transcript(&["On regarde les yeux, il y a un problème"]);
        let pending = undiscussed(&t, &mut note, Some(&said), all(&t));
        assert!(!labels_of(&pending).contains(&"Yeux"), "{pending:?}");
        assert_eq!(labels_of(&pending), ["Oreilles", "Cœur", "Tartre"]);
        // Accents and plurals: "coeur" names Cœur.
        let said = transcript(&["Le coeur est bien"]);
        let pending = undiscussed(&t, &mut json!({}), Some(&said), all(&t));
        assert!(!labels_of(&pending).contains(&"Cœur"));
    }

    #[test]
    fn the_drafting_schema_tags_systems_and_the_saved_schema_does_not() {
        let t = template();
        assert!(!t.schema_text.contains("\"system\""), "the template's own schema is untouched");
        let mut schema = t.schema.clone();
        tag_systems(&t, &mut schema, None);
        let items = &schema["properties"]["examen_physique"]["items"];
        assert_eq!(items["required"], json!(["text", "system", "evidence"]));
        assert_eq!(
            items["properties"]["system"]["anyOf"][1]["enum"],
            json!(["Yeux", "Oreilles", "Cœur", "Tartre"])
        );
        assert_eq!(schema["properties"]["motif"], json!({ "$ref": "#/$defs/statements" }));
        // No default text in the schema.
        assert!(!schema.to_string().contains("aucun écoulement"));
        // One heading's schema, as re-prompting uses it.
        let mut heading = t.heading_schema("examen_physique").unwrap();
        tag_systems(&t, &mut heading, Some("examen_physique"));
        assert!(heading["items"]["properties"]["system"].is_object());
        assert!(heading["$defs"]["evidence"].is_object());
        let mut other = t.heading_schema("motif").unwrap();
        tag_systems(&t, &mut other, Some("motif"));
        assert!(other["items"].get("properties").is_none());
    }

    #[test]
    fn confirming_makes_the_line_the_vets_own_and_removing_drops_it() {
        let t = template();
        let said = transcript(&["Bonjour"]);
        let note = json!({ "motif": [{"text": "Contrôle", "evidence": [0]}], "examen_physique": [] });
        let pending = all(&t);
        let confirm = SettleRequest {
            heading: "examen_physique".into(),
            labels: vec!["Yeux".into()],
            confirm: true,
            text: None,
        };
        let r = settle(&t, &said, &note, &pending, &confirm, "Dr Martin", "test").unwrap();
        let exam = r.note["examen_physique"].as_array().unwrap();
        assert_eq!(exam.len(), 1);
        assert_eq!(exam[0]["text"], "Yeux: Normal, aucun écoulement");
        assert_eq!(exam[0]["entered_by"], "Dr Martin");
        assert_eq!(exam[0]["evidence"], json!([]));
        assert_eq!(labels_of(&r.template_defaults), ["Oreilles", "Cœur", "Tartre"]);
        assert_eq!(r.edit.as_ref().unwrap().defaults_confirmed, ["Yeux"]);
        assert!(r.faithfulness.vet_entered.contains(&"/examen_physique/0".to_string()));
        assert!(r.faithfulness.passed);
        // The vet's own wording for one line.
        let reworded = SettleRequest { labels: vec!["Cœur".into()], text: Some("RAS".into()), ..confirm };
        let r2 = settle(&t, &said, &r.note, &r.template_defaults, &reworded, "Dr Martin", "test").unwrap();
        assert_eq!(r2.note["examen_physique"][1]["text"], "Cœur: RAS");
        // Remove the rest: nothing is added to the note.
        let remove = SettleRequest { heading: "examen_physique".into(), ..Default::default() };
        let r3 = settle(&t, &said, &r2.note, &r2.template_defaults, &remove, "Dr Martin", "test").unwrap();
        assert!(r3.template_defaults.is_empty());
        assert_eq!(r3.note["examen_physique"].as_array().unwrap().len(), 2);
        assert_eq!(r3.edit.as_ref().unwrap().defaults_removed, ["Oreilles", "Tartre"]);
        // Nothing left to settle.
        assert!(settle(&t, &said, &r3.note, &[], &remove, "Dr Martin", "test").is_err());
    }
}
