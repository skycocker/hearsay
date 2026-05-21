//! Summarization via local Gemma 3 (default 12B Q4) through llama.cpp.
//!
//! Implementation lands in task #7.

#[derive(Debug, thiserror::Error)]
pub enum SummarizeError {
    #[error("model `{model}` is English-only; cannot summarize {language:?}")]
    LanguageNotSupported { model: String, language: String },
    #[error("not yet implemented")]
    Unimplemented,
}
