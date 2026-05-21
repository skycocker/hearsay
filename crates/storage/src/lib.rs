//! Persistence for hearsay: SQLite metadata + streaming audio writer.

mod audio_writer;
mod db;
mod enums;
mod migrations;

pub use audio_writer::{AudioWriter, TARGET_SAMPLE_RATE, WavAudioWriter};
pub use db::Storage;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("hound (wav): {0}")]
    Hound(#[from] hound::Error),
    #[error("invalid session id: {0}")]
    InvalidSessionId(#[from] ulid::DecodeError),
    #[error("corrupt enum in column `{column}`: {value:?}")]
    CorruptEnum {
        column: &'static str,
        value: String,
    },
    #[error("audio writer already finalized")]
    AlreadyFinalized,
}
