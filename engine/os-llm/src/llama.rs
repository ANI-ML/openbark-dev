//! llama.cpp backend via `llama-cpp-2`.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Instant;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::token::data::LlamaTokenData;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;

use crate::{
    GenerateOutput, GenerateRequest, GenerateStats, LlmBackend, LlmCapabilities, LlmError, chat_template,
};

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

pub struct LlamaCppBackend {
    // Field order matters: the model must drop before the backend.
    model: LlamaModel,
    backend: LlamaBackend,
    options: LlamaCppOptions,
    chat_template: Option<String>,
    model_name: String,
}

impl LlamaCppBackend {
    /// Load a GGUF model. Only one instance may exist per process (llama.cpp
    /// backend initialisation is global).
    pub fn load(options: LlamaCppOptions) -> Result<Self, LlmError> {
        if !Path::new(&options.model_path).is_file() {
            return Err(LlmError::ModelNotFound(options.model_path.display().to_string()));
        }
        let mut backend = LlamaBackend::init().map_err(|e| LlmError::Load(e.to_string()))?;
        if std::env::var_os("OPENBARK_LLAMA_LOG").is_none() {
            backend.void_logs();
        }
        let params = LlamaModelParams::default().with_n_gpu_layers(options.gpu_layers);
        let model = LlamaModel::load_from_file(&backend, &options.model_path, &params)
            .map_err(|e| LlmError::Load(e.to_string()))?;
        let chat_template = model.meta_val_str("tokenizer.chat_template").ok();
        // The file name identifies the exact quant; `general.name` is often
        // uninformative (Google's Gemma 4 GGUFs say "Hf").
        let model_name = options.model_path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        tracing::info!(model = %model_name, has_template = chat_template.is_some(), "llm loaded");
        Ok(Self { model, backend, options, chat_template, model_name })
    }

    fn token_text(&self, token: llama_cpp_2::token::LlamaToken) -> String {
        let bytes = self.model.vocab().token_to_piece(token, true, None);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn format_prompt(&self, request: &GenerateRequest) -> Result<String, LlmError> {
        let template = self
            .chat_template
            .as_deref()
            .ok_or_else(|| LlmError::Template("model has no tokenizer.chat_template".into()))?;
        let vocab = self.model.vocab();
        let bos = self.token_text(vocab.bos());
        let eos = self.token_text(vocab.eos());
        chat_template::render(template, &request.messages, &bos, &eos)
            .map_err(|e| LlmError::Template(e.to_string()))
    }
}

impl LlmBackend for LlamaCppBackend {
    fn capabilities(&self) -> LlmCapabilities {
        LlmCapabilities {
            backend: "llama.cpp",
            model_name: self.model_name.clone(),
            context_length: self.model.n_ctx_train(),
            streaming: true,
            json_schema_constrained: true,
            // Audio needs the mtmd path (S2 configuration C); not wired in M0.
            audio_input: false,
        }
    }

    fn generate(
        &self,
        request: &GenerateRequest,
        on_token: &mut dyn FnMut(&str) -> bool,
    ) -> Result<GenerateOutput, LlmError> {
        let started = Instant::now();
        let prompt = self.format_prompt(request)?;
        let vocab = self.model.vocab();
        // The rendered template already contains BOS where the model wants it.
        let tokens = vocab.tokenize(prompt.as_bytes(), false, true);
        let prompt_tokens = tokens.len() as u32;

        let ctx_limit = self.model.n_ctx_train().min(self.options.max_context);
        let needed = prompt_tokens + request.max_tokens;
        if needed > ctx_limit {
            return Err(LlmError::ContextOverflow {
                prompt: prompt_tokens,
                max_new: request.max_tokens,
                ctx: ctx_limit,
            });
        }
        // Round up to a multiple of 256 to keep llama.cpp's allocator happy.
        let n_ctx = needed.div_ceil(256) * 256;
        let n_batch = self.options.batch_size.min(n_ctx);
        let mut ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(n_batch.min(512));
        if let Some(threads) = self.options.threads {
            ctx_params = ctx_params.with_n_threads(threads).with_n_threads_batch(threads);
        }
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| LlmError::Inference(e.to_string()))?;

        // Prompt processing in batch-sized chunks; logits only for the last token.
        let mut batch = LlamaBatch::new(n_batch as usize, 1);
        let last = tokens.len() - 1;
        for (chunk_idx, chunk) in tokens.chunks(n_batch as usize).enumerate() {
            batch.clear();
            for (i, token) in chunk.iter().enumerate() {
                let pos = chunk_idx * n_batch as usize + i;
                batch
                    .add(*token, pos as i32, &[0], pos == last)
                    .map_err(|e| LlmError::Inference(e.to_string()))?;
            }
            ctx.decode(&mut batch).map_err(|e| LlmError::Inference(e.to_string()))?;
        }
        let first_token_at = Instant::now();

        let mut sampler = Sampler::new(&self.model, request)?;
        let mut text = String::new();
        let mut pending: Vec<u8> = Vec::new();
        let mut completion_tokens = 0u32;
        let mut pos = tokens.len() as i32;
        let mut hit_max_tokens = true;

        while completion_tokens < request.max_tokens {
            let token = sampler.next(&ctx, batch.n_tokens() - 1)?;
            if vocab.is_eog(token) {
                hit_max_tokens = false;
                break;
            }
            completion_tokens += 1;
            pending.extend(vocab.token_to_piece(token, false, None));
            let valid = match std::str::from_utf8(&pending) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            if valid > 0 {
                let piece =
                    String::from_utf8(pending.drain(..valid).collect()).expect("validated utf-8 prefix");
                text.push_str(&piece);
                if !on_token(&piece) {
                    hit_max_tokens = false;
                    break;
                }
            }
            batch.clear();
            batch.add(token, pos, &[0], true).map_err(|e| LlmError::Inference(e.to_string()))?;
            pos += 1;
            ctx.decode(&mut batch).map_err(|e| LlmError::Inference(e.to_string()))?;
        }
        if !pending.is_empty() {
            text.push_str(&String::from_utf8_lossy(&pending));
        }

        let gen_secs = first_token_at.elapsed().as_secs_f64();
        Ok(GenerateOutput {
            text,
            stats: GenerateStats {
                prompt_tokens,
                completion_tokens,
                time_to_first_token_ms: (first_token_at - started).as_millis() as u64,
                total_ms: started.elapsed().as_millis() as u64,
                tokens_per_second: if gen_secs > 0.0 { completion_tokens as f64 / gen_secs } else { 0.0 },
                hit_max_tokens,
            },
        })
    }
}

/// Sampling with an optional JSON-schema grammar, applied lazily.
///
/// Masking the full vocabulary with the grammar on every token is CPU-bound
/// and dominated generation time with Gemma 4's ~262k-token vocabulary (20
/// tok/s on an RTX 4080 in spike S2). Like llama.cpp's own `common_sampler`,
/// we sample unconstrained first and only check that one token against the
/// grammar; the full mask runs only when the token is rejected.
struct Sampler {
    chain: LlamaSampler,
    /// A fresh chain used to pick among grammar-masked candidates.
    fallback: LlamaSampler,
    grammar: Option<LlamaSampler>,
}

impl Sampler {
    fn new(model: &LlamaModel, request: &GenerateRequest) -> Result<Self, LlmError> {
        let grammar = match &request.json_schema {
            Some(schema) => {
                let grammar = compact_whitespace(
                    &llama_cpp_2::json_schema_to_grammar(schema)
                        .map_err(|e| LlmError::Grammar(e.to_string()))?,
                );
                Some(
                    LlamaSampler::grammar(model, &grammar, "root")
                        .map_err(|e| LlmError::Grammar(e.to_string()))?,
                )
            }
            None => None,
        };
        Ok(Self { chain: selection_chain(request), fallback: selection_chain(request), grammar })
    }

    fn next(
        &mut self,
        ctx: &llama_cpp_2::context::LlamaContext<'_>,
        idx: i32,
    ) -> Result<LlamaToken, LlmError> {
        let token = self.chain.sample(ctx, idx);
        let Some(grammar) = self.grammar.as_mut() else { return Ok(token) };

        let mut single = LlamaTokenDataArray::new(vec![LlamaTokenData::new(token, 1.0, 0.0)], false);
        single.apply_sampler(grammar);
        let token = if single.data[0].logit().is_finite() {
            token
        } else {
            let mut candidates = ctx.token_data_array_ith(idx);
            candidates.apply_sampler(grammar);
            candidates.apply_sampler(&mut self.fallback);
            candidates
                .selected_token()
                .ok_or_else(|| LlmError::Grammar("no token satisfies the grammar".into()))?
        };
        grammar.accept(token);
        Ok(token)
    }
}

fn selection_chain(request: &GenerateRequest) -> LlamaSampler {
    if request.temperature <= 0.0 {
        return LlamaSampler::greedy();
    }
    LlamaSampler::chain_simple([
        LlamaSampler::top_k(40),
        LlamaSampler::top_p(0.95, 1),
        LlamaSampler::temp(request.temperature),
        LlamaSampler::dist(request.seed),
    ])
}

/// llama.cpp's JSON grammar allows newlines plus up to 20 spaces/tabs between
/// tokens; models then spend a large share of output on indentation. Allow at
/// most a single space instead (still valid JSON, far fewer tokens).
fn compact_whitespace(grammar: &str) -> String {
    grammar
        .lines()
        .map(|line| if line.trim_start().starts_with("space ::=") { r#"space ::= | " ""# } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::compact_whitespace;

    #[test]
    fn whitespace_rule_is_replaced_and_other_rules_kept() {
        let g = concat!(
            r#"root ::= "{" space "}""#,
            "\n",
            r#"space ::= | " " | "\n"{1,2} [ \t]{0,20}"#,
            "\n",
            r#"string ::= "x""#
        );
        let out = compact_whitespace(g);
        assert!(out.contains(r#"space ::= | " ""#));
        assert!(!out.contains("{0,20}"));
        assert!(out.starts_with("root ::="));
        assert!(out.ends_with("string ::= \"x\""));
    }
}
