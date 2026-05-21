//! Thin wrapper around `whisper-rs::WhisperContext`.
//!
//! The actual `WhisperContext` is expensive to build (model file load +
//! Metal context setup), so the worker owns one for the lifetime of a
//! session. Each `transcribe` call creates a fresh decode state — whisper
//! states aren't safe to reuse across decodes.

use std::path::PathBuf;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::TranscribeError;

#[derive(Debug, Clone)]
pub struct TranscriberConfig {
    pub model_path: PathBuf,
    /// ISO-639-1 language code, or `None` to let whisper auto-detect.
    pub language: Option<String>,
    /// Threads whisper.cpp uses internally. Apple Silicon perf cores
    /// usually want 4-6; more buys little because Metal handles the heavy
    /// math. 0 lets whisper pick.
    pub n_threads: i32,
}

#[derive(Debug, Clone)]
pub struct TranscribedSegment {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
    /// Whisper attaches a language to each segment in auto-detect mode;
    /// `None` when the model was started with an explicit language.
    pub language: Option<String>,
}

pub struct Transcriber {
    ctx: WhisperContext,
    config: TranscriberConfig,
}

impl Transcriber {
    pub fn new(config: TranscriberConfig) -> Result<Self, TranscribeError> {
        if !config.model_path.exists() {
            return Err(TranscribeError::ModelMissing {
                path: config.model_path.display().to_string(),
            });
        }
        let model_str = config
            .model_path
            .to_str()
            .ok_or_else(|| TranscribeError::NonUtf8Path(config.model_path.clone()))?;
        let ctx = WhisperContext::new_with_params(model_str, WhisperContextParameters::default())
            .map_err(|e| TranscribeError::Whisper(format!("loading model: {e:?}")))?;
        Ok(Self { ctx, config })
    }

    /// Synchronous. Decodes `pcm` (16 kHz mono `f32`) and returns segments
    /// whose timestamps are shifted by `offset_ms` so they're
    /// session-relative.
    pub fn transcribe(
        &self,
        pcm: &[f32],
        offset_ms: u32,
    ) -> Result<Vec<TranscribedSegment>, TranscribeError> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| TranscribeError::Whisper(format!("create_state: {e:?}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // Whisper logs each step to stdout by default — noisy in a daemon.
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        if let Some(lang) = self.config.language.as_deref() {
            params.set_language(Some(lang));
        }
        params.set_n_threads(self.config.n_threads);
        // We hand timestamps back to the daemon ourselves — disable the
        // internal token-level timestamps to save work.
        params.set_token_timestamps(false);

        state
            .full(params, pcm)
            .map_err(|e| TranscribeError::Whisper(format!("decode: {e:?}")))?;

        let n_segments = state
            .full_n_segments()
            .map_err(|e| TranscribeError::Whisper(format!("n_segments: {e:?}")))?;

        let mut out = Vec::with_capacity(n_segments as usize);
        for i in 0..n_segments {
            let text = state
                .full_get_segment_text(i)
                .map_err(|e| TranscribeError::Whisper(format!("segment {i} text: {e:?}")))?;
            let start_cs = state
                .full_get_segment_t0(i)
                .map_err(|e| TranscribeError::Whisper(format!("segment {i} t0: {e:?}")))?;
            let end_cs = state
                .full_get_segment_t1(i)
                .map_err(|e| TranscribeError::Whisper(format!("segment {i} t1: {e:?}")))?;

            // whisper.cpp reports times in centiseconds (10 ms units).
            let local_start_ms = (start_cs * 10).max(0) as u32;
            let local_end_ms = (end_cs * 10).max(0) as u32;

            out.push(TranscribedSegment {
                start_ms: offset_ms.saturating_add(local_start_ms),
                end_ms: offset_ms.saturating_add(local_end_ms),
                text: text.trim().to_owned(),
                language: None,
            });
        }
        Ok(out)
    }
}
