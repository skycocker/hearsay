//! Persistence for hearsay: SQLite metadata + streaming Opus audio writer.
//!
//! Implementation lands in task #5.

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not yet implemented")]
    Unimplemented,
}
