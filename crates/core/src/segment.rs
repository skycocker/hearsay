use serde::{Deserialize, Serialize};

use crate::SessionId;

/// A single transcribed slice of speech inside a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub session_id: SessionId,
    /// Monotonically increasing index inside the session.
    pub seq: u32,
    /// Milliseconds since the start of the session.
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
    /// ISO-639-1 language code if Whisper attached one.
    pub language: Option<String>,
    pub confidence: Option<f32>,
    /// Filled in by the diarization step; `None` during the live transcript.
    pub speaker_id: Option<u32>,
}
