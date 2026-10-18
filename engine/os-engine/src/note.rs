//! Template-constrained note generation.

use os_llm::{ChatMessage, GenerateRequest, GenerateStats, LlmBackend};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::faithfulness::{self, FaithfulnessReport};
use crate::render;
use crate::template::{CORE_RULES, FieldDef, LoadedTemplate, PartKind, split_heading};
use crate::transcript::Transcript;
use crate::{EngineError, PRODUCT_NAME};

#[derive(Debug, Clone)]
pub struct NoteOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    pub seed: u32,
    /// Details entered by the clinic (patient name, reason for visit). Given
    /// to the model as context, never as evidence.
    pub context: Option<String>,
}

impl Default for NoteOptions {
    fn default() -> Self {
        // Low temperature: charting should be deterministic and literal.
        Self { max_tokens: 4096, temperature: 0.1, seed: 42, context: None }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteResult {
    /// Always "draft": only a DVM can sign a note, and the engine never does.
    pub status: &'static str,
    pub template: String,
    pub model: String,
    pub note: Value,
    pub faithfulness: FaithfulnessReport,
    pub stats: GenerateStats,
    /// Set when this draft came from re-prompting one heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refinement: Option<Refinement>,
    #[serde(skip)]
    pub markdown: String,
}

/// What a re-prompt asked for, kept with the run it produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Refinement {
    /// Heading path that was redrafted: "plan" or "plan/medications".
    pub heading: String,
    pub request: String,
    /// The run whose note was refined ("runs/002_gemma…").
    #[serde(default)]
    pub from_run: Option<String>,
    /// Whether the request was also saved as a standard for the template.
    #[serde(default)]
    pub saved_as_standard: bool,
}

const JSON_FORMATS: &str = r#"JSON formats:
- a field is null, or {"value": 4.1, "unit": "kg", "evidence": [18]} for a measurement,
  or {"value": "pink but tacky", "unit": null, "evidence": [20]} for a description
- a statement is {"text": "Vomiting once or twice daily for 3 days", "evidence": [2, 4]}
- a medication is {"drug": "Maropitant", "dose": "1 mg/kg", "route": "SC", "frequency": "once", "duration": null, "evidence": [34]}
"#;

fn with_instruction(instruction: Option<&str>) -> String {
    instruction.map(|i| format!(" — {i}")).unwrap_or_default()
}

/// One guidance line per heading: "- plan.follow_up: Follow-up — instruction".
fn guidance_line(
    template: &LoadedTemplate,
    path: &str,
    name: &str,
    label: &str,
    kind: PartKind,
    fields: &[FieldDef],
    instruction: Option<&str>,
) -> String {
    let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
    let mut line = match kind {
        // A section that is only measurements: "- patient: Name, Species (null if not stated)".
        PartKind::Fields if !name.contains('.') => {
            format!("- {name}: {} (null if not stated){}", labels.join(", "), with_instruction(instruction))
        }
        PartKind::Fields => format!(
            "- {name}: {label}{} [{}; null if not stated]",
            with_instruction(instruction),
            labels.join(", ")
        ),
        PartKind::Medications if !label.to_lowercase().contains("medication") => {
            format!("- {name}: {label} (medications){}", with_instruction(instruction))
        }
        _ => format!("- {name}: {label}{}", with_instruction(instruction)),
    };
    for standard in template.standards_for(path) {
        line.push_str(&format!(" Team standard: {}.", standard.text.trim_end_matches('.')));
    }
    line.push('\n');
    line
}

/// "What each part of the JSON should contain", for every heading or for
/// one heading path (a section path includes its subheadings).
fn guidance(template: &LoadedTemplate, only: Option<&str>) -> String {
    let mut out = String::new();
    for section in &template.template.sections {
        let in_section = only.is_none_or(|o| o == section.id || o.starts_with(&format!("{}/", section.id)));
        if !in_section {
            continue;
        }
        if let Some(kind) = section.kind {
            out.push_str(&guidance_line(
                template,
                &section.id,
                &section.id,
                &section.title,
                kind,
                &section.fields,
                section.instruction.as_deref(),
            ));
            continue;
        }
        if let Some(instruction) = &section.instruction {
            out.push_str(&format!("- {}: {} — {instruction}\n", section.id, section.title));
        }
        for standard in template.standards_for(&section.id) {
            out.push_str(&format!(
                "- {} (whole section): Team standard: {}.\n",
                section.id,
                standard.text.trim_end_matches('.')
            ));
        }
        for part in &section.parts {
            let path = format!("{}/{}", section.id, part.key);
            if only.is_some_and(|o| o != section.id && o != path) {
                continue;
            }
            out.push_str(&guidance_line(
                template,
                &path,
                &format!("{}.{}", section.id, part.key),
                &part.label,
                part.kind,
                &part.fields,
                part.instruction.as_deref(),
            ));
        }
    }
    out
}

fn rules(template: &LoadedTemplate) -> String {
    let mut out = String::from("\nRules:\n");
    for rule in CORE_RULES.iter().copied().chain(template.template.rules.iter().map(String::as_str)) {
        out.push_str(&format!("- {rule}\n"));
    }
    for standard in template.standards_for("") {
        out.push_str(&format!("- Team standard: {}\n", standard.text));
    }
    out
}

/// The system prompt: role, template instructions, rules and standards.
pub fn system_prompt(template: &LoadedTemplate) -> String {
    format!(
        "You are the documentation assistant inside {PRODUCT_NAME}, a veterinary scribe. \
         You turn a consultation transcript into a structured draft for the veterinarian to review.\n\n{}\n{}\
         \nWhat each part of the JSON should contain:\n{}\n{JSON_FORMATS}\nRespond with the JSON object only.",
        template.template.instructions.trim(),
        rules(template),
        guidance(template, None),
    )
}

fn user_message(transcript: &Transcript, context: Option<&str>) -> String {
    let entered = context
        .filter(|c| !c.trim().is_empty())
        .map(|c| {
            format!("Entered by the clinic before the visit (not from the recording, not evidence): {c}.\n\n")
        })
        .unwrap_or_default();
    format!(
        "{entered}Transcript of the consultation. Each line starts with its segment id in brackets; cite these ids as evidence.\n\n{}",
        transcript.for_prompt()
    )
}

/// System and user messages for a template + transcript.
pub fn build_messages(
    template: &LoadedTemplate,
    transcript: &Transcript,
    context: Option<&str>,
) -> Vec<ChatMessage> {
    vec![ChatMessage::system(system_prompt(template)), ChatMessage::user(user_message(transcript, context))]
}

/// Messages for redrafting one heading of an existing note.
pub fn build_refine_messages(
    template: &LoadedTemplate,
    transcript: &Transcript,
    context: Option<&str>,
    heading: &str,
    current: &Value,
    request: &str,
) -> Vec<ChatMessage> {
    let title = template.heading_title(heading).unwrap_or_else(|| heading.to_string());
    let system = format!(
        "You are the documentation assistant inside {PRODUCT_NAME}, a veterinary scribe. \
         A veterinarian is reviewing a draft note and asked you to redraft one part of it.\n\n{}\n{}\
         - The veterinarian's request says what to look for and how to write it. It is not evidence: \
         if the transcript doesn't contain what is asked, leave it out rather than invent it.\n\
         - Keep every fact in the current draft that the transcript supports, unless the veterinarian asked \
         to remove it. Reword, split, merge or add; don't drop.\n\
         \nThe part to redraft is \"{title}\":\n{}\n{JSON_FORMATS}\nRespond with the JSON for this part only.",
        template.template.instructions.trim(),
        rules(template),
        guidance(template, Some(heading)),
    );
    let user = format!(
        "{}\n\nCurrent draft of \"{title}\" (may be incomplete or wrong):\n{}\n\nThe veterinarian asks: {}",
        user_message(transcript, context),
        serde_json::to_string(current).unwrap_or_default(),
        request.trim()
    );
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// Generate a draft note. `on_token` streams raw JSON text as it is produced.
pub fn generate_note(
    llm: &dyn LlmBackend,
    template: &LoadedTemplate,
    transcript: &Transcript,
    options: &NoteOptions,
    on_token: &mut dyn FnMut(&str) -> bool,
) -> Result<NoteResult, EngineError> {
    if transcript.is_empty() {
        return Err(EngineError::EmptyTranscript);
    }
    let request = GenerateRequest {
        messages: build_messages(template, transcript, options.context.as_deref()),
        max_tokens: options.max_tokens,
        temperature: options.temperature,
        seed: options.seed,
        json_schema: Some(template.schema_text.clone()),
    };
    let output = llm.generate(&request, on_token)?;
    let note: Value = serde_json::from_str(output.text.trim()).map_err(|e| {
        let hint = if output.stats.hit_max_tokens { " (output hit max_tokens)" } else { "" };
        EngineError::BadJson(format!("{e}{hint}"))
    })?;
    validate(template, &note)?;
    let faithfulness = faithfulness::check(&note, transcript);
    let markdown = render::markdown(template, &note, transcript, &faithfulness);
    Ok(NoteResult {
        status: "draft",
        template: template.label(),
        model: llm.capabilities().model_name,
        note,
        faithfulness,
        stats: output.stats,
        refinement: None,
        markdown,
    })
}

/// Redraft one heading of `note` following the veterinarian's `request`.
/// The rest of the note is kept as it was; the whole note is then validated
/// and checked for faithfulness again.
#[allow(clippy::too_many_arguments)]
pub fn refine_note(
    llm: &dyn LlmBackend,
    template: &LoadedTemplate,
    transcript: &Transcript,
    note: &Value,
    heading: &str,
    request: &str,
    options: &NoteOptions,
    on_token: &mut dyn FnMut(&str) -> bool,
) -> Result<NoteResult, EngineError> {
    if transcript.is_empty() {
        return Err(EngineError::EmptyTranscript);
    }
    if request.trim().is_empty() {
        return Err(EngineError::Template("Say what should change under this heading.".into()));
    }
    let schema = template.heading_schema(heading).ok_or_else(|| {
        EngineError::Template(format!("{} has no heading '{heading}'.", template.template.name))
    })?;
    let (sec, part) = split_heading(heading);
    let current = match part {
        Some(key) => note[sec][key].clone(),
        None => note[sec].clone(),
    };
    let generate = GenerateRequest {
        messages: build_refine_messages(
            template,
            transcript,
            options.context.as_deref(),
            heading,
            &current,
            request,
        ),
        max_tokens: options.max_tokens,
        temperature: options.temperature,
        seed: options.seed,
        json_schema: Some(schema.to_string()),
    };
    let output = llm.generate(&generate, on_token)?;
    let redrafted: Value = serde_json::from_str(output.text.trim()).map_err(|e| {
        let hint = if output.stats.hit_max_tokens { " (output hit max_tokens)" } else { "" };
        EngineError::BadJson(format!("{e}{hint}"))
    })?;
    let mut note = note.clone();
    match part {
        Some(key) => note[sec][key] = redrafted,
        None => note[sec] = redrafted,
    }
    validate(template, &note)?;
    let faithfulness = faithfulness::check(&note, transcript);
    let markdown = render::markdown(template, &note, transcript, &faithfulness);
    Ok(NoteResult {
        status: "draft",
        template: template.label(),
        model: llm.capabilities().model_name,
        note,
        faithfulness,
        stats: output.stats,
        refinement: Some(Refinement {
            heading: heading.to_string(),
            request: request.trim().to_string(),
            from_run: None,
            saved_as_standard: false,
        }),
        markdown,
    })
}

pub fn validate(template: &LoadedTemplate, note: &Value) -> Result<(), EngineError> {
    let validator =
        jsonschema::validator_for(&template.schema).map_err(|e| EngineError::Template(e.to_string()))?;
    let errors: Vec<String> =
        validator.iter_errors(note).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    if errors.is_empty() { Ok(()) } else { Err(EngineError::SchemaViolation(errors.join("; "))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Segment;

    #[test]
    fn prompt_contains_rules_parts_and_transcript() {
        let template = LoadedTemplate::builtin("soap").unwrap();
        let transcript = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "Hello".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        let messages = build_messages(&template, &transcript, Some("patient name Milo"));
        assert!(messages[1].content.contains("not evidence): patient name Milo."));
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("Do not add findings"));
        assert!(messages[0].content.contains("objective.vitals"));
        assert!(messages[0].content.contains("plan.medications"));
        assert!(messages[1].content.contains("[0] Speaker 1 (00:00): Hello"));
    }

    fn one_line() -> Transcript {
        Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "We gave 100 mL of LRS.".into(),
                hints: Vec::new(),
                check: None,
            }],
        }
    }

    #[test]
    fn standards_reach_the_prompt() {
        let mut template = LoadedTemplate::builtin("soap").unwrap();
        let standard = |heading: &str, text: &str| crate::template::Standard {
            id: text.into(),
            heading: heading.into(),
            text: text.into(),
            added_at: chrono::Local::now(),
            added_by: None,
        };
        template.template.standards = vec![
            standard("plan/medications", "List fluids with volume and rate"),
            standard("", "Use metric units"),
            standard("assessment", "Problems before differentials"),
        ];
        let system = system_prompt(&template);
        assert!(
            system.contains(
                "- plan.medications: Medications Team standard: List fluids with volume and rate.\n"
            ),
            "{system}"
        );
        assert!(system.contains("- Team standard: Use metric units\n"));
        assert!(
            system.contains("- assessment (whole section): Team standard: Problems before differentials.")
        );
        assert!(
            system.contains("Use only information stated in the transcript"),
            "core rules always present"
        );
        assert!(system.contains("Differentials only if"), "template rules follow");
    }

    #[test]
    fn refine_prompt_covers_only_the_heading() {
        let template = LoadedTemplate::builtin("soap").unwrap();
        let messages = build_refine_messages(
            &template,
            &one_line(),
            None,
            "plan/medications",
            &serde_json::json!([]),
            "Include the fluids",
        );
        assert!(messages[0].content.contains("The part to redraft is \"Plan > Medications\""));
        assert!(messages[0].content.contains("- plan.medications: Medications"));
        assert!(!messages[0].content.contains("plan.follow_up"));
        assert!(messages[0].content.contains("It is not evidence"));
        assert!(messages[1].content.ends_with("The veterinarian asks: Include the fluids"));
        assert!(messages[1].content.contains("[0] Speaker 1 (00:00): We gave 100 mL of LRS."));
        // A whole section brings its subheadings.
        let section =
            build_refine_messages(&template, &one_line(), None, "plan", &serde_json::json!({}), "x");
        assert!(section[0].content.contains("plan.follow_up") && !section[0].content.contains("subjective."));
    }

    #[test]
    fn team_template_prompt_lists_headings() {
        let template = LoadedTemplate::from_yaml(
            "name: Dental\nsections:\n  - { title: Anaesthesia, kind: medications, instruction: Premeds and induction }\n  - { title: Charting, kind: fields, fields: [{ label: Teeth extracted }] }\n",
        )
        .unwrap();
        let system = system_prompt(&template);
        assert!(
            system.contains("- anaesthesia: Anaesthesia (medications) — Premeds and induction\n"),
            "{system}"
        );
        assert!(system.contains("- charting: Teeth extracted (null if not stated)\n"));
    }

    #[test]
    fn empty_note_skeleton_validates() {
        let template = LoadedTemplate::builtin("soap").unwrap();
        let note = render::tests::empty_note();
        validate(&template, &note).unwrap();
    }

    #[test]
    fn missing_section_fails_validation() {
        let template = LoadedTemplate::builtin("soap").unwrap();
        let mut note = render::tests::empty_note();
        note.as_object_mut().unwrap().remove("plan");
        assert!(matches!(validate(&template, &note), Err(EngineError::SchemaViolation(_))));
    }
}
