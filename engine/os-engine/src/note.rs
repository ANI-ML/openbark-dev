//! Template-constrained note generation.

use os_llm::{ChatMessage, GenerateRequest, GenerateStats, LlmBackend};
use serde::Serialize;
use serde_json::Value;

use crate::faithfulness::{self, FaithfulnessReport};
use crate::render;
use crate::template::{LoadedTemplate, PartKind};
use crate::transcript::Transcript;
use crate::{EngineError, PRODUCT_NAME};

#[derive(Debug, Clone)]
pub struct NoteOptions {
    pub max_tokens: u32,
    pub temperature: f32,
    pub seed: u32,
}

impl Default for NoteOptions {
    fn default() -> Self {
        // Low temperature: charting should be deterministic and literal.
        Self { max_tokens: 4096, temperature: 0.1, seed: 42 }
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
    #[serde(skip)]
    pub markdown: String,
}

/// System and user messages for a template + transcript.
pub fn build_messages(template: &LoadedTemplate, transcript: &Transcript) -> Vec<ChatMessage> {
    let t = &template.template;
    let mut system = format!(
        "You are the documentation assistant inside {PRODUCT_NAME}, a veterinary scribe. \
         You turn a consultation transcript into a structured draft for the veterinarian to review.\n\n{}\n",
        t.instructions.trim()
    );
    if !t.rules.is_empty() {
        system.push_str("\nRules:\n");
        for rule in &t.rules {
            system.push_str(&format!("- {rule}\n"));
        }
    }
    system.push_str("\nWhat each part of the JSON should contain:\n");
    for section in &t.sections {
        if section.kind == Some(PartKind::Fields) {
            let labels: Vec<&str> = section.fields.iter().map(|f| f.label.as_str()).collect();
            system.push_str(&format!("- {}: {} (null if not stated)\n", section.id, labels.join(", ")));
        }
        for part in &section.parts {
            let mut line = format!("- {}.{}: {}", section.id, part.key, part.label);
            if let Some(instruction) = &part.instruction {
                line.push_str(&format!(" — {instruction}"));
            }
            if part.kind == PartKind::Fields {
                let labels: Vec<&str> = part.fields.iter().map(|f| f.label.as_str()).collect();
                line.push_str(&format!(" [{}; null if not stated]", labels.join(", ")));
            }
            system.push_str(&line);
            system.push('\n');
        }
    }
    system.push_str("\nRespond with the JSON object only.");

    let user = format!(
        "Transcript of the consultation. Each line starts with its segment id in brackets; cite these ids as evidence.\n\n{}",
        transcript.for_prompt()
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
        messages: build_messages(template, transcript),
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
            segments: vec![Segment { id: 0, start: 0.0, end: 1.0, speaker: Some(0), text: "Hello".into() }],
        };
        let messages = build_messages(&template, &transcript);
        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("Do not add findings"));
        assert!(messages[0].content.contains("objective.vitals"));
        assert!(messages[0].content.contains("plan.medications"));
        assert!(messages[1].content.contains("[0] Speaker 1 (00:00): Hello"));
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
