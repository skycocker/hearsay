//! Real-time transcription via whisper.cpp.
//!
//! Three pieces stitched together:
//!
//! 1. [`Chunker`] (pure logic, no whisper dep) — accumulates 16 kHz mono
//!    PCM and emits `TranscribeRequest`s on fixed-size chunk boundaries.
//! 2. [`Transcriber`] — owns a `WhisperContext`, blocking `transcribe`
//!    call. Must be invoked from a non-async thread.
//! 3. [`TranscriptionWorker`] — runs a dedicated OS thread that pulls
//!    audio over a crossbeam channel and emits [`TranscribedSegment`]s
//!    via a tokio mpsc the caller can `recv().await` on.

mod chunker;
mod model;
mod worker;

pub use chunker::{Chunker, TranscribeRequest};
pub use model::{TranscribedSegment, Transcriber, TranscriberConfig};
pub use worker::{FeedError, TranscriptionWorker};

#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    #[error("whisper model not found at `{path}`. Run scripts/setup-models.sh to fetch one.")]
    ModelMissing { path: String },
    #[error("whisper error: {0}")]
    Whisper(String),
    #[error("model path is not valid UTF-8: {0:?}")]
    NonUtf8Path(std::path::PathBuf),
}
