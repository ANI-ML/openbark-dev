//! The llama.cpp backend in a build without llama.cpp (`llama` feature off):
//! phones, until drafting on the phone is proven there (ADR-0018 sections 1
//! and 10). Loading a model says so instead of failing to link.

pub use crate::llama_options::LlamaCppOptions;
use crate::{GenerateOutput, GenerateRequest, LlmBackend, LlmCapabilities, LlmError};

/// What every call answers in this build.
pub const NOT_IN_THIS_BUILD: &str = "drafting with llama.cpp isn't in this build";

pub struct LlamaCppBackend {
    _never: std::convert::Infallible,
}

impl LlamaCppBackend {
    pub fn load(options: LlamaCppOptions) -> Result<Self, LlmError> {
        let _ = options;
        Err(LlmError::Load(NOT_IN_THIS_BUILD.into()))
    }
}

impl LlmBackend for LlamaCppBackend {
    fn capabilities(&self) -> LlmCapabilities {
        match self._never {}
    }

    fn generate(
        &self,
        _request: &GenerateRequest,
        _on_token: &mut dyn FnMut(&str) -> bool,
    ) -> Result<GenerateOutput, LlmError> {
        match self._never {}
    }
}
