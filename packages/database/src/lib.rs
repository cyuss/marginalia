//! SQLite storage.
//!
//! WAL, foreign keys on, forward-only numbered migrations. WHY forward-only:
//! a down-migration on a user's only copy of their annotation history is a
//! data-loss mechanism wearing a helpful hat. Recovery is a restore from
//! backup, not a reverse migration.

pub mod migrations;
pub mod repositories;

use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration {version} failed: {source}")]
    Migration {
        version: u32,
        #[source]
        source: rusqlite::Error,
    },

    #[error("database is at version {found}, which is newer than this build supports ({supported}). Upgrade Marginalia.")]
    SchemaTooNew { found: u32, supported: u32 },
}

pub type DbResult<T> = Result<T, DbError>;

/// Open a database and bring it up to date.
pub fn open(path: &str) -> DbResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrations::apply_all(&conn)?;
    Ok(conn)
}

/// An in-memory database, for tests.
pub fn open_in_memory() -> DbResult<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrations::apply_all(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> DbResult<()> {
    // WHY each of these:
    //   foreign_keys  — SQLite defaults them OFF; without this, every REFERENCES
    //                   in our schema is decoration.
    //   journal_mode  — WAL lets the UI read while a sync writes.
    //   synchronous   — NORMAL is the right trade with WAL; FULL costs more than
    //                   it buys us here, and we never rely on the DB alone for
    //                   the integrity of a user's files.
    //   busy_timeout  — a sync and a UI query racing should wait, not fail.
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    // journal_mode returns a row, so it cannot go in execute_batch on all
    // versions; query it instead.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_keys_are_actually_enforced() {
        // The failure this guards against is silent: SQLite accepts orphan rows
        // happily if the pragma is off.
        let conn = open_in_memory().unwrap();
        let enabled: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(enabled, 1);

        let err = conn.execute(
            "INSERT INTO document_mapping
               (id, local_document_id, original_filename, original_checksum,
                device_state, created_at, updated_at)
             VALUES ('m1', 'no-such-document', 'x.pdf', 'abc', 'METADATA_ONLY',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        );
        assert!(err.is_err(), "an orphan mapping must be rejected");
    }
}
