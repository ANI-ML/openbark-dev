//! Options for the llama.cpp backend, shared by builds with and without it.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LlamaCppOptions {
    pub model_path: PathBuf,
    /// Layers to offload to the GPU. `u32::MAX` = all (llama.cpp clamps).
    pub gpu_layers: u32,
    /// Prompt-processing batch size.
    pub batch_size: u32,
    /// CPU threads; `None` lets llama.cpp decide.
    pub threads: Option<i32>,
    /// Upper bound for per-request context; the model's trained length caps it.
    pub max_context: u32,
}

impl LlamaCppOptions {
    pub fn new(model_path: impl Into<PathBuf>) -> Self {
        Self {
            model_path: model_path.into(),
            gpu_layers: u32::MAX,
            batch_size: 2048,
            threads: None,
            max_context: 32_768,
        }
    }
}
