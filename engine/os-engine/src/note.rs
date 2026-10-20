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
    /// For client documents: the veterinarian's note for the visit, which the
    /// document follows where it differs from the transcript.
    pub reference: Option<Reference>,
    /// Language to write the note in ("fr", "es"…); the template's when None.
    pub language: Option<String>,
    /// Multi-pet visits: the animal this note is about, and the others.
    pub pet: Option<String>,
    pub other_pets: Vec<String>,
}

/// "This visit covered several animals… write only about Mango."
fn pet_line(pet: Option<&str>, others: &[String]) -> Option<String> {
    let pet = pet?.trim();
    if pet.is_empty() || others.is_empty() {
        return None;
    }
    Some(format!(
        "\n\nThis consultation covered more than one animal: {pet} and {}. Write this note only about {pet}: \
         include only what was said about {pet} (or about all the animals together, such as advice for the household), \
         and leave out what was said only about the others. If it's unclear which animal something was about, leave it out.",
        others.join(", ")
    ))
}

/// In a multi-pet visit, hint words that sound like the animals' names
/// ("Arlive" for Olive), so the model can tell whose findings a line holds.
/// Only the prompt sees these hints; the saved transcript is unchanged.
fn with_name_hints<'a>(
    transcript: &'a Transcript,
    options: &NoteOptions,
) -> std::borrow::Cow<'a, Transcript> {
    let names: Vec<String> = options.pet.iter().chain(options.other_pets.iter()).cloned().collect();
    if names.len() < 2 {
        return std::borrow::Cow::Borrowed(transcript);
    }
    let mut named = transcript.clone();
    for s in &mut named.segments {
        for h in crate::vocab::name_hints(&s.text, &names) {
            if !s.hints.iter().any(|x| x.heard == h.heard) {
                s.hints.push(h);
            }
        }
    }
    std::borrow::Cow::Owned(named)
}

/// Multi-pet visits: which animal each transcript line is about ("all" for
/// lines about every animal, the household, or none in particular). Asked of
/// the model as a short labelling task, so each animal's draft can be written
/// from its own lines (M2 item 2: drafting from the whole visit mixed up the
/// animals' vitals).
pub fn attribute_lines(
    llm: &dyn LlmBackend,
    transcript: &Transcript,
    pets: &[String],
) -> Result<(std::collections::BTreeMap<u32, String>, GenerateStats), EngineError> {
    let names = pets.join(", ");
    let system = format!(
        "You label the lines of a veterinary consultation that covered several animals: {names}.\n\
         For every line, say which animal it is about: one of {names}, or \"all\" when it is about every animal, \
         the household, or no animal in particular (greetings, small talk, payment).\n\
         Once the conversation turns to one animal, the following lines are about that animal until another is named. \
         Follow pronouns (he, she, his, her) and the exam being done. Names may be misheard; a hint such as \
         [heard \"Arlive\", likely Olive] tells you which animal was meant.\n\
         Label every line id once, in order."
    );
    let mut options =
        NoteOptions { pet: pets.first().cloned(), other_pets: pets[1..].to_vec(), ..Default::default() };
    options.pet = pets.first().cloned();
    let named = with_name_hints(transcript, &options);
    let user = format!("Transcript:\n\n{}", named.for_prompt());
    let mut choices: Vec<Value> = pets.iter().map(|p| Value::String(p.clone())).collect();
    choices.push(Value::String("all".into()));
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "about": { "type": "string", "enum": choices }
                    },
                    "required": ["id", "about"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["lines"],
        "additionalProperties": false
    });
    let request = GenerateRequest {
        messages: vec![ChatMessage::system(system), ChatMessage::user(user)],
        max_tokens: 64 + 24 * transcript.segments.len() as u32,
        temperature: 0.0,
        seed: 42,
        json_schema: Some(schema.to_string()),
    };
    let output = llm.generate(&request, &mut |_| true)?;
    let parsed: Value = serde_json::from_str(output.text.trim())
        .map_err(|e| EngineError::BadJson(format!("line labels: {e}")))?;
    let labels = parsed["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["id"].as_u64()? as u32, l["about"].as_str()?.to_string())))
        .collect();
    Ok((labels, output.stats))
}

/// The lines one animal's note may draw on: its own, and those about all of
/// them. Unlabelled lines are kept (when in doubt, the model's own rule to
/// leave out what's unclear applies).
pub fn lines_for_pet(
    transcript: &Transcript,
    labels: &std::collections::BTreeMap<u32, String>,
    pet: &str,
) -> Transcript {
    Transcript {
        segments: transcript
            .segments
            .iter()
            .filter(|s| labels.get(&s.id).is_none_or(|l| l == pet || l == "all"))
            .cloned()
            .collect(),
    }
}

/// Add the multi-pet instruction to a system message.
fn with_pet(mut messages: Vec<ChatMessage>, pet: Option<&str>, others: &[String]) -> Vec<ChatMessage> {
    if let (Some(line), Some(system)) = (pet_line(pet, others), messages.first_mut()) {
        system.content.push_str(&line);
    }
    messages
}

/// English names of the note languages offered in the app.
pub const NOTE_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("de", "German"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("nl", "Dutch"),
];

/// "Write the note in French…" when the language isn't English.
fn language_line(code: Option<&str>) -> Option<String> {
    let code = code?.trim();
    if code.is_empty() || code == "en" {
        return None;
    }
    let name = NOTE_LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n).unwrap_or(code);
    Some(format!(
        "\n\nLanguage: write all text in the note in {name}, whatever language the consultation was in. \
         This overrides any mention of English above. \
         Keep drug names, doses, units and numbers exactly as said. Field and heading names stay as given."
    ))
}

/// Add the language instruction to a system message.
fn with_language(mut messages: Vec<ChatMessage>, code: Option<&str>) -> Vec<ChatMessage> {
    if let (Some(line), Some(system)) = (language_line(code), messages.first_mut()) {
        system.content.push_str(&line);
    }
    messages
}

/// The vet's note given to a client document as reference.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    /// The note as rendered Markdown (compact, with "[entered by …]" marks).
    pub markdown: String,
    /// Numbers the vet entered by hand: supported even if not in the transcript.
    pub vet_numbers: std::collections::HashSet<String>,
}

impl Reference {
    pub fn from_note(note: &Value, markdown: &str) -> Self {
        Self { markdown: markdown.to_string(), vet_numbers: faithfulness::vet_entered_numbers(note) }
    }
}

impl Default for NoteOptions {
    fn default() -> Self {
        // Low temperature: charting should be deterministic and literal.
        Self {
            max_tokens: 4096,
            temperature: 0.1,
            seed: 42,
            context: None,
            reference: None,
            language: None,
            pet: None,
            other_pets: Vec::new(),
        }
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
    /// Set when this draft is a veterinarian's hand edit of an earlier one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<crate::edit::Edit>,
    /// Who asked for this revision (team members, M1 item 11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<crate::users::Who>,
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
    /// That run's id ([`crate::session::RunInfo::id`]): stable across
    /// devices, where folder names may differ (ADR-0017 section 13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_run_id: Option<String>,
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

/// The tooth line of the JSON formats, only for templates with a chart. Its
/// example is not taken from any eval fixture.
const TOOTH_FORMAT: &str = r#"- a tooth finding is {"tooth": "407", "text": "3 mm of gingival recession", "finding": "gingival_recession", "detail": null, "grade": null, "mm": 3, "planned": false, "evidence": [12]}
"#;

/// The drafting budget for a template with a tooth chart: a finding costs
/// about 50 tokens (ADR-0021 decision 5; the context limit is 32768).
pub const CHART_MAX_TOKENS: u32 = 8192;

/// The drafting budget: `default`, or [`CHART_MAX_TOKENS`] when the template
/// has a chart and `default` is smaller.
pub fn max_tokens_for(template: &LoadedTemplate, default: u32) -> u32 {
    if template.chart().is_some() { default.max(CHART_MAX_TOKENS) } else { default }
}

/// The JSON formats for a template: the tooth line and the finding guide only
/// when it has a chart.
fn json_formats(template: &LoadedTemplate) -> String {
    if template.chart().is_some() {
        format!("{JSON_FORMATS}{TOOTH_FORMAT}{}", tooth_findings_guide())
    } else {
        JSON_FORMATS.to_string()
    }
}

/// What each tooth finding takes, from the shared finding table
/// (`packages/dental/chart.json`), so the model pairs a type, a stage or a
/// measurement with the right finding. The grammar allows every detail on
/// every finding; this guide and the shape flag keep them together.
pub fn tooth_findings_guide() -> String {
    let chart = crate::dental::chart();
    let mut plain: Vec<&str> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for f in &chart.findings {
        let mut parts: Vec<String> = Vec::new();
        let details: Vec<String> = f
            .details
            .iter()
            .filter(|d| d.with_grade.is_none())
            .map(|d| {
                // Every group of words the type needs: "complicated or pulp + root".
                let take = if d.cues.len() > 1 { 2 } else { 3 };
                let said: Vec<String> = d
                    .cues
                    .iter()
                    .map(|g| {
                        g.iter()
                            .take(take)
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(if d.cues.len() > 1 { " or " } else { ", " })
                    })
                    .collect();
                if said.is_empty() { d.id.clone() } else { format!("{} ({})", d.id, said.join(" + ")) }
            })
            .collect();
        if !details.is_empty() {
            parts.push(format!("detail {}, or null when the type wasn't said", details.join(", ")));
        }
        if let Some(g) = &f.grade {
            let word = match f.id.as_str() {
                "periodontal_stage" | "resorption" => "stage",
                "resorption_type" => "type",
                _ => "grade",
            };
            parts.push(format!("grade = the {word} {}-{}", g.min, g.max));
        }
        let staged: Vec<String> = f
            .details
            .iter()
            .filter_map(|d| d.with_grade.map(|g| format!("{} ({g}{})", d.id, d.code)))
            .collect();
        if !staged.is_empty() {
            parts.push(format!("with stage 4, detail {}", staged.join(", ")));
        }
        match f.mm {
            crate::dental::MmUse::Required => parts.push("mm, the depth said".into()),
            crate::dental::MmUse::Optional => parts.push("mm if said".into()),
            crate::dental::MmUse::None => {}
        }
        if f.plannable {
            parts.push("planned true only while it is still to be done".into());
        }
        if f.text_required {
            parts.push("text says what was found".into());
        }
        if parts.is_empty() {
            plain.push(f.id.as_str());
        } else {
            lines.push(format!("- {}: {}\n", f.id, parts.join("; ")));
        }
    }
    format!(
        "Tooth findings (detail, grade and mm are null unless listed here):\n- {}: nothing else\n{}",
        plain.join(", "),
        lines.concat()
    )
}

/// Rules for a tooth chart (ADR-0021 decision 5), added whenever the template
/// has one, in drafting and re-prompting. Teams can't remove them.
pub const CHART_RULES: &[&str] = &[
    "Tooth chart: chart a tooth only when it was named, by number or by name, and a finding was said about it. Teeth not mentioned are left out. Chart every finding said about a named tooth, even when it also belongs under another heading (oral exam, radiographs, treatment). An extraction or crown amputation done in this visit is charted too, on each tooth, even when the line also says what wasn't done (\"closed, no sectioning\").",
    "Tooth chart: one item per finding. A tooth with a fracture and an extraction has two items; a stage, a pocket depth and a mobility said together are three items.",
    "Tooth chart: use the tooth number as said. When only a name was said, use its modified Triadan number: 100 right upper, 200 left upper, 300 left lower, 400 right lower; 500-800 the same for deciduous teeth; the incisors are x01 to x03, the canine x04 and the first molar x09.",
    "Tooth chart: never chart a tooth as normal unless normal, fine, healthy or the like was said about that tooth.",
    "Tooth chart: chart a tooth as missing only when it was said to be missing or absent. Never infer missing from an extraction or from silence.",
    "Tooth chart: whole-mouth statements (\"stage two across the mouth\", \"no resorptive lesions\", \"everything else looks normal\") never go on teeth.",
    "Tooth chart: a range or group (\"101 to 103\", \"all the lower incisors\") becomes one item per tooth, only when every tooth in it is clear.",
    "Tooth chart: grade and mm only when said. detail is null when the type wasn't said: \"fractured\" alone is never a guessed complicated fracture.",
    "Tooth chart: planned is true for a plan or a possibility (\"may need to come out\", \"we'll extract it at the recheck\"). A treatment planned and then done in the same recording is charted once, as done.",
    "Tooth chart: never chart a finding said to be absent (\"no resorption on 307\", \"404 is not fractured\", \"doesn't need to come out\"), and never give a tooth what was said about another tooth in the same line.",
    "Tooth chart: if a number and a name disagree (\"108, upper left fourth premolar\"), chart the number as said and keep both in text.",
    "Tooth chart: a line that names teeth ends with [teeth: ...], the Triadan numbers read from that line (by number, name, range or group). Go through every such line, exams and treatments alike, and chart what it says about each of those teeth. The list is not evidence of a finding, and never write it in text.",
];

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
        PartKind::Text => format!(
            "- {name}: {label} (a text box: connected sentences that read as a paragraph, each sentence one statement with its own evidence){}",
            with_instruction(instruction)
        ),
        PartKind::Teeth => format!(
            "- {name}: {label} (a tooth chart: one item per finding on one tooth; teeth not mentioned are left out){}",
            with_instruction(instruction)
        ),
        _ => format!("- {name}: {label}{}", with_instruction(instruction)),
    };
    for standard in template.standards_for(path) {
        line.push_str(&format!(" {}: {}.", standard_label(standard), standard.text.trim_end_matches('.')));
    }
    line.push('\n');
    line
}

/// "Team standard" for the team's standards, "Standard" for the drafting
/// person's own (ADR-0024 section 8).
fn standard_label(standard: &crate::template::Standard) -> &'static str {
    match standard.scope {
        crate::template::Scope::Team => "Team standard",
        crate::template::Scope::Personal => "Standard",
    }
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
                "- {} (whole section): {}: {}.\n",
                section.id,
                standard_label(standard),
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

/// The sentence that ends every rules block (ADR-0024 section 4.3): the rules
/// that always apply win over anything a template, a version or a standard says.
pub const PRECEDENCE: &str = "If a template rule, a standard or a heading instruction conflicts with the rules that always apply, follow the rules that always apply.";

/// The rules block: the rules that always apply (the core rules, then the
/// template's fixed rules), then the template's own rules, its standards for
/// the whole note and the level of detail, then [`PRECEDENCE`].
fn rules(template: &LoadedTemplate) -> String {
    rules_with(template, &[])
}

/// Redrafting one heading: the veterinarian's request steers, never adds facts.
const REFINE_RULES: &[&str] = &[
    "The veterinarian's request says what to look for and how to write it. It is not evidence: \
     if the transcript doesn't contain what is asked, leave it out rather than invent it.",
    "Keep every fact in the current draft that the transcript supports, unless the veterinarian asked \
     to remove it. Reword, split, merge or add; don't drop.",
];

/// [`rules`], with more rules that always apply after the template's fixed rules.
fn rules_with(template: &LoadedTemplate, also_always: &[&str]) -> String {
    let mut out = String::from("\nRules that always apply:\n");
    let chart: &[&str] = if template.chart().is_some() { CHART_RULES } else { &[] };
    let always = CORE_RULES
        .iter()
        .copied()
        .chain(template.template.fixed_rules.iter().map(String::as_str))
        .chain(chart.iter().copied())
        .chain(also_always.iter().copied());
    for rule in always {
        out.push_str(&format!("- {rule}\n"));
    }
    let mut own = String::new();
    for rule in &template.template.rules {
        own.push_str(&format!("- {rule}\n"));
    }
    for standard in template.standards_for("") {
        own.push_str(&format!("- {}: {}\n", standard_label(standard), standard.text));
    }
    match template.template.detail {
        crate::template::Detail::Concise => own.push_str(
            "- Level of detail: concise. Use the fewest words that keep every clinical fact; short phrases are fine.\n",
        ),
        crate::template::Detail::Thorough => own.push_str(
            "- Level of detail: thorough. Include every relevant detail that was said, with its context; still nothing that wasn't said.\n",
        ),
        crate::template::Detail::Standard => {}
    }
    if !own.is_empty() {
        out.push_str("Template rules:\n");
        out.push_str(&own);
    }
    out.push_str(PRECEDENCE);
    out.push('\n');
    out
}

/// The system prompt: role, template instructions, rules and standards.
pub fn system_prompt(template: &LoadedTemplate) -> String {
    format!(
        "You are the documentation assistant inside {PRODUCT_NAME}, a veterinary scribe. \
         You turn a consultation transcript into a structured draft for the veterinarian to review.\n\n{}\n{}\
         \nWhat each part of the JSON should contain:\n{}\n{}\nRespond with the JSON object only.",
        template.template.instructions.trim(),
        rules(template),
        guidance(template, None),
        json_formats(template),
    )
}

fn reference_message(reference: Option<&Reference>) -> String {
    reference
        .filter(|r| !r.markdown.trim().is_empty())
        .map(|r| {
            format!(
                "The veterinarian's note for this visit. Follow it where it differs from the transcript \
                 (the veterinarian corrected it); lines marked [entered by …] are the veterinarian's own. \
                 Still cite transcript lines as evidence.\n\n{}\n\n",
                r.markdown.trim()
            )
        })
        .unwrap_or_default()
}

/// For a template with a tooth chart, each line that names teeth ends with
/// `[teeth: 301, 302, 303, 401, 402, 403]`: the Triadan numbers the tooth
/// reader ([`crate::dental::hint_teeth`]) finds in it, by number, name, range or
/// group, limited to the chart's dentition. A name that disagrees with the
/// number beside it is left out, and a group with a count, a vague amount or
/// a negation in front of it ("two of the lower incisors") names no tooth. The model then numbers teeth said
/// by name or as a group the same way the faithfulness pass checks them, and
/// sees every line that names a tooth. Only the prompt sees these hints; the
/// saved transcript is unchanged, and templates without a chart are untouched.
pub fn with_tooth_hints<'a>(
    template: &LoadedTemplate,
    transcript: &'a Transcript,
) -> std::borrow::Cow<'a, Transcript> {
    let Some((_, dentition)) = template.chart() else {
        return std::borrow::Cow::Borrowed(transcript);
    };
    let mut hinted = transcript.clone();
    let mut changed = false;
    for s in &mut hinted.segments {
        let teeth: Vec<String> = crate::dental::hint_teeth(&s.text)
            .into_iter()
            .filter(|t| crate::dental::has_tooth(dentition, t))
            .collect();
        if !teeth.is_empty() {
            s.text = format!("{} [teeth: {}]", s.text, teeth.join(", "));
            changed = true;
        }
    }
    if changed { std::borrow::Cow::Owned(hinted) } else { std::borrow::Cow::Borrowed(transcript) }
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
    let hinted = with_tooth_hints(template, transcript);
    vec![ChatMessage::system(system_prompt(template)), ChatMessage::user(user_message(&hinted, context))]
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
         \nThe part to redraft is \"{title}\":\n{}\n{}\nRespond with the JSON for this part only.",
        template.template.instructions.trim(),
        rules_with(template, REFINE_RULES),
        guidance(template, Some(heading)),
        json_formats(template),
    );
    let hinted = with_tooth_hints(template, transcript);
    let user = format!(
        "{}\n\nCurrent draft of \"{title}\" (may be incomplete or wrong):\n{}\n\nThe veterinarian asks: {}",
        user_message(&hinted, context),
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
    // Multi-pet: draft each animal from its own lines (and the shared ones).
    let mut labelling_ms = 0;
    let scoped = match (options.pet.as_deref(), options.other_pets.is_empty()) {
        (Some(pet), false) => {
            let pets: Vec<String> =
                std::iter::once(pet.to_string()).chain(options.other_pets.iter().cloned()).collect();
            match attribute_lines(llm, transcript, &pets) {
                Ok((labels, stats)) => {
                    labelling_ms = stats.total_ms;
                    std::borrow::Cow::Owned(lines_for_pet(transcript, &labels, pet))
                }
                // Labelling failed: draft from the whole visit, with the multi-pet instruction.
                Err(_) => std::borrow::Cow::Borrowed(transcript),
            }
        }
        _ => std::borrow::Cow::Borrowed(transcript),
    };
    let named = with_name_hints(&scoped, options);
    let mut messages = with_pet(
        with_language(
            build_messages(template, &named, options.context.as_deref()),
            options.language.as_deref().or(Some(template.template.language.as_str())),
        ),
        options.pet.as_deref(),
        &options.other_pets,
    );
    if let Some(user) = messages.last_mut() {
        user.content = format!("{}{}", reference_message(options.reference.as_ref()), user.content);
    }
    let request = GenerateRequest {
        messages,
        max_tokens: max_tokens_for(template, options.max_tokens),
        temperature: options.temperature,
        seed: options.seed,
        json_schema: Some(template.schema_text.clone()),
    };
    let output = llm.generate(&request, on_token)?;
    let mut note: Value = serde_json::from_str(output.text.trim()).map_err(|e| {
        let hint = if output.stats.hit_max_tokens { " (output hit max_tokens)" } else { "" };
        EngineError::BadJson(format!("{e}{hint}"))
    })?;
    // A runaway repeat ("404 normal" seventeen times) becomes one item.
    crate::dental::merge_repeats(&mut note);
    validate(template, &note)?;
    let vet_numbers = options.reference.as_ref().map(|r| r.vet_numbers.clone()).unwrap_or_default();
    // Word-level alignment only when the note is in the consultation's language
    // (English here; other consultation languages pass through untranslated).
    let align = options.language.as_deref().is_none_or(|l| l == "en");
    let dentition = template.chart().and_then(|(_, d)| d);
    // Checked against the lines the draft was written from: in a multi-pet
    // visit, this animal's own lines, so a cited line about another animal is
    // invalid evidence.
    let faithfulness =
        faithfulness::check_for_pet(&note, &scoped, &vet_numbers, align, dentition, &options.other_pets);
    let markdown = render::markdown(template, &note, transcript, &faithfulness);
    Ok(NoteResult {
        status: "draft",
        template: template.label(),
        model: llm.capabilities().model_name,
        note,
        faithfulness,
        stats: GenerateStats { total_ms: output.stats.total_ms + labelling_ms, ..output.stats },
        refinement: None,
        edit: None,
        by: None,
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
    let named = with_name_hints(transcript, options);
    let generate = GenerateRequest {
        messages: with_pet(
            with_language(
                build_refine_messages(
                    template,
                    &named,
                    options.context.as_deref(),
                    heading,
                    &current,
                    request,
                ),
                options.language.as_deref().or(Some(template.template.language.as_str())),
            ),
            options.pet.as_deref(),
            &options.other_pets,
        ),
        max_tokens: max_tokens_for(template, options.max_tokens),
        temperature: options.temperature,
        seed: options.seed,
        json_schema: Some(schema.to_string()),
    };
    let output = llm.generate(&generate, on_token)?;
    let mut redrafted: Value = serde_json::from_str(output.text.trim()).map_err(|e| {
        let hint = if output.stats.hit_max_tokens { " (output hit max_tokens)" } else { "" };
        EngineError::BadJson(format!("{e}{hint}"))
    })?;
    crate::dental::merge_repeats(&mut redrafted);
    let mut note = note.clone();
    match part {
        Some(key) => note[sec][key] = redrafted,
        None => note[sec] = redrafted,
    }
    // The rest of the note may hold the vet's own edits.
    crate::edit::validate_edited(template, &note)?;
    let dentition = template.chart().and_then(|(_, d)| d);
    let faithfulness = faithfulness::check_for_pet(
        &note,
        transcript,
        &std::collections::HashSet::new(),
        true,
        dentition,
        &options.other_pets,
    );
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
            from_run_id: None,
            saved_as_standard: false,
        }),
        edit: None,
        by: None,
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
    fn each_animal_drafts_from_its_own_and_shared_lines() {
        let t = Transcript {
            segments: [
                "Hi both",
                "Olive weighs 3.9 kilos",
                "Pickles weighs 6.3 kilos",
                "Keep the litter clean",
                "Thanks",
            ]
            .iter()
            .enumerate()
            .map(|(i, text)| crate::transcript::Segment {
                id: i as u32,
                start: i as f32,
                end: i as f32 + 1.0,
                speaker: Some(0),
                text: text.to_string(),
                hints: Vec::new(),
                check: None,
            })
            .collect(),
        };
        let labels: std::collections::BTreeMap<u32, String> =
            [(0, "all"), (1, "Olive"), (2, "Pickles"), (3, "all")]
                .into_iter()
                .map(|(i, l)| (i, l.to_string()))
                .collect();
        let olive: Vec<u32> = lines_for_pet(&t, &labels, "Olive").segments.iter().map(|s| s.id).collect();
        assert_eq!(olive, [0, 1, 3, 4], "unlabelled line 4 is kept; Pickles's line 2 isn't");
        let pickles: Vec<u32> = lines_for_pet(&t, &labels, "Pickles").segments.iter().map(|s| s.id).collect();
        assert_eq!(pickles, [0, 2, 3, 4]);
    }

    #[test]
    fn multi_pet_notes_are_about_one_animal() {
        assert!(pet_line(Some("Mango"), &[]).is_none(), "single pet: no instruction");
        assert!(pet_line(None, &["Biscuit".into()]).is_none());
        let line = pet_line(Some("Mango"), &["Biscuit".into(), "Pepper".into()]).unwrap();
        assert!(line.contains("Mango and Biscuit, Pepper"));
        assert!(line.contains("only about Mango"));
    }

    #[test]
    fn note_language_is_asked_for_except_english() {
        assert!(language_line(Some("en")).is_none());
        assert!(language_line(None).is_none());
        let fr = language_line(Some("fr")).unwrap();
        assert!(fr.contains("in French") && fr.contains("numbers exactly as said"));
        assert!(language_line(Some("pl")).unwrap().contains("in pl"));
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
            scope: crate::template::Scope::Team,
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

    #[test]
    fn tooth_hints_only_in_chart_prompts() {
        let seg = |id: u32, text: &str| Segment {
            id,
            start: 0.0,
            end: 1.0,
            speaker: Some(0),
            text: text.into(),
            hints: Vec::new(),
            check: None,
        };
        let t = Transcript {
            segments: vec![
                seg(0, "All the lower incisors are mobile, grade three."),
                seg(1, "The upper left canine looks healthy."),
                seg(2, "Recheck in ten days."),
            ],
        };
        let dental = LoadedTemplate::builtin("dental").unwrap();
        let user = &build_messages(&dental, &t, None)[1].content;
        assert!(user.contains("grade three. [teeth: 301, 302, 303, 401, 402, 403]"), "{user}");
        assert!(user.contains("looks healthy. [teeth: 204]"), "{user}");
        assert!(user.contains("Recheck in ten days.\n"), "{user}");
        assert!(!t.segments[0].text.contains("[teeth"));
        // A cat's chart lists only a cat's teeth.
        let cat = dental.for_species(Some("Cat"));
        let t2 = Transcript { segments: vec![seg(0, "105 and 106 are missing.")] };
        assert!(build_messages(&cat, &t2, None)[1].content.contains("missing. [teeth: 106]"));
        // Re-prompting the chart sees them too; a template without a chart never does.
        let refine =
            build_refine_messages(&dental, &t, None, "dental_chart", &serde_json::json!([]), "Add 108");
        assert!(refine[1].content.contains("[teeth: 204]"));
        let soap = LoadedTemplate::builtin("soap").unwrap();
        assert!(!build_messages(&soap, &t, None)[1].content.contains("[teeth"));
    }

    #[test]
    fn chart_rules_and_tooth_format_only_for_templates_with_a_chart() {
        let dental = LoadedTemplate::builtin("dental").unwrap();
        let prompt = system_prompt(&dental);
        for rule in CHART_RULES {
            assert!(prompt.contains(rule), "missing chart rule: {rule}");
        }
        assert!(prompt.contains("- a tooth finding is {\"tooth\": \"407\""));
        assert!(prompt.contains("- dental_chart: Dental chart (a tooth chart:"));
        let soap = LoadedTemplate::builtin("soap").unwrap();
        assert!(!system_prompt(&soap).contains("Tooth chart"));
        assert!(!system_prompt(&soap).contains("tooth finding"));
        // Re-prompting the chart keeps its rules.
        let t = Transcript::default();
        let messages =
            build_refine_messages(&dental, &t, None, "dental_chart", &serde_json::json!([]), "Add 108");
        assert!(messages[0].content.contains(CHART_RULES[0]));
        // The budget doubles for a chart, and stays for everything else.
        assert_eq!(max_tokens_for(&dental, 4096), CHART_MAX_TOKENS);
        assert_eq!(max_tokens_for(&soap, 4096), 4096);
    }

    /// The app's prompt for a dog's dental (ADR-0021): the patient details
    /// the clinic entered go with the transcript, and the prompt says that
    /// today's extractions are charted, even beside "no sectioning", with no
    /// "or under Findings by tooth" left for a dog. Drafted from that prompt,
    /// E4B left every extraction off the dental-chart consult (the eval now
    /// drafts with the same patient details, docs/eval/M2.md).
    #[test]
    fn the_dental_prompt_with_patient_details_still_asks_for_extractions_on_the_chart() {
        let dental = LoadedTemplate::builtin("dental").unwrap().for_species(Some("Dog"));
        let t = Transcript {
            segments: vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                speaker: Some(0),
                text: "The lower incisors come out closed, no sectioning.".into(),
                hints: Vec::new(),
                check: None,
            }],
        };
        let context = "patient name Juno; reason for visit: Dental; species: Dog";
        let messages = build_messages(&dental, &t, Some(context));
        assert!(messages[1].content.contains(context), "{}", messages[1].content);
        assert!(messages[1].content.contains("[teeth: 301, 302, 303, 401, 402, 403]"));
        let system = &messages[0].content;
        assert!(
            system.contains(
                "An extraction or crown amputation done in this visit is charted too, on each tooth"
            )
        );
        assert!(
            system.contains(
                "Extractions and crown amputations go in the dental chart too, one item per tooth."
            )
        );
        assert!(
            !system.contains("or under Findings by tooth, one item per tooth"),
            "the weaker wording dropped extractions"
        );
    }
}
