//! Schema migrations.
//!
//! Hand-rolled rather than depending on refinery so the migration list and
//! current version are visible at a glance and migrations can be tested in
//! isolation. Each migration runs in its own transaction; partial application
//! is not possible.

use rusqlite::Connection;

use crate::StorageError;

struct Migration {
    id: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    id: 1,
    sql: r"
        CREATE TABLE sessions (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            source_kind   TEXT NOT NULL,
            source_meta   TEXT NOT NULL,        -- JSON
            language      TEXT,                  -- ISO-639-1 or NULL
            audio_path    TEXT NOT NULL,
            started_at    INTEGER NOT NULL,      -- unix ms
            ended_at      INTEGER,
            status        TEXT NOT NULL
        );

        CREATE TABLE segments (
            session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            seq           INTEGER NOT NULL,
            start_ms      INTEGER NOT NULL,
            end_ms        INTEGER NOT NULL,
            text          TEXT NOT NULL,
            language      TEXT,
            confidence    REAL,
            speaker_id    INTEGER,
            PRIMARY KEY (session_id, seq)
        );
        CREATE INDEX idx_segments_session_start ON segments(session_id, start_ms);

        CREATE TABLE speakers (
            session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            speaker_id    INTEGER NOT NULL,
            label         TEXT,
            PRIMARY KEY (session_id, speaker_id)
        );

        CREATE TABLE summaries (
            session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            model         TEXT NOT NULL,
            content       TEXT NOT NULL,
            generated_at  INTEGER NOT NULL,
            PRIMARY KEY (session_id, model)
        );
    ",
}];

pub(crate) fn run(conn: &mut Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            id          INTEGER PRIMARY KEY,
            applied_at  INTEGER NOT NULL
        )",
    )?;

    let current: u32 = conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;

    for m in MIGRATIONS {
        if m.id > current {
            tracing::info!(migration_id = m.id, "applying migration");
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.execute(
                "INSERT INTO schema_version (id, applied_at) VALUES (?1, ?2)",
                rusqlite::params![m.id, chrono::Utc::now().timestamp_millis()],
            )?;
            tx.commit()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_gets_all_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        let version: u32 = conn
            .query_row("SELECT MAX(id) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().id);
    }

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        run(&mut conn).unwrap();
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, MIGRATIONS.len() as u32);
    }

    #[test]
    fn expected_tables_exist() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for expected in ["schema_version", "sessions", "segments", "speakers", "summaries"] {
            assert!(names.contains(&expected.to_string()), "missing table {expected}");
        }
    }
}
