//! The speed check (ADR-0034): a short, timed generation from invented text
//! that measures how fast the note model reads and writes on this computer,
//! so setup can say how long a long visit's note takes here instead of
//! guessing. Nothing from any visit is used, and only counts and timings are
//! logged.

use std::time::{Duration, Instant};

use os_engine::awake::{AwakeGuard, AwakeReason};
use os_llm::{ChatMessage, GenerateRequest, LlmError};
use serde::Serialize;

use crate::policy::{self, DraftsOn, LONG_CONTEXT_SLOWDOWN};
use crate::{App, AppError, DRAFT_STOPPED, Events, Progress, Result, Stage};

/// Writing stops this long after the first word, or at [`MAX_TOKENS`].
pub const WRITE_BUDGET: Duration = Duration::from_secs(15);
/// The most the speed check writes.
pub const MAX_TOKENS: u32 = 256;

const INSTRUCTIONS: &str = "You summarise veterinary consult transcripts for the medical record. \
Use only what the transcript says. Answer with JSON: a list of short points.";

/// An invented consult, about 500 tokens with the instructions: no real
/// animal, owner or clinic. Long enough to time the reading, short enough to
/// read in seconds on a fast computer and well under a minute on a slow one.
const SAMPLE: &str = "Vet: Good morning. What brings you in today?
Client: She has been scratching her ears for about a week, mostly the left one, and shaking her head.
Vet: Any change in appetite or energy?
Client: No, she eats normally and still wants her walks twice a day.
Vet: Has she had ear problems before?
Client: Once, two summers ago, after swimming in the lake. It cleared up with drops.
Vet: Is she swimming this year?
Client: Most weekends, yes.
Vet: Let me look. Her temperature is 38.6 degrees, heart rate 96, breathing normal. The right ear canal \
looks clean. The left canal is red and swollen with brown discharge, and it smells yeasty. The eardrum is \
hard to see because of the swelling. She is a bit sore when I touch the base of the ear.
Client: Is it the same as last time?
Vet: It looks similar. I'll take a swab and look at it under the microscope.
Vet: The swab shows a lot of yeast and a few rod-shaped bacteria. I'll send a culture to be sure about \
the bacteria.
Client: What do we do in the meantime?
Vet: Clean the left ear once a day with the cleaning solution for three days, then start the ear drops, \
five drops twice a day for fourteen days. Dry her ears after every swim. If she gets worse, or the head \
shaking gets more frequent, call us.
Client: Can she keep swimming?
Vet: Let's keep her out of the water until we recheck in two weeks. I'll call you when the culture is back, \
in about three days.
Client: Thank you. Anything else I should watch for?
Vet: Tilting her head, losing her balance or a lot of pain. Those would mean the middle ear may be \
involved, and we would want to see her sooner.";

const SCHEMA: &str = r#"{"type":"object","properties":{"points":{"type":"array","maxItems":24,"items":{"type":"string","maxLength":240}}},"required":["points"]}"#;

/// What the speed check measured on this computer.
#[derive(Debug, Clone, Serialize)]
pub struct SpeedCheck {
    pub model: String,
    /// Loading the note model into memory (0 when it was already loaded).
    pub load_ms: u64,
    pub read_tokens: u32,
    pub read_ms: u64,
    pub read_tokens_per_second: f64,
    pub written_tokens: u32,
    pub write_tokens_per_second: f64,
    /// A long visit's note at these speeds ([`policy::long_visit_note_secs`]).
    pub long_visit_note_secs: Option<u64>,
    pub drafts_on: DraftsOn,
}

impl App {
    /// Load the note model (as a draft would) and time it reading the
    /// invented consult and writing for up to [`WRITE_BUDGET`]. Progress
    /// goes out as a draft's: the model loading, the prompt being read, the
    /// writing heartbeat. Stop ([`App::cancel_draft`]) ends it. Nothing is saved.
    pub fn speed_check(&self, llm: &str, events: &dyn Events) -> Result<SpeedCheck> {
        if self.is_recording() {
            return Err(AppError("Stop the recording first, then check the speed.".into()));
        }
        let _work = self.begin_work()?;
        let _awake = AwakeGuard::acquire(AwakeReason::Processing);
        self.drafting_with(events, |control| {
            let started = Instant::now();
            let backend = self.load_llm_with_progress(llm, events)?;
            let load_ms = started.elapsed().as_millis() as u64;
            if control.is_cancelled() {
                return Err(AppError(DRAFT_STOPPED.into()));
            }
            events.progress(&Progress::new(Stage::ReadTranscript, 0.0, 1.0));
            let mut request = GenerateRequest::new(vec![
                ChatMessage::system(INSTRUCTIONS),
                ChatMessage::user(format!("Transcript:\n{SAMPLE}")),
            ]);
            request.max_tokens = MAX_TOKENS;
            request.json_schema = Some(SCHEMA.into());
            request.control = Some(control.clone());
            let mut writing: Option<Instant> = None;
            let out = backend
                .generate(&request, &mut |_| {
                    let since = *writing.get_or_insert_with(|| {
                        events.progress(&Progress::new(Stage::Write, 0.0, 0.0));
                        Instant::now()
                    });
                    since.elapsed() < WRITE_BUDGET
                })
                .map_err(|e| match e {
                    LlmError::Cancelled => AppError(DRAFT_STOPPED.into()),
                    other => AppError(other.to_string()),
                })?;
            let stats = out.stats;
            let read_ms = stats.time_to_first_token_ms.max(1);
            let read = stats.prompt_tokens as f64 * 1000.0 / read_ms as f64;
            let write = stats.tokens_per_second;
            let note_secs = policy::long_visit_note_secs(read, write, LONG_CONTEXT_SLOWDOWN);
            // Counts and timings only.
            tracing::info!(
                model = %llm,
                load_ms,
                read_tokens = stats.prompt_tokens,
                read_ms,
                read_tokens_per_second = read,
                written_tokens = stats.completion_tokens,
                write_tokens_per_second = write,
                long_visit_note_secs = note_secs,
                "speed check finished"
            );
            Ok(SpeedCheck {
                model: llm.to_string(),
                load_ms,
                read_tokens: stats.prompt_tokens,
                read_ms,
                read_tokens_per_second: read,
                written_tokens: stats.completion_tokens,
                write_tokens_per_second: write,
                long_visit_note_secs: note_secs,
                drafts_on: self.hardware.drafts_on,
            })
        })
    }
}
