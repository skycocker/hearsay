use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::SessionId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
    pub session_id: SessionId,
    /// Identifies which model produced this summary, e.g. "gemma3-12b-q4".
    pub model: String,
    /// Markdown body.
    pub content: String,
    pub generated_at: DateTime<Utc>,
}
