//! Versioned, forward-only migrations.
//!
//! Migrations are embedded in the binary rather than read from disk: a
//! shipped app must not depend on files a user could move, and the schema a
//! build expects should be part of that build.

use marginalia_core::clock::{Clock, SYSTEM_CLOCK};
use rusqlite::Connection;

use crate::{DbError, DbResult};

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Every migration, in order. Append only.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../migrations/0001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "zotero_sync_state",
        sql: include_str!("../migrations/0002_zotero_sync_state.sql"),
    },
];

pub fn latest_version() -> u32 {
    MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
}

pub fn current_version(conn: &Connection) -> DbResult<u32> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           name TEXT NOT NULL,
           applied_at TEXT NOT NULL
         );",
    )?;
    let version: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    Ok(version)
}

/// Apply every migration the database has not yet seen.
///
/// Each migration runs inside a transaction together with the bookkeeping row,
/// so a crash mid-migration leaves the database at the previous version rather
/// than in a half-migrated state that nothing knows how to interpret.
pub fn apply_all(conn: &Connection) -> DbResult<()> {
    apply_all_with_clock(conn, &SYSTEM_CLOCK)
}

/// As [`apply_all`], with an explicit clock so a device test can pin the
/// `applied_at` stamps and reproduce clock skew.
pub fn apply_all_with_clock(conn: &Connection, clock: &dyn Clock) -> DbResult<()> {
    let current = current_version(conn)?;
    let latest = latest_version();

    if current > latest {
        // An older build opening a newer database must refuse rather than
        // guess. Downgrading silently would corrupt data the old code cannot
        // represent.
        return Err(DbError::SchemaTooNew {
            found: current,
            supported: latest,
        });
    }

    for m in MIGRATIONS.iter().filter(|m| m.version > current) {
        conn.execute_batch("BEGIN")?;
        let result = conn.execute_batch(m.sql).and_then(|_| {
            conn.execute(
                "INSERT INTO schema_migrations (version, name, applied_at)
                     VALUES (?1, ?2, ?3)",
                rusqlite::params![m.version, m.name, clock.now().to_rfc3339()],
            )
            .map(|_| ())
        });

        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                tracing::info!(version = m.version, name = m.name, "migration applied");
            }
            Err(source) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(DbError::Migration {
                    version: m.version,
                    source,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    #[test]
    fn migrations_are_numbered_consecutively_from_one() {
        // Catches a duplicated or skipped version number at test time rather
        // than on a user's machine.
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                m.version,
                i as u32 + 1,
                "migration versions must be consecutive starting at 1"
            );
        }
    }

    #[test]
    fn a_fresh_database_reaches_the_latest_version() {
        let conn = open_in_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), latest_version());
    }

    #[test]
    fn applying_twice_is_a_no_op() {
        let conn = open_in_memory().unwrap();
        let before = current_version(&conn).unwrap();
        apply_all(&conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), before);
    }

    #[test]
    fn an_older_build_refuses_a_newer_database() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at)
             VALUES (9999, 'from-the-future', '2030-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        match apply_all(&conn) {
            Err(DbError::SchemaTooNew { found, .. }) => assert_eq!(found, 9999),
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }
}
