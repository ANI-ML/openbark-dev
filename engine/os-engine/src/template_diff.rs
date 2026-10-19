//! What changed between two templates (ADR-0024 section 9): a structured
//! comparison, not a text diff. Headings are matched by key, so a renamed
//! heading reads as renamed, not as one removed and another added.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::template::{FieldDef, PartKind, Template};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HeadingChange {
    Added,
    Removed,
    Renamed {
        from: String,
        to: String,
    },
    /// Its place among the headings both templates share.
    Moved {
        from: usize,
        to: usize,
    },
    /// What goes under it: "statements", "fields", "medications", "text" or "subheadings".
    KindChanged {
        from: String,
        to: String,
    },
    InstructionChanged {
        from: Option<String>,
        to: Option<String>,
    },
    MeasurementsChanged {
        added: Vec<String>,
        removed: Vec<String>,
        renamed: Vec<(String, String)>,
    },
    /// Which species it's shown for (empty: every species).
    ShowChanged {
        from: Vec<String>,
        to: Vec<String>,
    },
    ExamplesChanged {
        added: Vec<String>,
        removed: Vec<String>,
    },
}

/// The changes to one heading ("plan" or "plan/medications").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeadingDiff {
    pub path: String,
    /// The title in the second template, or the first when it was removed.
    pub title: String,
    pub changes: Vec<HeadingChange>,
}

/// A change to the whole note: name, description, icon, level of detail,
/// who it's for, instructions, rules or fixed rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteChange {
    pub field: String,
    pub from: Value,
    pub to: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TemplateDiff {
    pub headings: Vec<HeadingDiff>,
    pub note: Vec<NoteChange>,
}

impl TemplateDiff {
    pub fn is_empty(&self) -> bool {
        self.headings.is_empty() && self.note.is_empty()
    }
}

fn kind_name(kind: Option<PartKind>) -> String {
    match kind {
        None => "subheadings".into(),
        Some(k) => {
            serde_json::to_value(k).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
        }
    }
}

/// One heading, as compared.
struct Node<'a> {
    title: &'a str,
    kind: Option<PartKind>,
    instruction: &'a Option<String>,
    fields: &'a [FieldDef],
    species: &'a [String],
    examples: &'a [String],
}

fn compare_node(a: &Node, b: &Node) -> Vec<HeadingChange> {
    let mut out = Vec::new();
    if a.title != b.title {
        out.push(HeadingChange::Renamed { from: a.title.into(), to: b.title.into() });
    }
    if a.kind != b.kind {
        out.push(HeadingChange::KindChanged { from: kind_name(a.kind), to: kind_name(b.kind) });
    }
    if a.instruction != b.instruction {
        out.push(HeadingChange::InstructionChanged {
            from: a.instruction.clone(),
            to: b.instruction.clone(),
        });
    }
    if a.kind == Some(PartKind::Fields) || b.kind == Some(PartKind::Fields) {
        let before: HashMap<&str, &str> =
            a.fields.iter().map(|f| (f.key.as_str(), f.label.as_str())).collect();
        let after: HashMap<&str, &str> =
            b.fields.iter().map(|f| (f.key.as_str(), f.label.as_str())).collect();
        let added: Vec<String> = b
            .fields
            .iter()
            .filter(|f| !before.contains_key(f.key.as_str()))
            .map(|f| f.label.clone())
            .collect();
        let removed: Vec<String> = a
            .fields
            .iter()
            .filter(|f| !after.contains_key(f.key.as_str()))
            .map(|f| f.label.clone())
            .collect();
        let renamed: Vec<(String, String)> = a
            .fields
            .iter()
            .filter_map(|f| {
                after.get(f.key.as_str()).filter(|l| **l != f.label).map(|l| (f.label.clone(), l.to_string()))
            })
            .collect();
        if !(added.is_empty() && removed.is_empty() && renamed.is_empty()) {
            out.push(HeadingChange::MeasurementsChanged { added, removed, renamed });
        }
    }
    if a.species != b.species {
        out.push(HeadingChange::ShowChanged { from: a.species.to_vec(), to: b.species.to_vec() });
    }
    if a.examples != b.examples {
        let added: Vec<String> = b.examples.iter().filter(|e| !a.examples.contains(e)).cloned().collect();
        let removed: Vec<String> = a.examples.iter().filter(|e| !b.examples.contains(e)).cloned().collect();
        out.push(HeadingChange::ExamplesChanged { added, removed });
    }
    out
}

/// The keys in `b`'s order whose place changed among the keys both share:
/// the fewest moves (those outside a longest run kept in order).
fn moved<'a>(a: &[&'a str], b: &[&'a str]) -> HashMap<&'a str, (usize, usize)> {
    let common_a: Vec<&str> = a.iter().copied().filter(|k| b.contains(k)).collect();
    let common_b: Vec<&str> = b.iter().copied().filter(|k| a.contains(k)).collect();
    let pos_a: HashMap<&str, usize> = common_a.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    // Longest increasing subsequence of a-positions, in b's order.
    let seq: Vec<usize> = common_b.iter().map(|k| pos_a[k]).collect();
    let n = seq.len();
    let mut len = vec![1usize; n];
    let mut prev = vec![usize::MAX; n];
    for i in 0..n {
        for j in 0..i {
            if seq[j] < seq[i] && len[j] + 1 > len[i] {
                len[i] = len[j] + 1;
                prev[i] = j;
            }
        }
    }
    let mut keep = vec![false; n];
    if let Some(mut i) = (0..n).max_by_key(|&i| (len[i], std::cmp::Reverse(i))) {
        loop {
            keep[i] = true;
            if prev[i] == usize::MAX {
                break;
            }
            i = prev[i];
        }
    }
    common_b.iter().enumerate().filter(|(i, _)| !keep[*i]).map(|(i, k)| (*k, (pos_a[k], i))).collect()
}

/// What changed from `a` to `b`.
pub fn template_diff(a: &Template, b: &Template) -> TemplateDiff {
    let mut diff = TemplateDiff::default();
    let note = |field: &str, from: Value, to: Value, out: &mut Vec<NoteChange>| {
        if from != to {
            out.push(NoteChange { field: field.into(), from, to });
        }
    };
    note("name", json!(a.name), json!(b.name), &mut diff.note);
    note("description", json!(a.description), json!(b.description), &mut diff.note);
    note("icon", json!(a.icon), json!(b.icon), &mut diff.note);
    note("detail", json!(a.detail), json!(b.detail), &mut diff.note);
    note("document", json!(a.document), json!(b.document), &mut diff.note);
    note("instructions", json!(a.instructions.trim()), json!(b.instructions.trim()), &mut diff.note);
    note("rules", json!(a.rules), json!(b.rules), &mut diff.note);
    note("fixed_rules", json!(a.fixed_rules), json!(b.fixed_rules), &mut diff.note);

    let ids_a: Vec<&str> = a.sections.iter().map(|s| s.id.as_str()).collect();
    let ids_b: Vec<&str> = b.sections.iter().map(|s| s.id.as_str()).collect();
    let moves = moved(&ids_a, &ids_b);
    for sb in &b.sections {
        let Some(sa) = a.sections.iter().find(|s| s.id == sb.id) else {
            diff.headings.push(HeadingDiff {
                path: sb.id.clone(),
                title: sb.title.clone(),
                changes: vec![HeadingChange::Added],
            });
            continue;
        };
        let mut changes = compare_node(
            &Node {
                title: &sa.title,
                kind: sa.kind,
                instruction: &sa.instruction,
                fields: &sa.fields,
                species: &sa.when.species,
                examples: &sa.examples,
            },
            &Node {
                title: &sb.title,
                kind: sb.kind,
                instruction: &sb.instruction,
                fields: &sb.fields,
                species: &sb.when.species,
                examples: &sb.examples,
            },
        );
        if let Some((from, to)) = moves.get(sb.id.as_str()) {
            changes.insert(0, HeadingChange::Moved { from: *from, to: *to });
        }
        if !changes.is_empty() {
            diff.headings.push(HeadingDiff { path: sb.id.clone(), title: sb.title.clone(), changes });
        }
        // Subheadings, matched by key within the section.
        let keys_a: Vec<&str> = sa.parts.iter().map(|p| p.key.as_str()).collect();
        let keys_b: Vec<&str> = sb.parts.iter().map(|p| p.key.as_str()).collect();
        let part_moves = moved(&keys_a, &keys_b);
        for pb in &sb.parts {
            let path = format!("{}/{}", sb.id, pb.key);
            let Some(pa) = sa.parts.iter().find(|p| p.key == pb.key) else {
                diff.headings.push(HeadingDiff {
                    path,
                    title: pb.label.clone(),
                    changes: vec![HeadingChange::Added],
                });
                continue;
            };
            let mut changes = compare_node(
                &Node {
                    title: &pa.label,
                    kind: Some(pa.kind),
                    instruction: &pa.instruction,
                    fields: &pa.fields,
                    species: &pa.when.species,
                    examples: &pa.examples,
                },
                &Node {
                    title: &pb.label,
                    kind: Some(pb.kind),
                    instruction: &pb.instruction,
                    fields: &pb.fields,
                    species: &pb.when.species,
                    examples: &pb.examples,
                },
            );
            if let Some((from, to)) = part_moves.get(pb.key.as_str()) {
                changes.insert(0, HeadingChange::Moved { from: *from, to: *to });
            }
            if !changes.is_empty() {
                diff.headings.push(HeadingDiff { path, title: pb.label.clone(), changes });
            }
        }
        for pa in sa.parts.iter().filter(|p| !keys_b.contains(&p.key.as_str())) {
            diff.headings.push(HeadingDiff {
                path: format!("{}/{}", sa.id, pa.key),
                title: pa.label.clone(),
                changes: vec![HeadingChange::Removed],
            });
        }
    }
    for sa in a.sections.iter().filter(|s| !ids_b.contains(&s.id.as_str())) {
        diff.headings.push(HeadingDiff {
            path: sa.id.clone(),
            title: sa.title.clone(),
            changes: vec![HeadingChange::Removed],
        });
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::LoadedTemplate;

    fn soap() -> Template {
        LoadedTemplate::builtin("soap").unwrap().template
    }

    fn normalized(t: Template) -> Template {
        LoadedTemplate::new(t).unwrap().template
    }

    #[test]
    fn identical_templates_have_no_differences() {
        assert!(template_diff(&soap(), &soap()).is_empty());
    }

    #[test]
    fn a_renamed_heading_is_renamed_not_removed_and_added() {
        let mut b = soap();
        b.sections.iter_mut().find(|s| s.id == "plan").unwrap().title = "Treatment plan".into();
        let d = template_diff(&soap(), &b);
        assert_eq!(d.headings.len(), 1);
        assert_eq!(d.headings[0].path, "plan");
        assert_eq!(
            d.headings[0].changes,
            vec![HeadingChange::Renamed { from: "Plan".into(), to: "Treatment plan".into() }]
        );
    }

    #[test]
    fn a_release_adding_a_heading_lists_exactly_one_added() {
        let mut b = soap();
        let mut pain: crate::template::Section =
            serde_yaml_ng::from_str("title: Pain score\nkind: statements\n").unwrap();
        pain.id = String::new();
        b.sections.insert(2, pain);
        let b = normalized(b);
        let d = template_diff(&soap(), &b);
        assert_eq!(
            d.headings,
            vec![HeadingDiff {
                path: "pain_score".into(),
                title: "Pain score".into(),
                changes: vec![HeadingChange::Added]
            }]
        );
        assert!(d.note.is_empty());
    }

    /// Golden: a template pair with one change of every kind.
    #[test]
    fn every_kind_of_change_matches_golden_json() {
        let a = normalized(
            serde_yaml_ng::from_str(
                r#"
id: x
name: Dental
icon: tooth
rules: [Chart by quadrant.]
sections:
  - { title: Anaesthesia, kind: medications }
  - title: Findings
    parts:
      - { title: Oral exam, kind: statements, instruction: By quadrant. }
      - { title: Charting, kind: fields, fields: [{ label: Missing teeth }, { label: Calculus }] }
  - { title: Follow-up, kind: statements, examples: [Recheck in 2 weeks] }
  - { title: Home care, kind: statements }
"#,
            )
            .unwrap(),
        );
        let b = normalized(
            serde_yaml_ng::from_str(
                r#"
id: x
name: Dental procedure
icon: tooth
detail: concise
rules: [Chart by quadrant., Name every tooth.]
sections:
  - { title: Findings, parts: [{ key: charting, title: Charting, kind: fields, fields: [{ key: missing_teeth, label: Teeth missing }, { label: Pocket depth }] }, { key: oral_exam, title: Oral exam, kind: text, instruction: By tooth. }] }
  - { id: anaesthesia, title: Anaesthesia, kind: medications, when: { species: [cat] } }
  - { id: follow_up, title: Follow-up, kind: statements, examples: [Recheck in 10 days] }
"#,
            )
            .unwrap(),
        );
        let got = serde_json::to_value(template_diff(&a, &b)).unwrap();
        let want = serde_json::json!({
            "headings": [
                { "path": "findings/charting", "title": "Charting", "changes": [
                    { "kind": "measurements_changed", "added": ["Pocket depth"], "removed": ["Calculus"], "renamed": [["Missing teeth", "Teeth missing"]] }
                ] },
                { "path": "findings/oral_exam", "title": "Oral exam", "changes": [
                    { "kind": "moved", "from": 0, "to": 1 },
                    { "kind": "kind_changed", "from": "statements", "to": "text" },
                    { "kind": "instruction_changed", "from": "By quadrant.", "to": "By tooth." }
                ] },
                { "path": "anaesthesia", "title": "Anaesthesia", "changes": [
                    { "kind": "moved", "from": 0, "to": 1 },
                    { "kind": "show_changed", "from": [], "to": ["feline"] }
                ] },
                { "path": "follow_up", "title": "Follow-up", "changes": [
                    { "kind": "examples_changed", "added": ["Recheck in 10 days"], "removed": ["Recheck in 2 weeks"] }
                ] },
                { "path": "home_care", "title": "Home care", "changes": [{ "kind": "removed" }] }
            ],
            "note": [
                { "field": "name", "from": "Dental", "to": "Dental procedure" },
                { "field": "detail", "from": "standard", "to": "concise" },
                { "field": "rules", "from": ["Chart by quadrant."], "to": ["Chart by quadrant.", "Name every tooth."] }
            ]
        });
        assert_eq!(got, want, "{}", serde_json::to_string_pretty(&got).unwrap());
    }

    /// Golden: the built-in SOAP note against a version with fixed rules and
    /// the whole-note instructions changed.
    #[test]
    fn whole_note_changes_match_golden_json() {
        let mut b = soap();
        b.instructions = "Write a short SOAP note.".into();
        b.fixed_rules.push("Never name a drug that wasn't said.".into());
        let got = serde_json::to_value(template_diff(&soap(), &b)).unwrap();
        assert_eq!(got["headings"], serde_json::json!([]));
        let fields: Vec<&str> =
            got["note"].as_array().unwrap().iter().map(|c| c["field"].as_str().unwrap()).collect();
        assert_eq!(fields, ["instructions", "fixed_rules"]);
        assert_eq!(got["note"][0]["to"], "Write a short SOAP note.");
        assert_eq!(got["note"][1]["to"].as_array().unwrap().len(), 2);
    }
}
