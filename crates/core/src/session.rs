use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::SessionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Mic,
    SystemAudio,
    Meet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: SessionId,
    pub name: String,
    pub source_kind: SourceKind,
    /// Free-form JSON describing the source — device id, meet url, etc.
    pub source_meta: serde_json::Value,
    /// ISO-639-1 ("pl", "en") or None to auto-detect.
    pub language: Option<String>,
    pub audio_path: PathBuf,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SessionStatus,
}
