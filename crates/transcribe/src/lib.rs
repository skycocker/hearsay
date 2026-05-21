//! Real-time transcription via whisper.cpp with VAD-based chunking.
//!
//! Implementation lands in task #6.

#[derive(Debug, thiserror::Error)]
pub enum TranscribeError {
    #[error("not yet implemented")]
    Unimplemented,
}
