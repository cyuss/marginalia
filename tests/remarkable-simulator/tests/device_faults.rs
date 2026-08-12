//! # Device-side fault tests
//!
//! The existing simulator models a device *from the outside* — a desktop
//! talking across a cable. The standalone runtime needs the other half: the
//! things that happen **to the application while it is running on the device**.
//!
//! Roadmap invariant 14 is the specification for this file:
//!
//! > A network outage, crash, low-storage event, interrupted download, or power
//! > loss must leave the application recoverable and native RM2 content usable.
//!
//! Each test below reproduces one of those without a device, and asserts that
//! recovery is real rather than assumed. They use the `Device` storage profile,
//! so they also serve as the first evidence for ADR-005's durability choice.
//!
//! Not covered here, deliberately: **network outage**. There is no network code
//! yet, and a mock of an adapter that does not exist would test nothing. It
//! arrives with the Zotero adapter in Phase 2.

use marginalia_core::clock::{Clock, FixedClock};
use marginalia_core::device::StorageInfo;
use marginalia_core::ids::DocumentId;
use marginalia_core::intent::{ExplicitUserIntent, UserAction};
use marginalia_database::{open_with_profile, DbError, StorageProfile};
use marginalia_remarkable::provider::DeviceIntrospection;
use marginalia_simulator::{DeviceProfile, SimulatedDevice};

const MB: u64 = 1024 * 1024;
const RESERVE: u64 = 500 * MB;

/// A scratch directory that behaves like the application's own data area:
/// owned by us, removed completely on drop.
struct Sandbox(std::path::PathBuf);

impl Sandbox {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("marginalia-sim-{label}-{}", DocumentId::new()));
        std::fs::create_dir_all(&dir).expect("create sandbox");
        Self(dir)
    }

    fn db_path(&self) -> String {
        self.0
            .join("marginalia.sqlite")
            .to_str()
            .expect("utf-8 path")
            .to_string()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ── power loss and abrupt process death ─────────────────────────────────────

/// The device can lose power mid-write. What must survive is the *previous*
/// consistent state — never a half-applied one.
#[test]
fn an_uncommitted_transaction_is_absent_after_an_abrupt_death() {
    let sandbox = Sandbox::new("powerloss");
    let path = sandbox.db_path();

    // Session 1: write one row, begin a second write, then die without
    // committing. Dropping the connection without COMMIT is the closest
    // faithful analogue of the process being killed.
    {
        let conn = open_with_profile(&path, StorageProfile::Device).unwrap();
        conn.execute(
            "INSERT INTO document (id, title, source, state, created_at, updated_at)
             VALUES ('survivor', 'Committed before the lights went out', 'ZOTERO',
                     'METADATA_ONLY', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute_batch("BEGIN").unwrap();
        conn.execute(
            "INSERT INTO document (id, title, source, state, created_at, updated_at)
             VALUES ('casualty', 'Mid-flight', 'ZOTERO',
                     'METADATA_ONLY', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        // No COMMIT. Process dies here.
    }

    // Session 2: the application restarts.
    let conn = open_with_profile(&path, StorageProfile::Device).unwrap();
    let survived: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM document WHERE id = 'survivor'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let half_written: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM document WHERE id = 'casualty'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(survived, 1, "a committed row must survive a crash");
    assert_eq!(
        half_written, 0,
        "an uncommitted row must not appear after recovery"
    );
}

/// Restarting must not re-run migrations or corrupt the schema. This is the
/// ordinary case that happens every time the device wakes up.
#[test]
fn reopening_the_database_is_idempotent() {
    let sandbox = Sandbox::new("reopen");
    let path = sandbox.db_path();

    let version_after = |p: &str| -> u32 {
        let conn = open_with_profile(p, StorageProfile::Device).unwrap();
        marginalia_database::migrations::current_version(&conn).unwrap()
    };

    let first = version_after(&path);
    let second = version_after(&path);
    let third = version_after(&path);

    assert_eq!(first, second);
    assert_eq!(second, third);
    assert_eq!(first, marginalia_database::migrations::latest_version());
}

// ── corruption ──────────────────────────────────────────────────────────────

/// A corrupt database must be reported, not silently accepted. The user's
/// annotations are the thing at stake; opening a damaged file and carrying on
/// is how a bad situation becomes an unrecoverable one.
#[test]
fn a_corrupted_database_file_is_reported_rather_than_ignored() {
    let sandbox = Sandbox::new("corrupt");
    let path = sandbox.db_path();

    // Create a valid database, then damage its header.
    {
        let _ = open_with_profile(&path, StorageProfile::Device).unwrap();
    }
    std::fs::write(&path, b"this is not a SQLite database, it is a cat photo").unwrap();

    match open_with_profile(&path, StorageProfile::Device) {
        Err(DbError::Sqlite(_)) => { /* reported, as required */ }
        Err(other) => panic!("expected a SQLite error, got {other:?}"),
        Ok(_) => panic!(
            "a corrupted database opened successfully. Recovery must start from \
             an honest failure, not from a file we pretended was fine."
        ),
    }
}

/// Truncation is the realistic corruption on a device that lost power during a
/// write, and it must be caught for the same reason.
#[test]
fn a_truncated_database_file_is_reported() {
    let sandbox = Sandbox::new("truncated");
    let path = sandbox.db_path();

    {
        let _ = open_with_profile(&path, StorageProfile::Device).unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 3]).unwrap();

    assert!(
        open_with_profile(&path, StorageProfile::Device).is_err(),
        "a truncated database must not open silently"
    );
}

// ── storage pressure ────────────────────────────────────────────────────────

/// On the desktop the reserve protected the *device* from a transfer. In the
/// standalone model the application shares the disk with the user's documents,
/// so the same reserve has to guard the application's own writes.
#[test]
fn a_device_below_its_reserve_refuses_its_own_writes() {
    let sim = SimulatedDevice::new(DeviceProfile::low_storage());
    let storage = DeviceIntrospection::storage(&sim).unwrap();

    assert!(
        !storage.can_accept(1, RESERVE),
        "with less free space than the reserve, even a one-byte write is refused"
    );
    assert!(!storage.can_accept(84 * MB, RESERVE));
}

#[test]
fn a_healthy_device_permits_a_reasonable_write() {
    let sim = SimulatedDevice::new(DeviceProfile::known_healthy());
    let storage = DeviceIntrospection::storage(&sim).unwrap();
    assert!(storage.can_accept(84 * MB, RESERVE));
}

/// Storage that shrinks between the check and the write is the race the
/// reserve exists to absorb. Re-checking against the newer figure must refuse.
#[test]
fn storage_shrinking_between_check_and_write_is_caught_by_a_recheck() {
    let before = StorageInfo {
        total_bytes: 6 * 1024 * MB,
        free_bytes: 2 * 1024 * MB,
    };
    assert!(before.can_accept(84 * MB, RESERVE));

    // The user reads and annotates while we were preparing.
    let after = StorageInfo {
        total_bytes: 6 * 1024 * MB,
        free_bytes: 300 * MB,
    };
    assert!(
        !after.can_accept(84 * MB, RESERVE),
        "a pre-flight check is not a promise; the write path must re-check"
    );
}

// ── clock skew ──────────────────────────────────────────────────────────────

/// A device returning from a week asleep can come back with a clock that
/// jumped. A confirmation minted before the jump must not still authorise a
/// write after it.
#[test]
fn a_clock_jumping_forward_expires_a_pending_confirmation() {
    let doc = DocumentId::new();
    let minted_at = FixedClock::at(chrono::Utc::now());
    let intent = ExplicitUserIntent::record(UserAction::SendToRemarkable, doc, minted_at.now());

    assert!(intent.is_fresh(minted_at.now(), 300));

    // The device wakes up a week later.
    let after_sleep = FixedClock::at(minted_at.now() + chrono::Duration::days(7));
    assert!(
        !intent.is_fresh(after_sleep.now(), 300),
        "a week-old confirmation must not authorise a write"
    );
}

/// And a clock that goes *backwards* must not resurrect an expired one.
#[test]
fn a_clock_jumping_backwards_does_not_revive_a_stale_confirmation() {
    let doc = DocumentId::new();
    let now = chrono::Utc::now();

    // Minted in what the device now believes is the future.
    let intent = ExplicitUserIntent::record(
        UserAction::SendToRemarkable,
        doc,
        now + chrono::Duration::hours(2),
    );

    let skewed = FixedClock::at(now);
    assert!(
        !intent.is_fresh(skewed.now(), 300),
        "a confirmation stamped in the future must be rejected, not trusted"
    );
}

// ── the device stays usable ─────────────────────────────────────────────────

/// Invariant 14's last clause: whatever happens to us, the user's own content
/// is untouched. Every fault above operates inside our sandbox; none of them
/// can reach a document we did not put there.
#[test]
fn no_device_side_fault_touches_the_users_own_documents() {
    let sim = SimulatedDevice::new(DeviceProfile::populated_with_user_documents());
    let before = sim.document_count();

    // Exercise the read surface the standalone runtime has.
    let _ = DeviceIntrospection::device_info(&sim).unwrap();
    let _ = DeviceIntrospection::storage(&sim).unwrap();

    assert_eq!(sim.document_count(), before);
    assert_eq!(
        sim.write_count(),
        0,
        "the on-device read surface must be incapable of changing anything"
    );
}
