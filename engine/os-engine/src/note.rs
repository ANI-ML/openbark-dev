//! Template-constrained note generation.

use os_llm::{
    ChatMessage, Control, GenerateOutput, GenerateRequest, GenerateStats, LlmBackend, LlmError, Phase,
};
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
    /// The Stop button and the drafting board's progress, shared with the
    /// model while it runs.
    pub control: Option<Control>,
    /// Re-prompting: the template's normal findings still waiting in the
    /// current draft (ADR-0032). A redrafted finding for a system drops its
    /// default; none is ever added back.
    pub pending_defaults: Vec<crate::defaults::PendingDefault>,
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
    control: Option<&Control>,
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
        control: control.cloned(),
    };
    let output = llm.generate(&request, &mut |_| true)?;
    if control.is_some_and(Control::is_cancelled) {
        return Err(LlmError::Cancelled.into());
    }
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
            control: None,
            pending_defaults: Vec::new(),
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
    /// Normal findings from the template for systems nothing was said about,
    /// waiting for a veterinarian (ADR-0032). Kept beside the note, never in
    /// it, so older versions, the checks, Copy, the PDF and client documents
    /// never take them for findings. Signing waits until none is left.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_defaults: Vec<crate::defaults::PendingDefault>,
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
const TOOTH_FORMAT: &str = r#"- a tooth finding is {"tooth": "407", "text": "3 mm of gingival recession", "finding": "gingival_recession", "grade": null, "detail": null, "mm": 3, "planned": false, "evidence": [12]}
"#;

/// The drafting budget for a template with a tooth chart: a finding costs
/// about 50 tokens (ADR-0021 decision 5; the context limit is 32768).
pub const CHART_MAX_TOKENS: u32 = 8192;

/// The drafting budget: `default`, or [`CHART_MAX_TOKENS`] when the template
/// has a chart and `default` is smaller.
pub fn max_tokens_for(template: &LoadedTemplate, default: u32) -> u32 {
    if template.chart().is_some() {
        default.max(CHART_MAX_TOKENS)
    } else if !template.template.document.is_record() {
        default.min(CLIENT_MAX_TOKENS)
    } else {
        default
    }
}

/// The drafting budget for a client document (a summary, discharge
/// instructions): they are written to be read in a minute or two, about 250
/// to 400 words, which with the JSON keys and the cited line numbers comes to
/// well under 1,000 tokens. Twice that leaves room for a long visit and stops
/// a runaway heading at half the note budget (a clinic's client summary ran
/// on, unfinished, for most of an hour on a slow laptop). Checked against the
/// long-visit CI job (.github/workflows/drafting.yml).
pub const CLIENT_MAX_TOKENS: u32 = 2048;

/// The statement line for headings that list systems (ADR-0032), only for
/// templates with normal findings.
const SYSTEM_FORMAT: &str = r#"- under a heading that lists systems, a statement also names its system: {"text": "Serous discharge from the left eye", "system": "Eyes", "evidence": [7]}
"#;

/// The JSON formats for a template: the tooth line and the finding guide only
/// when it has a chart, the system line only when it has normal findings.
fn json_formats(template: &LoadedTemplate) -> String {
    let mut out = JSON_FORMATS.to_string();
    if !crate::template::headings_with_defaults(&template.template).is_empty() {
        out.push_str(SYSTEM_FORMAT);
    }
    if template.chart().is_some() {
        out.push_str(TOOTH_FORMAT);
        out.push_str(&tooth_findings_guide());
    }
    out
}

/// What each tooth finding takes, from the shared finding table
/// (`packages/dental/chart.json`), so the model pairs a type, a stage or a
/// measurement with the right finding. The grammar allows every detail on
/// every finding; this guide and the shape flag keep them together.
pub fn tooth_findings_guide() -> String {
    let chart = crate::dental::chart();
    let mut plain: Vec<String> = Vec::new();
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
                let name = crate::dental::draft_name(&d.id);
                if said.is_empty() { name.to_string() } else { format!("{name} ({})", said.join(" + ")) }
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
        if f.id == "extraction" {
            // A sectioned extraction is a closed one too (AVDC XS), so "closed"
            // is said with both.
            parts.push(format!(
                "a tooth said to be sectioned is {} even when also said to be closed",
                crate::dental::draft_name("closed_sectioned")
            ));
        }
        if f.plannable {
            parts.push("planned true only while it is still to be done".into());
        }
        if f.text_required {
            parts.push("text says what was found".into());
        }
        let name = crate::dental::draft_name(&f.id);
        if parts.is_empty() {
            // The words that signal it: "normal (normal, fine, healthy)".
            let said: Vec<&str> = f.cues.iter().take(3).map(String::as_str).collect();
            plain.push(if said.is_empty() {
                name.to_string()
            } else {
                format!("{name} ({})", said.join(", "))
            });
        } else {
            lines.push(format!("- {name}: {}\n", parts.join("; ")));
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
    "Tooth chart: never chart a finding said to be absent (\"no resorption on 307\", \"404 is not fractured\", \"doesn't need to come out\"), and never give a tooth what was said about another tooth in the same line. Missing (\"not present\") and unerupted (\"hasn't erupted\") are findings, not absent ones.",
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
    // The systems of a heading with normal findings, never their text (ADR-0032).
    if let Some(systems) = crate::defaults::systems_note(template, path) {
        line.push_str(&systems);
    }
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

/// The schema the drafting grammar follows: the template's, with tooth
/// findings in their drafting names ([`crate::dental::DRAFT_NAMES`]).
/// Every list and free string is bounded ([`crate::template::bounded_for_drafting`]).
/// Statements under a heading with normal findings also name their system
/// ([`crate::defaults::tag_systems`]); the template's own schema has no tag.
pub fn drafting_schema_text(template: &LoadedTemplate) -> String {
    let mut schema = template.schema.clone();
    crate::defaults::tag_systems(template, &mut schema, None);
    drafting_schema_for(&schema).to_string()
}

/// [`drafting_schema_text`] for one heading's schema (re-prompting).
fn drafting_schema_for(schema: &Value) -> Value {
    crate::template::bounded_for_drafting(&crate::dental::drafting_schema(schema))
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
        serde_json::to_string(&{
            let mut current = current.clone();
            crate::dental::rename_for_drafting(&mut current, true);
            current
        })
        .unwrap_or_default(),
        request.trim()
    );
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// The model's JSON. A draft cut off at the token budget is never parsed into
/// a note: it says so ([`EngineError::RanOut`]).
fn parse_output(output: &GenerateOutput) -> Result<Value, EngineError> {
    serde_json::from_str(output.text.trim()).map_err(|e| {
        if output.stats.hit_max_tokens {
            EngineError::RanOut(e.to_string())
        } else {
            EngineError::BadJson(e.to_string())
        }
    })
}

/// Run the model with the repetition guard and the Stop button: a draft that
/// starts repeating itself stops early ([`EngineError::Repeating`]), one that
/// was stopped comes back as [`LlmError::Cancelled`], and the control hears
/// [`Phase::Generated`] before the caller checks the output.
fn generate_guarded(
    llm: &dyn LlmBackend,
    request: &GenerateRequest,
    on_token: &mut dyn FnMut(&str) -> bool,
) -> Result<GenerateOutput, EngineError> {
    let control = request.control.as_ref();
    let mut guard = RepeatGuard::default();
    let mut repeating = false;
    let output = llm.generate(request, &mut |piece| {
        if control.is_some_and(Control::is_cancelled) {
            return false;
        }
        if guard.push(piece) {
            repeating = true;
            return false;
        }
        on_token(piece)
    })?;
    if control.is_some_and(Control::is_cancelled) {
        return Err(LlmError::Cancelled.into());
    }
    if repeating {
        tracing::warn!(
            completion_tokens = output.stats.completion_tokens,
            "drafting stopped: the model repeated itself"
        );
        return Err(EngineError::Repeating);
    }
    if let Some(c) = control {
        c.report(Phase::Generated);
    }
    Ok(output)
}

/// Spots a model stuck repeating itself: the end of what it has written is
/// one short stretch (a phrase, a line, a whole statement) said over and over.
/// Legitimate notes repeat words and keys, never the same run of text ten
/// times in a row, and tooth-chart items (over 100 characters each, merged
/// when repeated: [`crate::dental::merge_repeats`]) are longer than any
/// period checked here.
#[derive(Default)]
pub struct RepeatGuard {
    tail: Vec<u8>,
    pieces: usize,
}

impl RepeatGuard {
    /// Longest repeating stretch looked for, in bytes.
    const MAX_PERIOD: usize = 100;
    /// Times a stretch must repeat back to back.
    const REPEATS: usize = 10;
    /// Bytes the repeats must cover at least, so a short run of the same
    /// character never counts.
    const MIN_SPAN: usize = 200;
    const KEEP: usize = 4096;

    /// Add a piece of output; true when the output has started repeating.
    pub fn push(&mut self, piece: &str) -> bool {
        self.tail.extend_from_slice(piece.as_bytes());
        if self.tail.len() > 2 * Self::KEEP {
            self.tail.drain(..self.tail.len() - Self::KEEP);
        }
        self.pieces += 1;
        // Every few pieces is plenty: a loop goes on for hundreds.
        self.pieces.is_multiple_of(8) && self.repeating()
    }

    fn repeating(&self) -> bool {
        let t = &self.tail;
        let n = t.len();
        (1..=Self::MAX_PERIOD).any(|p| {
            let span = (p * Self::REPEATS).max(Self::MIN_SPAN);
            span + p <= n && (n - span..n).all(|i| t[i] == t[i - p])
        })
    }
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
            match attribute_lines(llm, transcript, &pets, options.control.as_ref()) {
                Ok((labels, stats)) => {
                    labelling_ms = stats.total_ms;
                    std::borrow::Cow::Owned(lines_for_pet(transcript, &labels, pet))
                }
                // Stop pressed while the lines were labelled: stop, don't draft.
                Err(EngineError::Llm(LlmError::Cancelled)) => return Err(LlmError::Cancelled.into()),
                Err(EngineError::Llm(LlmError::Stopped)) => return Err(LlmError::Stopped.into()),
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
        json_schema: Some(drafting_schema_text(template)),
        control: options.control.clone(),
    };
    let output = generate_guarded(llm, &request, on_token)?;
    let mut note: Value = parse_output(&output)?;
    crate::dental::rename_for_drafting(&mut note, false);
    // A runaway repeat ("404 normal" seventeen times) becomes one item.
    crate::dental::merge_repeats(&mut note);
    // "Species: Yorkshire terrier" becomes Dog, from the breed table.
    crate::breeds::species_from_breed(&mut note, &scoped);
    // "Tartre : null" is dropped and a value of "null" becomes a real null,
    // before the defaults are decided, so a blank never counts as a finding.
    let blanks = crate::blanks::scrub_note(&mut note);
    // Normal findings for the systems nothing was said about (ADR-0032),
    // decided in code; the tags the model gave come out of the note here.
    let template_defaults =
        crate::defaults::undiscussed(template, &mut note, Some(&scoped), crate::defaults::all(template));
    if blanks > 0 || !template_defaults.is_empty() {
        tracing::info!(blanks, template_defaults = template_defaults.len(), "draft tidied");
    }
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
        template_defaults,
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
    let mut schema = template.heading_schema(heading).ok_or_else(|| {
        EngineError::Template(format!("{} has no heading '{heading}'.", template.template.name))
    })?;
    crate::defaults::tag_systems(template, &mut schema, Some(heading));
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
        json_schema: Some(drafting_schema_for(&schema).to_string()),
        control: options.control.clone(),
    };
    let output = generate_guarded(llm, &generate, on_token)?;
    let mut redrafted: Value = parse_output(&output)?;
    crate::dental::rename_for_drafting(&mut redrafted, false);
    crate::dental::merge_repeats(&mut redrafted);
    crate::blanks::scrub_note(&mut redrafted);
    let mut note = note.clone();
    match part {
        Some(key) => note[sec][key] = redrafted,
        None => note[sec] = redrafted,
    }
    if sec == "patient" {
        crate::breeds::species_from_breed(&mut note, transcript);
    }
    // A redrafted finding for a system drops its waiting default (ADR-0032).
    let template_defaults =
        crate::defaults::undiscussed(template, &mut note, None, options.pending_defaults.clone());
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
        template_defaults,
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
    use serde_json::json;

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

    /// Answers every request with one canned draft, and keeps the requests.
    struct Canned {
        text: String,
        requests: std::sync::Mutex<Vec<GenerateRequest>>,
    }

    impl LlmBackend for Canned {
        fn capabilities(&self) -> os_llm::LlmCapabilities {
            os_llm::LlmCapabilities {
                backend: "canned",
                model_name: "canned".into(),
                context_length: 32768,
                streaming: true,
                json_schema_constrained: true,
                audio_input: false,
            }
        }

        fn generate(
            &self,
            request: &GenerateRequest,
            on_token: &mut dyn FnMut(&str) -> bool,
        ) -> Result<os_llm::GenerateOutput, os_llm::LlmError> {
            self.requests.lock().unwrap().push(request.clone());
            on_token(&self.text);
            Ok(os_llm::GenerateOutput { text: self.text.clone(), stats: Default::default() })
        }
    }

    /// A template whose exam heading has normal findings, each text marked
    /// so a test can look for it anywhere the model could see it (ADR-0032).
    const MARKED_EXAM: &str = r#"
name: Exam
sections:
  - title: Reason
    kind: statements
  - title: Physical exam
    kind: statements
    defaults:
      - { label: Eyes, text: "ZQX-EYES clear, no discharge" }
      - { label: Ears, text: "ZQX-EARS clean" }
      - { label: Tartar, text: "ZQX-TARTAR none" }
      - { label: Temperature, text: "ZQX-TEMP 38.5 °C" }
"#;

    fn labels(p: &[crate::defaults::PendingDefault]) -> Vec<&str> {
        p.iter().map(|d| d.label.as_str()).collect()
    }

    /// ADR-0032: the default text never reaches the model, in drafting or
    /// re-prompting: not in a message, not in the grammar. The prompt only
    /// names the systems, and the rule against unsaid normals is unchanged.
    #[test]
    fn normal_finding_text_never_reaches_the_model() {
        let template = LoadedTemplate::from_yaml(MARKED_EXAM).unwrap();
        let draft = json!({
            "reason": [{"text": "Fluids given", "evidence": [0]}],
            "physical_exam": [{"text": "Mild discharge from the left eye", "system": "Eyes", "evidence": [0]}]
        });
        let llm = Canned { text: draft.to_string(), requests: Default::default() };
        let (t, none) = (one_line(), NoteOptions::default());
        let result = generate_note(&llm, &template, &t, &none, &mut |_| true).unwrap();
        let redraft = json!([
            {"text": "Mild discharge from the left eye", "system": "Eyes", "evidence": [0]},
            {"text": "Ear canals clean", "system": "Ears", "evidence": [0]}
        ]);
        let again = Canned { text: redraft.to_string(), requests: Default::default() };
        let pending_defaults = result.template_defaults.clone();
        let options = NoteOptions { pending_defaults, ..Default::default() };
        let refined = refine_note(
            &again,
            &template,
            &t,
            &result.note,
            "physical_exam",
            "Add the ears",
            &options,
            &mut |_| true,
        )
        .unwrap();
        let mut requests = llm.requests.lock().unwrap().clone();
        requests.extend(again.requests.lock().unwrap().iter().cloned());
        assert_eq!(requests.len(), 2);
        for request in &requests {
            for m in &request.messages {
                assert!(!m.content.contains("ZQX"), "default text in a message: {}", m.content);
            }
            let schema = request.json_schema.as_deref().unwrap();
            assert!(!schema.contains("ZQX"), "default text in the grammar");
            assert!(schema.contains("\"system\""), "statements are tagged with their system");
        }
        let system = &requests[0].messages[0].content;
        assert!(system.contains(&format!("- {}\n", CORE_RULES[1])), "the rule is unchanged");
        let systems = "Systems to report on, only if they were discussed: Eyes, Ears, Tartar, Temperature.";
        assert!(system.contains(systems), "{system}");
        assert!(requests[1].messages[0].content.contains("Systems to report on"));
        // Drafting: the discussed system loses its default, the tags are gone.
        assert_eq!(labels(&result.template_defaults), ["Ears", "Tartar", "Temperature"]);
        assert!(!result.note.to_string().contains("\"system\""));
        // Re-prompting: the ears were redrafted, so their default goes.
        assert_eq!(labels(&refined.template_defaults), ["Tartar", "Temperature"]);
        assert!(!refined.note.to_string().contains("\"system\""));
    }

    /// ADR-0032: the faithfulness pass never sees a default: not flagged, not
    /// counted, and a number in one ("38.5 °C") is never an unsupported number.
    /// "Tartar : null" is dropped, so Tartar shows as its default, marked.
    #[test]
    fn faithfulness_skips_defaults_and_null_statements_are_dropped() {
        let template = LoadedTemplate::from_yaml(MARKED_EXAM).unwrap();
        let draft = json!({
            "reason": [{"text": "We gave 100 mL of LRS", "evidence": [0]}],
            "physical_exam": [
                {"text": "Tartar : null", "system": "Tartar", "evidence": [0]},
                {"text": "T: null", "system": "Temperature", "evidence": [0]},
                {"text": "null", "system": null, "evidence": [0]}
            ]
        });
        let llm = Canned { text: draft.to_string(), requests: Default::default() };
        let (t, none) = (one_line(), NoteOptions::default());
        let result = generate_note(&llm, &template, &t, &none, &mut |_| true).unwrap();
        assert_eq!(result.note["physical_exam"], json!([]));
        assert!(!result.note.to_string().contains("null\""), "{}", result.note);
        assert_eq!(labels(&result.template_defaults), ["Eyes", "Ears", "Tartar", "Temperature"]);
        let f = &result.faithfulness;
        assert!(f.passed, "{f:?}");
        assert_eq!(f.statements_checked, 1, "only the reason line");
        assert!(f.unsupported_numbers.is_empty() && f.citation_mismatches.is_empty());
        assert!(f.flagged_paths().iter().all(|p| p.starts_with("/reason/")), "{f:?}");
        // Saved: the defaults sit beside the note, never in it.
        let saved = serde_json::to_value(&result).unwrap();
        assert_eq!(crate::defaults::pending_in(&saved), result.template_defaults);
        assert!(!saved["note"].to_string().contains("ZQX"));
        assert!(!result.markdown.contains("ZQX") && !result.markdown.contains("null"));
    }

    /// The tagged statements convert to a drafting grammar, labels with
    /// accents included ("Cœur").
    #[test]
    fn the_tagged_drafting_schema_converts_to_a_grammar() {
        let template = LoadedTemplate::from_yaml(crate::defaults::tests::EXAM).unwrap();
        let schema = drafting_schema_text(&template);
        assert!(schema.contains("Cœur") && !schema.contains("Rythme régulier"));
        let grammar = os_llm::grammar_for_schema(&schema).unwrap();
        assert!(grammar.contains("system"));
        // Templates without normal findings draft with the same schema as before.
        let soap = LoadedTemplate::builtin("soap").unwrap();
        assert_eq!(drafting_schema_text(&soap), drafting_schema_for(&soap.schema).to_string());
    }

    /// The model drafts with names no other id starts ("resorption_stage",
    /// "sectioned_closed"); the saved draft holds the finding table's ids,
    /// and the template's own schema is unchanged.
    #[test]
    fn a_chart_is_drafted_with_drafting_names_and_saved_with_ids() {
        let template = LoadedTemplate::builtin("dental").unwrap().for_species(Some("cat"));
        let mut note = serde_json::Map::new();
        for (key, node) in template.schema["properties"].as_object().unwrap() {
            let empty = match node.get("properties").and_then(Value::as_object) {
                Some(fields) => Value::Object(fields.keys().map(|k| (k.clone(), Value::Null)).collect()),
                None => json!([]),
            };
            note.insert(key.clone(), empty);
        }
        note.insert(
            "dental_chart".into(),
            json!([
                {"tooth": "307", "text": "stage 4b", "finding": "resorption_stage", "grade": 4, "detail": "crown_worse", "mm": null, "planned": false, "evidence": [0]},
                {"tooth": "307", "text": "type 2", "finding": "resorption_type", "grade": 2, "detail": null, "mm": null, "planned": false, "evidence": [1]},
                {"tooth": "309", "text": "sectioned and extracted", "finding": "extraction", "grade": null, "detail": "sectioned_closed", "mm": null, "planned": false, "evidence": [2]}
            ]),
        );
        let llm = Canned { text: Value::Object(note).to_string(), requests: Default::default() };
        let seg = |id: u32, text: &str| Segment {
            id,
            start: id as f32,
            end: id as f32 + 1.0,
            speaker: Some(0),
            text: text.into(),
            hints: Vec::new(),
            check: None,
        };
        let t = Transcript {
            segments: vec![
                seg(0, "Three oh seven has tooth resorption, stage four b."),
                seg(1, "On the radiograph, three oh seven is type two."),
                seg(2, "Three oh nine is sectioned and extracted."),
            ],
        };
        let result = generate_note(&llm, &template, &t, &NoteOptions::default(), &mut |_| true).unwrap();
        let chart = result.note["dental_chart"].as_array().unwrap();
        assert_eq!(chart[0]["finding"], "resorption");
        assert_eq!(chart[1]["finding"], "resorption_type");
        assert_eq!(chart[2]["detail"], "closed_sectioned");
        let requests = llm.requests.lock().unwrap();
        let schema = requests[0].json_schema.as_deref().unwrap();
        assert!(schema.contains("\"resorption_stage\"") && !schema.contains("\"resorption\""), "{schema}");
        assert!(schema.contains("\"sectioned_closed\"") && !schema.contains("\"closed_sectioned\""));
        assert!(requests[0].messages[0].content.contains("- resorption_stage: grade = the stage 1-5"));
        assert!(
            template.schema_text.contains("\"resorption\"")
                && !template.schema_text.contains("resorption_stage")
        );
        // Templates without a chart draft with their own schema, bounded.
        let soap = LoadedTemplate::builtin("soap").unwrap();
        assert_eq!(
            drafting_schema_text(&soap),
            crate::template::bounded_for_drafting(&soap.schema).to_string()
        );
        assert!(!soap.schema_text.contains("maxLength"), "the saved schema has no bounds");
    }

    /// Lists and free strings are bounded in the drafting grammar (0.2.3),
    /// and every built-in template's bounded schema still converts quickly.
    #[test]
    fn drafting_schemas_are_bounded_and_still_convert() {
        use crate::template::{
            MAX_EVIDENCE, MAX_MEDICATION_CHARS, MAX_MEDICATIONS, MAX_STATEMENTS, MAX_TEXT_CHARS,
            MAX_UNIT_CHARS,
        };
        for id in LoadedTemplate::builtin_ids() {
            let t = LoadedTemplate::builtin(id).unwrap();
            let text = drafting_schema_text(&t);
            let bounded: Value = serde_json::from_str(&text).unwrap();
            let defs = &bounded["$defs"];
            assert_eq!(defs["statements"]["maxItems"], MAX_STATEMENTS, "{id}");
            assert_eq!(defs["evidence"]["maxItems"], MAX_EVIDENCE, "{id}");
            assert_eq!(defs["evidence"]["minItems"], 1, "{id}: existing bounds stay");
            assert_eq!(defs["statement"]["properties"]["text"]["maxLength"], MAX_TEXT_CHARS, "{id}");
            assert_eq!(defs["medication"]["properties"]["drug"]["maxLength"], MAX_MEDICATION_CHARS, "{id}");
            assert_eq!(
                defs["field"]["anyOf"][1]["properties"]["unit"]["anyOf"][1]["maxLength"], MAX_UNIT_CHARS,
                "{id}"
            );
            for node in bounded["properties"].as_object().unwrap().values() {
                if node["items"]["$ref"] == "#/$defs/medication" {
                    assert_eq!(node["maxItems"], MAX_MEDICATIONS, "{id}");
                }
            }
            let started = std::time::Instant::now();
            let grammar = os_llm::grammar_for_schema(&text).unwrap();
            assert!(grammar.contains("{0,"), "{id}: the bounds reach the grammar");
            assert!(started.elapsed().as_millis() < 2000, "{id}: {:?}", started.elapsed());
        }
        // The tooth chart keeps its own cap; enums aren't given a length.
        let dental = LoadedTemplate::builtin("dental").unwrap();
        let bounded: Value = serde_json::from_str(&drafting_schema_text(&dental)).unwrap();
        assert_eq!(bounded["properties"]["dental_chart"]["maxItems"], crate::template::MAX_CHART_ITEMS);
        assert!(bounded["$defs"]["tooth_finding"]["properties"]["tooth"].get("maxLength").is_none());
    }

    /// Client documents get a smaller budget; notes keep theirs.
    #[test]
    fn client_documents_draft_with_a_smaller_budget() {
        for id in ["client-summary", "discharge", "estimate-explanation", "euthanasia-aftercare"] {
            let t = LoadedTemplate::builtin(id).unwrap();
            assert_eq!(max_tokens_for(&t, 4096), CLIENT_MAX_TOKENS, "{id}");
        }
        let soap = LoadedTemplate::builtin("soap").unwrap();
        assert_eq!(max_tokens_for(&soap, 4096), 4096);
    }

    #[test]
    fn the_repeat_guard_stops_loops_and_lets_notes_through() {
        // A phrase said over and over inside a string.
        let mut guard = RepeatGuard::default();
        assert!((0..200).any(|_| guard.push("and the ")));
        // The same statement again and again.
        let mut guard = RepeatGuard::default();
        let item = r#"{"text": "Recheck in two weeks", "evidence": [40]}, "#;
        assert!((0..40).any(|_| guard.push(item)));
        // A real note, streamed in small pieces, never trips it.
        let note =
            include_str!("../../../docs/perf/results/rtx4080-win11-20261017/feline-vomiting-paced.e4b.md");
        let mut guard = RepeatGuard::default();
        let chars: Vec<char> = note.chars().collect();
        for piece in chars.chunks(3) {
            assert!(!guard.push(&piece.iter().collect::<String>()));
        }
        // Nor does a long list of different evidence ids.
        let mut guard = RepeatGuard::default();
        for n in 0..300 {
            assert!(!guard.push(&format!("{n}, ")));
        }
    }

    /// Writes the same statement until told to stop.
    struct Looping;

    impl LlmBackend for Looping {
        fn capabilities(&self) -> os_llm::LlmCapabilities {
            Canned { text: String::new(), requests: Default::default() }.capabilities()
        }

        fn generate(
            &self,
            request: &GenerateRequest,
            on_token: &mut dyn FnMut(&str) -> bool,
        ) -> Result<os_llm::GenerateOutput, os_llm::LlmError> {
            let mut text = String::from(r#"{"subjective": {"history": ["#);
            for n in 0..request.max_tokens {
                let piece = r#"{"text": "Eating well", "evidence": [0]}, "#;
                text.push_str(piece);
                if !on_token(piece) {
                    let stats = GenerateStats { completion_tokens: n, ..Default::default() };
                    return Ok(os_llm::GenerateOutput { text, stats });
                }
            }
            let stats = GenerateStats { hit_max_tokens: true, ..Default::default() };
            Ok(os_llm::GenerateOutput { text, stats })
        }
    }

    /// Answers with `text`, cut off at the token budget.
    struct CutOff(&'static str);

    impl LlmBackend for CutOff {
        fn capabilities(&self) -> os_llm::LlmCapabilities {
            Looping.capabilities()
        }

        fn generate(
            &self,
            _request: &GenerateRequest,
            on_token: &mut dyn FnMut(&str) -> bool,
        ) -> Result<os_llm::GenerateOutput, os_llm::LlmError> {
            on_token(self.0);
            let stats = GenerateStats { hit_max_tokens: true, ..Default::default() };
            Ok(os_llm::GenerateOutput { text: self.0.into(), stats })
        }
    }

    #[test]
    fn a_looping_draft_stops_early_with_a_clear_error() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let mut streamed = 0;
        let err = generate_note(&Looping, &soap, &one_line(), &NoteOptions::default(), &mut |_| {
            streamed += 1;
            true
        })
        .unwrap_err();
        assert!(matches!(err, EngineError::Repeating), "{err}");
        assert!(streamed < 100, "stopped after {streamed} pieces, not at the budget");
    }

    #[test]
    fn a_stopped_draft_says_so_and_a_cut_off_one_never_parses() {
        let soap = LoadedTemplate::builtin("soap").unwrap();
        let control = Control::new();
        control.cancel();
        let options = NoteOptions { control: Some(control), ..Default::default() };
        let err = generate_note(&Looping, &soap, &one_line(), &options, &mut |_| true).unwrap_err();
        assert!(matches!(err, EngineError::Llm(LlmError::Cancelled)), "{err}");
        let cut = CutOff(r#"{"subjective": {"history": [{"text": "Eat"#);
        let err =
            generate_note(&cut, &soap, &one_line(), &NoteOptions::default(), &mut |_| true).unwrap_err();
        assert!(matches!(err, EngineError::RanOut(_)), "{err}");
        assert!(err.to_string().ends_with("(output hit max_tokens)"));
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
