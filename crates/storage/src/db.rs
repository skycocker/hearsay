//! Connection setup + the `Storage` handle that every other crate uses.
//!
//! Single-connection-behind-a-mutex is intentional: SQLite serializes writes
//! regardless, so a connection pool wouldn't gain throughput, and our access
//! pattern is low-write (~one insert per transcription segment, a few times
//! per second peak).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use hearsay_core::{Segment, SessionId, SessionMeta, SessionStatus, Speaker, Summary};

use crate::enums::{source_kind_from_sql, source_kind_to_sql, status_from_sql, status_to_sql};
use crate::{StorageError, migrations};

#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

impl Storage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        Self::configure(&conn)?;
        migrations::run(&mut conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Convenience for tests.
    pub fn in_memory() -> Result<Self, StorageError> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        migrations::run(&mut conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    fn configure(conn: &Connection) -> Result<(), StorageError> {
        // WAL gives us concurrent reads alongside writes, NORMAL trades a
        // small durability window for ~10x write throughput, foreign_keys
        // is OFF by default — turning it on is the whole point of having
        // ON DELETE CASCADE on segments/speakers/summaries.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        Ok(())
    }

    // ---- sessions ----------------------------------------------------------

    pub fn insert_session(&self, m: &SessionMeta) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO sessions (id, name, source_kind, source_meta, language, audio_path,
                                   started_at, ended_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                m.id.to_string(),
                m.name,
                source_kind_to_sql(m.source_kind),
                serde_json::to_string(&m.source_meta).unwrap_or_else(|_| "null".to_owned()),
                m.language,
                m.audio_path.to_string_lossy(),
                m.started_at.timestamp_millis(),
                m.ended_at.map(|t| t.timestamp_millis()),
                status_to_sql(m.status),
            ],
        )?;
        Ok(())
    }

    pub fn finish_session(
        &self,
        id: SessionId,
        status: SessionStatus,
        ended_at: DateTime<Utc>,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE sessions SET status = ?1, ended_at = ?2 WHERE id = ?3",
            params![status_to_sql(status), ended_at.timestamp_millis(), id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: SessionId) -> Result<Option<SessionMeta>, StorageError> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, name, source_kind, source_meta, language, audio_path,
                    started_at, ended_at, status
             FROM sessions WHERE id = ?1",
            params![id.to_string()],
            row_to_session,
        )
        .optional()
        .map_err(StorageError::from)?
        .transpose()
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>, StorageError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, source_kind, source_meta, language, audio_path,
                    started_at, ended_at, status
             FROM sessions
             ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_session)?;
        rows.collect::<Result<Result<Vec<_>, StorageError>, _>>()?
    }

    pub fn delete_session(&self, id: SessionId) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    // ---- segments ----------------------------------------------------------

    pub fn insert_segment(&self, s: &Segment) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO segments (session_id, seq, start_ms, end_ms, text, language,
                                   confidence, speaker_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                s.session_id.to_string(),
                s.seq,
                s.start_ms,
                s.end_ms,
                s.text,
                s.language,
                s.confidence,
                s.speaker_id,
            ],
        )?;
        Ok(())
    }

    pub fn set_segment_speaker(
        &self,
        session_id: SessionId,
        seq: u32,
        speaker_id: u32,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE segments SET speaker_id = ?1 WHERE session_id = ?2 AND seq = ?3",
            params![speaker_id, session_id.to_string(), seq],
        )?;
        Ok(())
    }

    pub fn list_segments(&self, session_id: SessionId) -> Result<Vec<Segment>, StorageError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT session_id, seq, start_ms, end_ms, text, language, confidence, speaker_id
             FROM segments WHERE session_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |r| {
            Ok(Segment {
                session_id: SessionId::default(), // overwritten below
                seq: r.get::<_, u32>(1)?,
                start_ms: r.get::<_, u32>(2)?,
                end_ms: r.get::<_, u32>(3)?,
                text: r.get::<_, String>(4)?,
                language: r.get::<_, Option<String>>(5)?,
                confidence: r.get::<_, Option<f32>>(6)?,
                speaker_id: r.get::<_, Option<u32>>(7)?,
            })
            .map(|mut seg| {
                seg.session_id = session_id;
                seg
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ---- speakers ----------------------------------------------------------

    pub fn upsert_speaker(&self, s: &Speaker) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO speakers (session_id, speaker_id, label)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id, speaker_id) DO UPDATE SET label = excluded.label",
            params![s.session_id.to_string(), s.speaker_id, s.label],
        )?;
        Ok(())
    }

    pub fn list_speakers(&self, session_id: SessionId) -> Result<Vec<Speaker>, StorageError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT speaker_id, label FROM speakers WHERE session_id = ?1 ORDER BY speaker_id",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |r| {
            Ok(Speaker {
                session_id,
                speaker_id: r.get(0)?,
                label: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // ---- summaries ---------------------------------------------------------

    pub fn upsert_summary(&self, s: &Summary) -> Result<(), StorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO summaries (session_id, model, content, generated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id, model) DO UPDATE SET
                 content = excluded.content,
                 generated_at = excluded.generated_at",
            params![
                s.session_id.to_string(),
                s.model,
                s.content,
                s.generated_at.timestamp_millis()
            ],
        )?;
        Ok(())
    }

    pub fn list_summaries(&self, session_id: SessionId) -> Result<Vec<Summary>, StorageError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT model, content, generated_at FROM summaries
             WHERE session_id = ?1 ORDER BY generated_at DESC",
        )?;
        let rows = stmt.query_map(params![session_id.to_string()], |r| {
            let ts: i64 = r.get(2)?;
            Ok(Summary {
                session_id,
                model: r.get(0)?,
                content: r.get(1)?,
                generated_at: Utc
                    .timestamp_millis_opt(ts)
                    .single()
                    .unwrap_or_else(Utc::now),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<SessionMeta, StorageError>> {
    let id_str: String = row.get(0)?;
    let source_kind_str: String = row.get(2)?;
    let source_meta_str: String = row.get(3)?;
    let audio_path: String = row.get(5)?;
    let started_ms: i64 = row.get(6)?;
    let ended_ms: Option<i64> = row.get(7)?;
    let status_str: String = row.get(8)?;

    Ok((|| {
        let id: SessionId = id_str
            .parse()
            .map_err(StorageError::InvalidSessionId)?;
        Ok(SessionMeta {
            id,
            name: row.get::<_, String>(1).map_err(StorageError::Sqlite)?,
            source_kind: source_kind_from_sql(&source_kind_str)?,
            source_meta: serde_json::from_str(&source_meta_str)
                .unwrap_or(serde_json::Value::Null),
            language: row.get::<_, Option<String>>(4).map_err(StorageError::Sqlite)?,
            audio_path: PathBuf::from(audio_path),
            started_at: Utc
                .timestamp_millis_opt(started_ms)
                .single()
                .unwrap_or_else(Utc::now),
            ended_at: ended_ms.and_then(|ms| Utc.timestamp_millis_opt(ms).single()),
            status: status_from_sql(&status_str)?,
        })
    })())
}
