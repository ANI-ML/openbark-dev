//! LLM backends for the OpenBark engine.
//!
//! The rest of the engine talks to [`LlmBackend`] only; which library serves a
//! request is an implementation detail (master prompt §4.2). M0 ships one
//! backend, [`LlamaCppBackend`], over `llama-cpp-2`.

mod chat_template;
mod llama;
pub mod mlx;

pub use chat_template::ChatMessage;
pub use llama::{LlamaCppBackend, LlamaCppOptions};
pub use mlx::{MlxBackend, MlxDevice, MlxOptions, ModelCheck, PackCheck};

use serde::Serialize;

/// What a backend can do. The orchestrator picks pipelines from this.
#[derive(Debug, Clone, Serialize)]
pub struct LlmCapabilities {
    pub backend: &'static str,
    pub model_name: String,
    pub context_length: u32,
    pub streaming: bool,
    pub json_schema_constrained: bool,
    pub audio_input: bool,
}

#[derive(Debug, Clone)]
pub struct GenerateRequest {
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub seed: u32,
    /// JSON Schema (as a string) the output must conform to.
    pub json_schema: Option<String>,
}

impl GenerateRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self { messages, max_tokens: 2048, temperature: 0.2, seed: 42, json_schema: None }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GenerateStats {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Prompt processing time (to first generated token), milliseconds.
    pub time_to_first_token_ms: u64,
    pub total_ms: u64,
    pub tokens_per_second: f64,
    pub hit_max_tokens: bool,
}

#[derive(Debug, Clone)]
pub struct GenerateOutput {
    pub text: String,
    pub stats: GenerateStats,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("model file not found: {0}")]
    ModelNotFound(String),
    #[error("failed to load model: {0}")]
    Load(String),
    #[error("prompt does not fit: {prompt} tokens + {max_new} new > context {ctx}")]
    ContextOverflow { prompt: u32, max_new: u32, ctx: u32 },
    #[error("chat template: {0}")]
    Template(String),
    #[error("grammar: {0}")]
    Grammar(String),
    #[error("inference: {0}")]
    Inference(String),
}

/// A compute device llama.cpp can use (GPU or CPU).
#[derive(Debug, Clone, Serialize)]
pub struct ComputeDevice {
    pub name: String,
    pub description: String,
    pub backend: String,
    pub memory_total_bytes: u64,
}

/// Devices visible to the compiled llama.cpp backends (CUDA, Metal, Vulkan, CPU).
pub fn devices() -> Vec<ComputeDevice> {
    llama_cpp_2::list_llama_ggml_backend_devices()
        .into_iter()
        .map(|d| ComputeDevice {
            name: d.name,
            description: d.description,
            backend: d.backend,
            memory_total_bytes: d.memory_total as u64,
        })
        .collect()
}

/// The llama.cpp grammar a JSON Schema constrains drafting to, as the
/// llama.cpp backend builds it. Lets callers check that a template's schema
/// converts (the tooth chart's enums, for example) without loading a model.
pub fn grammar_for_schema(schema: &str) -> Result<String, LlmError> {
    llama_cpp_2::json_schema_to_grammar(schema).map_err(|e| LlmError::Grammar(e.to_string()))
}

/// A text-generation backend.
pub trait LlmBackend: Send + Sync {
    fn capabilities(&self) -> LlmCapabilities;

    /// Generate a completion. `on_token` receives text pieces as they are
    /// produced (always valid UTF-8); return `false` from it to stop early.
    fn generate(
        &self,
        request: &GenerateRequest,
        on_token: &mut dyn FnMut(&str) -> bool,
    ) -> Result<GenerateOutput, LlmError>;
}
