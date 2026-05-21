//! Wrapper around `llama-cpp-2`'s `LlamaModel`, specialized for one-shot
//! summarization runs.
//!
//! Model load is the expensive part (~3-5 s for 12B Q4 cold, then mmap'd
//! page faults on first inference). The context (KV cache, sampler state)
//! is cheap to build — so we keep the model resident and create a fresh
//! `LlamaContext` per `summarize` call. That keeps lifetimes simple and
//! prevents one session's KV cache from polluting another.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::OnceLock;

use hearsay_core::Segment;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::prompt;
use crate::SummarizeError;

/// One global llama backend — `LlamaBackend::init` is allowed exactly once
/// per process. `OnceLock` initializes it on first use and hands out a
/// `'static` reference forever after.
static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn backend() -> Result<&'static LlamaBackend, SummarizeError> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let b = LlamaBackend::init().map_err(|e| SummarizeError::Llama(format!("{e:?}")))?;
    Ok(BACKEND.get_or_init(|| b))
}

#[derive(Debug, Clone)]
pub struct SummarizerConfig {
    pub model_path: PathBuf,
    /// Context window in tokens. Gemma 3 supports up to 128 K but 32 K
    /// covers multi-hour meetings and uses less memory.
    pub n_ctx: u32,
    /// `0` means "use as many as you can"; tune down if hearsay is sharing
    /// the box with other latency-sensitive workloads.
    pub n_threads: i32,
    /// `999` = all model layers on GPU; `0` = pure CPU.
    pub n_gpu_layers: u32,
    /// Hard cap on generated tokens. Summaries should land comfortably
    /// under this; if they don't, increase or the prompt template is off.
    pub max_tokens: u32,
    pub seed: u32,
}

impl SummarizerConfig {
    pub fn for_model(path: PathBuf) -> Self {
        Self {
            model_path: path,
            n_ctx: 32_768,
            n_threads: 0,
            n_gpu_layers: 999,
            max_tokens: 1_500,
            seed: 42,
        }
    }
}

pub struct Summarizer {
    model: LlamaModel,
    config: SummarizerConfig,
}

impl Summarizer {
    pub fn new(config: SummarizerConfig) -> Result<Self, SummarizeError> {
        if !config.model_path.exists() {
            return Err(SummarizeError::ModelMissing {
                path: config.model_path.display().to_string(),
            });
        }
        let backend = backend()?;
        let model_params = LlamaModelParams::default().with_n_gpu_layers(config.n_gpu_layers);
        let model = LlamaModel::load_from_file(backend, &config.model_path, &model_params)
            .map_err(|e| SummarizeError::Llama(format!("load_from_file: {e:?}")))?;
        Ok(Self { model, config })
    }

    /// Blocking. Run on a `spawn_blocking` task from async contexts.
    pub fn summarize(
        &self,
        segments: &[Segment],
        language: Option<&str>,
    ) -> Result<String, SummarizeError> {
        if segments.iter().all(|s| s.text.trim().is_empty()) {
            return Err(SummarizeError::EmptyTranscript);
        }

        let p = prompt::build(segments, language);
        let template = self
            .model
            .chat_template(None)
            .map_err(|e| SummarizeError::Llama(format!("chat_template: {e:?}")))?;
        let messages = vec![
            LlamaChatMessage::new("system".into(), p.system)
                .map_err(|e| SummarizeError::Llama(format!("system message: {e:?}")))?,
            LlamaChatMessage::new("user".into(), p.user)
                .map_err(|e| SummarizeError::Llama(format!("user message: {e:?}")))?,
        ];
        let prompt_text = self
            .model
            .apply_chat_template(&template, &messages, true)
            .map_err(|e| SummarizeError::Llama(format!("apply_chat_template: {e:?}")))?;

        let backend = backend()?;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.config.n_ctx))
            .with_n_threads(self.config.n_threads);
        let mut ctx = self
            .model
            .new_context(backend, ctx_params)
            .map_err(|e| SummarizeError::Llama(format!("new_context: {e:?}")))?;

        let tokens = self
            .model
            .str_to_token(&prompt_text, AddBos::Always)
            .map_err(|e| SummarizeError::Llama(format!("str_to_token: {e:?}")))?;

        let prompt_len: i32 = tokens.len() as i32;
        let mut batch = LlamaBatch::new(self.config.n_ctx as usize, 1);
        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch
                .add(*token, i as i32, &[0], is_last)
                .map_err(|e| SummarizeError::Llama(format!("batch.add prompt: {e:?}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| SummarizeError::Llama(format!("decode prompt: {e:?}")))?;

        // Sampling chain: top-p prunes obvious garbage, temp adds a touch
        // of variety, dist picks the final token. Keep it deterministic-ish
        // for summaries — a higher temp tends to invent action items.
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::top_p(0.95, 1),
            LlamaSampler::temp(0.3),
            LlamaSampler::dist(self.config.seed),
        ]);

        let mut output = String::new();
        let mut n_cur = prompt_len;

        for _ in 0..self.config.max_tokens {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                break;
            }

            // Per-token decoder is fine — Gemma's SentencePiece tokens are
            // self-contained byte sequences that decode to valid UTF-8.
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|e| SummarizeError::Llama(format!("token_to_piece: {e:?}")))?;
            output.push_str(&piece);

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| SummarizeError::Llama(format!("batch.add token: {e:?}")))?;
            ctx.decode(&mut batch)
                .map_err(|e| SummarizeError::Llama(format!("decode token: {e:?}")))?;
            n_cur += 1;
        }

        Ok(output)
    }
}
