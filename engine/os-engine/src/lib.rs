//! The OpenBark scribe pipeline.
//!
//! audio → VAD → (diarization) → ASR → [`Transcript`] → template-constrained
//! note generation → faithfulness check → rendered draft.
//!
//! Notes produced here are always drafts: nothing in the engine signs, sends
//! or writes to a PIMS.

pub mod align;
pub mod audit;
pub mod awake;
pub mod crosscheck;
pub mod edit;
pub mod faithfulness;
pub mod fingerprints;
pub mod from_example;
pub mod live;
pub mod models;
pub mod note;
pub mod numbers;
pub mod patients;
pub mod pdf;
pub mod render;
pub mod session;
pub mod template;
pub mod template_diff;
pub mod template_lint;
pub mod template_store;
pub mod transcribe;
pub mod transcript;
pub mod trash;
pub mod users;
pub mod vault;
pub mod vocab;

use std::path::{Path, PathBuf};

pub use faithfulness::FaithfulnessReport;
/// OpenBark's housekeeping folder inside the visits folder (ADR-0022).
pub use os_models::locations::HOUSEKEEPING_DIR;

/// `<visits>/.openbark/deleted`: Deleted visits, kept for 30 days.
pub fn deleted_dir(root: &Path) -> PathBuf {
    root.join(HOUSEKEEPING_DIR).join("deleted")
}

/// `<visits>/.openbark/audit.log`: the deletion record.
pub fn audit_path(root: &Path) -> PathBuf {
    root.join(HOUSEKEEPING_DIR).join("audit.log")
}

/// `<visits>/.openbark/policy.json`: the clinic's retention rule (phase 2).
pub fn policy_path(root: &Path) -> PathBuf {
    root.join(HOUSEKEEPING_DIR).join("policy.json")
}
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
    #[error("{0}")]
    Template(String),
    #[error("model output is not valid JSON: {0}")]
    BadJson(String),
    #[error("model output does not match the template schema: {0}")]
    SchemaViolation(String),
    #[error("transcript is empty: no speech was recognised")]
    EmptyTranscript,
}
