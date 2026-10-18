//! The OpenBark scribe pipeline.
//!
//! audio → VAD → (diarization) → ASR → [`Transcript`] → template-constrained
//! note generation → faithfulness check → rendered draft.
//!
//! Notes produced here are always drafts: nothing in the engine signs, sends
//! or writes to a PIMS.

pub mod awake;
pub mod crosscheck;
pub mod edit;
pub mod faithfulness;
pub mod live;
pub mod models;
pub mod note;
mod numbers;
pub mod pdf;
pub mod render;
pub mod session;
pub mod template;
pub mod template_store;
pub mod transcribe;
pub mod transcript;
pub mod users;
pub mod vault;
pub mod vocab;

pub use faithfulness::FaithfulnessReport;
pub use note::{NoteOptions, NoteResult, Refinement, generate_note, refine_note};
pub use template::LoadedTemplate;
pub use template_store::TemplateStore;
pub use transcribe::{TranscribeStats, Transcriber};
pub use transcript::{Segment, Transcript};

/// Product name, kept in one place so a rename is a one-line change.
pub const PRODUCT_NAME: &str = "OpenBark";

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Asr(#[from] os_asr::AsrError),
    #[error(transparent)]
    Diar(#[from] os_diar::DiarError),
    #[error(transparent)]
    Llm(#[from] os_llm::LlmError),
    #[error("{0}")]
    Model(String),
    #[error("{0}")]
    Session(String),
    #[error("template: {0}")]
    Template(String),
    #[error("model output is not valid JSON: {0}")]
    BadJson(String),
    #[error("model output does not match the template schema: {0}")]
    SchemaViolation(String),
    #[error("transcript is empty: no speech was recognised")]
    EmptyTranscript,
}
