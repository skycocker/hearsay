use serde::{Deserialize, Serialize};

use crate::SessionId;

/// A speaker discovered by the diarization step. `label` starts as `None`
/// (frontend renders "Speaker N"); the user can override it from the UI later.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Speaker {
    pub session_id: SessionId,
    pub speaker_id: u32,
    pub label: Option<String>,
}
