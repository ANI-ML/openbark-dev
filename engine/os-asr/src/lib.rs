//! Speech-to-text and voice activity detection for the OpenBark engine.
//!
//! Every audio model runs through sherpa-onnx so the binary carries a single
//! ONNX Runtime and no second copy of ggml (ADR-0001, spike S2).

//!
//! Features (ADR-0018 section 1): `stt` links sherpa-onnx (recognisers, voice
//! detection, its WAV reader and resampler); `capture` links cpal for the
//! microphone. Desktop builds have both. Without `stt` the recognisers say
//! speech recognition isn't in this build, and audio is still decoded and
//! resampled, in Rust.

mod audio;
pub mod capture;
#[cfg(all(feature = "capture", target_os = "ios"))]
mod ios_audio;
pub mod level;
pub mod mic;
mod recognizer;
pub mod resample;
mod vad;
mod wav;

/// Re-exported so other crates share this crate's link-mode decision.
#[cfg(feature = "stt")]
pub use sherpa_onnx;

pub use resample::Resampler;

pub use audio::{Audio, AudioError, SAMPLE_RATE};
pub use recognizer::{AsrModel, OfflineAsr};
pub use resample::StreamResampler;
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
    /// A build without the `stt` feature (a phone before transcription is
    /// turned on there).
    #[error("speech recognition isn't in this build")]
    NotInThisBuild,
}

/// A speech-to-text backend over 16 kHz mono audio.
pub trait AsrBackend: Send + Sync {
    fn capabilities(&self) -> AsrCapabilities;
    fn transcribe(&self, samples: &[f32]) -> Result<String, AsrError>;
}

#[cfg_attr(not(feature = "stt"), allow(dead_code))]
pub(crate) fn require_file(path: &str) -> Result<(), AsrError> {
    if std::path::Path::new(path).is_file() { Ok(()) } else { Err(AsrError::ModelNotFound(path.to_string())) }
}

/// Threads for the main recogniser: `OPENBARK_ASR_THREADS` (for tuning),
/// else a third of the processor, 2 to 6. Measured on a 20-thread machine
/// (Parakeet, 8.5-minute consult): 2 threads 56 s, 4 34 s, 6 28 s, 8 39 s,
/// 16 92 s. More threads than that fight each other, and the second
/// recogniser and the note model need the rest.
pub(crate) fn default_threads() -> i32 {
    if let Some(n) =
        std::env::var("OPENBARK_ASR_THREADS").ok().and_then(|v| v.parse::<i32>().ok()).filter(|n| *n > 0)
    {
        return n;
    }
    (logical_cpus() / 3).clamp(2, 6)
}

/// Threads for the second recogniser, which runs beside the main one:
/// `OPENBARK_SECOND_THREADS` (for tuning), else two fifths of the processor,
/// 2 to 8. It's the slower model, so an import waits on it once the main
/// recogniser is done (8.5-minute consult, 20 threads: main 6 and second 4
/// took 183 s; main 8 and second 8, 152 s).
pub fn second_threads() -> i32 {
    if let Some(n) =
        std::env::var("OPENBARK_SECOND_THREADS").ok().and_then(|v| v.parse::<i32>().ok()).filter(|n| *n > 0)
    {
        return n;
    }
    (logical_cpus() * 2 / 5).clamp(2, 8)
}

/// Where speech recognition runs: `OPENBARK_ASR_PROVIDER` ("cpu", "cuda",
/// "coreml"), else the GPU when the GPU speech pack is installed beside the
/// program (ONNX Runtime's CUDA provider, ADR-0014), else the processor.
/// Small models (voice detection, speaker voices) stay on the processor.
pub fn asr_provider() -> String {
    if let Ok(p) = std::env::var("OPENBARK_ASR_PROVIDER") {
        let p = p.trim().to_ascii_lowercase();
        if !p.is_empty() {
            return p;
        }
    }
    if cuda_pack_installed() && gpu_usable() { "cuda".into() } else { "cpu".into() }
}

/// 0: not checked yet, 1: the GPU passed its self-test, 2: it failed.
static GPU_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Record the GPU self-test's result. Until it passes, recognisers run on the
/// processor: a GPU that fails inside ONNX Runtime can throw an exception
/// that aborts the program, so it's only tried in a separate process first.
pub fn set_gpu_usable(ok: bool) {
    GPU_STATE.store(if ok { 1 } else { 2 }, std::sync::atomic::Ordering::SeqCst);
}

pub fn gpu_usable() -> bool {
    GPU_STATE.load(std::sync::atomic::Ordering::SeqCst) == 1
}

/// The provider for one model: int8 models stay on the processor (on the
/// GPU they measured slower: 8.5-minute consult, Parakeet int8 43 s on an RTX
/// 4080 against 28 s on 6 processor threads) unless `OPENBARK_ASR_PROVIDER`
/// says otherwise.
#[cfg(feature = "stt")]
pub(crate) fn provider_for(model: &recognizer::AsrModel) -> String {
    let forced = std::env::var("OPENBARK_ASR_PROVIDER").is_ok_and(|p| !p.trim().is_empty());
    if !forced && model.files().iter().any(|f| f.contains(".int8.")) {
        return "cpu".into();
    }
    asr_provider()
}

/// ONNX Runtime's CUDA provider library next to the running program.
pub fn cuda_pack_installed() -> bool {
    let name =
        if cfg!(windows) { "onnxruntime_providers_cuda.dll" } else { "libonnxruntime_providers_cuda.so" };
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).is_some_and(|p| p.is_file())
}

fn logical_cpus() -> i32 {
    std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4)
}
