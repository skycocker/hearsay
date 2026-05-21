//! Meeting summarization via local Gemma 3 (or any GGUF chat model) through
//! `llama.cpp`.
//!
//! Three pieces:
//!
//! 1. [`prompt`] — language-aware prompt construction. Pure functions, no
//!    llama dep, fully unit-testable.
//! 2. [`Summarizer`] — wraps a loaded `LlamaModel` and exposes a synchronous
//!    `summarize` call. Spin one up at daemon start when the model exists,
//!    keep it resident if `summarization.keep_loaded` is true.
//! 3. Daemon-side wiring (in `hearsayd::session_manager`) — after a session
//!    stops, fire-and-forget the summary into storage.

mod model;
pub mod prompt;

pub use model::{Summarizer, SummarizerConfig};

#[derive(Debug, thiserror::Error)]
pub enum SummarizeError {
    #[error("summarization model not found at `{path}`. Run scripts/setup-models.sh to fetch one.")]
    ModelMissing { path: String },
    #[error("model `{model}` is English-only (Gemma 3 1B); session language is {language:?}.")]
    LanguageNotSupported {
        model: String,
        language: String,
    },
    #[error("llama: {0}")]
    Llama(String),
    #[error("model path is not valid UTF-8: {0:?}")]
    NonUtf8Path(std::path::PathBuf),
    #[error("nothing to summarize: session has no transcript yet")]
    EmptyTranscript,
}
