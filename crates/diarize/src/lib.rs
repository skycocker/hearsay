//! Offline speaker diarization (pyannote-segmentation + speaker-embedding
//! models via the `ort` ONNX runtime). Runs at the end of a session and
//! populates `segments.speaker_id` by overlapping its turns with the
//! whisper-produced segments.
//!
//! Implementation lands in task #14.

#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    #[error("model `{0}` not found")]
    ModelMissing(String),
    #[error("not yet implemented")]
    Unimplemented,
}

/// A contiguous stretch of speech attributed to a single speaker.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub start_ms: u32,
    pub end_ms: u32,
    pub speaker_id: u32,
}
