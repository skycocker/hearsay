//! Wire encoding for the small enums we persist as TEXT columns.

use hearsay_core::{SessionStatus, SourceKind};

use crate::StorageError;

pub(crate) fn source_kind_to_sql(k: SourceKind) -> &'static str {
    match k {
        SourceKind::Mic => "mic",
        SourceKind::SystemAudio => "system_audio",
        SourceKind::Meet => "meet",
    }
}

pub(crate) fn source_kind_from_sql(s: &str) -> Result<SourceKind, StorageError> {
    match s {
        "mic" => Ok(SourceKind::Mic),
        "system_audio" => Ok(SourceKind::SystemAudio),
        "meet" => Ok(SourceKind::Meet),
        other => Err(StorageError::CorruptEnum {
            column: "source_kind",
            value: other.to_owned(),
        }),
    }
}

pub(crate) fn status_to_sql(s: SessionStatus) -> &'static str {
    match s {
        SessionStatus::Active => "active",
        SessionStatus::Completed => "completed",
        SessionStatus::Failed => "failed",
    }
}

pub(crate) fn status_from_sql(s: &str) -> Result<SessionStatus, StorageError> {
    match s {
        "active" => Ok(SessionStatus::Active),
        "completed" => Ok(SessionStatus::Completed),
        "failed" => Ok(SessionStatus::Failed),
        other => Err(StorageError::CorruptEnum {
            column: "status",
            value: other.to_owned(),
        }),
    }
}
