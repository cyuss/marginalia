//! SQLite storage.
//!
//! Foreign keys on, forward-only numbered migrations. WHY forward-only:
//! a down-migration on a user's only copy of their annotation history is a
//! data-loss mechanism wearing a helpful hat. Recovery is a restore from
//! backup, not a reverse migration.
//!
//! Durability and journalling are **not** fixed here: they are chosen by a
//! [`StorageProfile`], because the right answer differs between a workstation
//! and a battery-powered device that can lose power mid-write. See
//! `docs/adr/ADR-005-device-storage-profile.md`.

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

    #[error(
        "requested journal mode '{requested}' but SQLite reported '{actual}'. \
         The filesystem may not support it."
    )]
    JournalModeRefused { requested: String, actual: String },
}

pub type DbResult<T> = Result<T, DbError>;

/// Where this database lives, and therefore how careful it has to be.
///
/// WHY this is a parameter rather than a constant: WAL needs shared-memory
/// support from the filesystem, and `synchronous = NORMAL` trades durability
/// for speed. Both are reasonable on a workstation with a UI thread reading
/// while a sync writes. Neither is obviously safe on a device that holds the
/// user's only copy of their annotation history and can lose power without
/// warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageProfile {
    /// Desktop. WAL so the UI can read during a sync; `synchronous = NORMAL`.
    Workstation,
    /// On-device.
    ///
    /// Deliberately conservative until U12 is measured on real hardware:
    /// rollback journal (no shared-memory requirement) and `synchronous =
    /// FULL` (survives power loss). Fail closed applies to durability the same
    /// way it applies to device writes — we can make this faster once we have
    /// evidence, but we do not start fast and hope.
    Device,
}

impl StorageProfile {
    fn journal_mode(self) -> &'static str {
        match self {
            StorageProfile::Workstation => "WAL",
            StorageProfile::Device => "DELETE",
        }
    }

    fn synchronous(self) -> &'static str {
        match self {
            StorageProfile::Workstation => "NORMAL",
            StorageProfile::Device => "FULL",
        }
    }

    /// Whether a refusal by SQLite to grant the requested journal mode should
    /// abort the open.
    ///
    /// On a workstation, falling back from WAL costs concurrency and nothing
    /// else. On the device we asked for the durable mode on purpose; silently
    /// getting something else is exactly the kind of quiet degradation this
    /// codebase refuses elsewhere.
    fn journal_mode_is_mandatory(self) -> bool {
        matches!(self, StorageProfile::Device)
    }
}

/// Open a database and bring it up to date, using the workstation profile.
pub fn open(path: &str) -> DbResult<Connection> {
    open_with_profile(path, StorageProfile::Workstation)
}

/// Open a database with an explicit profile.
pub fn open_with_profile(path: &str, profile: StorageProfile) -> DbResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn, profile)?;
    migrations::apply_all(&conn)?;
    Ok(conn)
}

/// An in-memory database, for tests.
pub fn open_in_memory() -> DbResult<Connection> {
    open_in_memory_with_profile(StorageProfile::Workstation)
}

/// An in-memory database with an explicit profile.
///
/// Note that an in-memory database cannot use WAL; SQLite reports `memory`.
/// The workstation profile therefore tolerates the substitution here, which is
/// precisely why [`StorageProfile::journal_mode_is_mandatory`] exists.
pub fn open_in_memory_with_profile(profile: StorageProfile) -> DbResult<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn, profile)?;
    migrations::apply_all(&conn)?;
    Ok(conn)
}

/// The journal mode SQLite actually granted.
pub fn journal_mode(conn: &Connection) -> DbResult<String> {
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    Ok(mode)
}

fn configure(conn: &Connection, profile: StorageProfile) -> DbResult<()> {
    // foreign_keys — SQLite defaults them OFF; without this, every REFERENCES
    //                in our schema is decoration.
    // busy_timeout — a sync and a UI query racing should wait, not fail.
    conn.execute_batch(&format!(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = {};
         PRAGMA busy_timeout = 5000;",
        profile.synchronous()
    ))?;

    // `PRAGMA journal_mode = x` returns the mode actually adopted, which is not
    // always the one requested. Reading the answer rather than assuming it is
    // the whole point.
    let requested = profile.journal_mode();
    let actual: String =
        conn.query_row(&format!("PRAGMA journal_mode = {requested}"), [], |r| {
            r.get(0)
        })?;

    if profile.journal_mode_is_mandatory() && !actual.eq_ignore_ascii_case(requested) {
        return Err(DbError::JournalModeRefused {
            requested: requested.to_string(),
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The device profile refuses an in-memory database by design, so its tests
    /// need a real file. Removed on drop so a failing test cannot leak state
    /// into the next run.
    struct TempDb(std::path::PathBuf);

    impl TempDb {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "marginalia-{label}-{}.sqlite",
                marginalia_core::ids::DocumentId::new()
            ));
            Self(path)
        }
        fn path(&self) -> &str {
            self.0.to_str().expect("utf-8 temp path")
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            for suffix in ["-wal", "-shm", "-journal"] {
                let mut p = self.0.clone().into_os_string();
                p.push(suffix);
                let _ = std::fs::remove_file(std::path::PathBuf::from(p));
            }
        }
    }

    #[test]
    fn foreign_keys_are_enforced_under_every_profile() {
        // The failure this guards against is silent: SQLite accepts orphan rows
        // happily if the pragma is off.
        let temp = TempDb::new("fk");
        let cases = [
            (
                StorageProfile::Workstation,
                open_in_memory_with_profile(StorageProfile::Workstation).unwrap(),
            ),
            (
                StorageProfile::Device,
                open_with_profile(temp.path(), StorageProfile::Device).unwrap(),
            ),
        ];

        for (profile, conn) in cases {
            let enabled: i64 = conn
                .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
                .unwrap();
            assert_eq!(enabled, 1, "{profile:?}");

            let err = conn.execute(
                "INSERT INTO document_mapping
                   (id, local_document_id, original_filename, original_checksum,
                    device_state, created_at, updated_at)
                 VALUES ('m1', 'no-such-document', 'x.pdf', 'abc', 'METADATA_ONLY',
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            );
            assert!(
                err.is_err(),
                "an orphan mapping must be rejected ({profile:?})"
            );
        }
    }

    #[test]
    fn the_device_profile_chooses_durability_over_speed() {
        let temp = TempDb::new("durability");
        let conn = open_with_profile(temp.path(), StorageProfile::Device).unwrap();

        // 2 == FULL. Until U12 is measured, the device does not trade
        // durability for throughput.
        let sync: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync, 2, "device profile must use synchronous = FULL");

        // And the journal mode it actually got is the one it asked for.
        assert_eq!(journal_mode(&conn).unwrap().to_lowercase(), "delete");
    }

    #[test]
    fn the_workstation_profile_prefers_concurrency() {
        let conn = open_in_memory_with_profile(StorageProfile::Workstation).unwrap();
        // 1 == NORMAL.
        let sync: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sync, 1);
    }

    #[test]
    fn a_refused_journal_mode_is_reported_not_swallowed_on_the_device() {
        // An in-memory database cannot honour DELETE; SQLite reports `memory`.
        // That makes it a convenient stand-in for the real risk, which is a
        // device filesystem refusing the mode we asked for. The device profile
        // must surface that rather than proceed on a false assumption.
        let result = open_in_memory_with_profile(StorageProfile::Device);
        match result {
            Err(DbError::JournalModeRefused { requested, actual }) => {
                assert_eq!(requested, "DELETE");
                assert_eq!(actual.to_lowercase(), "memory");
            }
            Ok(_) => panic!(
                "expected the device profile to refuse a substituted journal mode; \
                 if SQLite now honours DELETE in memory, this test needs revisiting"
            ),
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn the_schema_is_identical_under_both_profiles() {
        // A profile chooses durability, never shape. If this ever diverges,
        // the two runtimes are no longer reading the same database.
        fn tables(conn: &Connection) -> Vec<String> {
            let mut stmt = conn
                .prepare(
                    "SELECT name FROM sqlite_master
                     WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                )
                .unwrap();
            let rows: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            rows
        }

        let workstation = open_in_memory_with_profile(StorageProfile::Workstation).unwrap();
        // The device profile refuses in-memory, so compare against a plain
        // connection that has run the same migrations.
        let plain = {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
            migrations::apply_all(&conn).unwrap();
            conn
        };

        assert_eq!(tables(&workstation), tables(&plain));
    }
}
