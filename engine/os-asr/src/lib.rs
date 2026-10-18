//! Speech-to-text and voice activity detection for the OpenBark engine.
//!
//! Every audio model runs through sherpa-onnx so the binary carries a single
//! ONNX Runtime and no second copy of ggml (ADR-0001, spike S2).

mod audio;
pub mod capture;
mod recognizer;
mod vad;
mod wav;

/// Re-exported so other crates share this crate's link-mode decision.
pub use sherpa_onnx;

pub use audio::{Audio, AudioError, SAMPLE_RATE};
pub use recognizer::{AsrModel, OfflineAsr};
pub use vad::{SileroVad, SpeechSpan, StreamingVad, Utterance, VadOptions};
pub use wav::{AudioSink, WavWriter};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AsrCapabilities {
    pub backend: &'static str,
    pub model: String,
    pub streaming: bool,
    pub languages: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("model file not found: {0}")]
    ModelNotFound(String),
    #[error("failed to create recognizer (check model files and type)")]
    Create,
    #[error("recognition failed")]
    Recognize,
    #[error(transparent)]
    Audio(#[from] AudioError),
}

/// A speech-to-text backend over 16 kHz mono audio.
pub trait AsrBackend: Send + Sync {
    fn capabilities(&self) -> AsrCapabilities;
    fn transcribe(&self, samples: &[f32]) -> Result<String, AsrError>;
}

pub(crate) fn require_file(path: &str) -> Result<(), AsrError> {
    if std::path::Path::new(path).is_file() { Ok(()) } else { Err(AsrError::ModelNotFound(path.to_string())) }
}

pub(crate) fn default_threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get().min(8) as i32).unwrap_or(4)
}
