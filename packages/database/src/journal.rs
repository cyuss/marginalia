//! The activity journal.
//!
//! Every job and every operation inside it lands here, so the Activity view can
//! answer the question a user actually asks: *what did this program do?*
//!
//! Two properties the schema gives us for free, and this module relies on:
//!
//! - a `TRANSFER`, `REMOVAL` or `ZOTERO_EXPORT` job with a non-`USER` trigger is
//!   rejected by a `CHECK` constraint, so a scheduler cannot start one even by
//!   mistake;
//! - `sync_operation.idempotency_key` is `UNIQUE`, so recording the same
//!   operation twice fails rather than duplicating — which is what makes a
//!   replayed request a no-op instead of a second download.

use marginalia_core::clock::{Clock, SYSTEM_CLOCK};
use marginalia_core::ids::{SyncJobId, SyncOperationId};
use marginalia_core::sync::{JobTrigger, SyncJobKind, SyncJobState};
use rusqlite::{params, Connection};

use crate::{DbError, DbResult};

fn kind_str(kind: SyncJobKind) -> &'static str {
    match kind {
        SyncJobKind::ZoteroMetadata => "ZOTERO_METADATA",
        SyncJobKind::DeviceScan => "DEVICE_SCAN",
        SyncJobKind::AnnotationIngest => "ANNOTATION_INGEST",
        SyncJobKind::Transfer => "TRANSFER",
        SyncJobKind::Removal => "REMOVAL",
        SyncJobKind::ZoteroExport => "ZOTERO_EXPORT",
        SyncJobKind::TagBridge => "TAG_BRIDGE",
    }
}

fn trigger_str(trigger: JobTrigger) -> &'static str {
    match trigger {
        JobTrigger::User => "USER",
        JobTrigger::Schedule => "SCHEDULE",
        JobTrigger::Startup => "STARTUP",
    }
}

fn state_str(state: SyncJobState) -> &'static str {
    match state {
        SyncJobState::Created => "CREATED",
        SyncJobState::Planned => "PLANNED",
        SyncJobState::Rejected => "REJECTED",
        SyncJobState::Running => "RUNNING",
        SyncJobState::Cancelling => "CANCELLING",
        SyncJobState::Cancelled => "CANCELLED",
        SyncJobState::Completed => "COMPLETED",
        SyncJobState::CompletedWithWarnings => "COMPLETED_WITH_WARNINGS",
        SyncJobState::Failed => "FAILED",
        SyncJobState::RollingBack => "ROLLING_BACK",
        SyncJobState::RolledBack => "ROLLED_BACK",
        SyncJobState::RollbackFailed => "ROLLBACK_FAILED",
    }
}

/// One line of the Activity view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    pub job_id: SyncJobId,
    pub kind: String,
    pub state: String,
    pub triggered_by: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub counters: Option<String>,
    pub error: Option<String>,
}

pub struct Journal<'a> {
    conn: &'a Connection,
    clock: &'a dyn Clock,
}

impl<'a> Journal<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            clock: &SYSTEM_CLOCK,
        }
    }

    pub fn with_clock(conn: &'a Connection, clock: &'a dyn Clock) -> Self {
        Self { conn, clock }
    }

    /// Open a job. Returns its id.
    ///
    /// The trigger is checked here as well as by the schema. Two guards for the
    /// same rule is not redundancy when the rule is "a background timer must
    /// never move a user's files".
    pub fn begin(&self, kind: SyncJobKind, trigger: JobTrigger) -> DbResult<SyncJobId> {
        if !kind.may_be_triggered_by(trigger) {
            return Err(DbError::Sqlite(rusqlite::Error::InvalidParameterName(
                format!("{kind:?} may not be triggered by {trigger:?}"),
            )));
        }

        let id = SyncJobId::new();
        self.conn.execute(
            "INSERT INTO sync_job (id, kind, state, triggered_by, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.as_str(),
                kind_str(kind),
                state_str(SyncJobState::Running),
                trigger_str(trigger),
                self.clock.now().to_rfc3339()
            ],
        )?;
        Ok(id)
    }

    /// Close a job with its outcome.
    pub fn finish(
        &self,
        job: &SyncJobId,
        state: SyncJobState,
        counters: Option<&str>,
        error: Option<&str>,
    ) -> DbResult<()> {
        self.conn.execute(
            "UPDATE sync_job
                SET state = ?2, finished_at = ?3, counters = ?4, error = ?5
              WHERE id = ?1",
            params![
                job.as_str(),
                state_str(state),
                self.clock.now().to_rfc3339(),
                counters,
                error
            ],
        )?;
        Ok(())
    }

    /// Record one operation.
    ///
    /// Returns `Ok(false)` when the idempotency key has been seen before — the
    /// work is already done, and doing it again is the thing we are preventing.
    /// That is a normal outcome, not an error.
    pub fn record(
        &self,
        job: &SyncJobId,
        seq: u32,
        kind: &str,
        target: Option<&str>,
        idempotency_key: &str,
    ) -> DbResult<bool> {
        let now = self.clock.now().to_rfc3339();
        let result = self.conn.execute(
            "INSERT INTO sync_operation
               (id, sync_job_id, seq, kind, target_ref, state, idempotency_key,
                attempted_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'DONE', ?6, ?7, ?7)",
            params![
                SyncOperationId::new().as_str(),
                job.as_str(),
                seq,
                kind,
                target,
                idempotency_key,
                now
            ],
        );

        match result {
            Ok(_) => Ok(true),
            // A unique-constraint violation here means "already done", which is
            // the mechanism working rather than failing.
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Ok(false)
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Whether an operation with this key has already completed.
    pub fn already_done(&self, idempotency_key: &str) -> DbResult<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sync_operation WHERE idempotency_key = ?1",
            params![idempotency_key],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// The most recent jobs, newest first, for the Activity view.
    pub fn recent(&self, limit: u32) -> DbResult<Vec<JournalEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, state, triggered_by, started_at, finished_at, counters, error
               FROM sync_job ORDER BY started_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(JournalEntry {
                    job_id: SyncJobId::from_string(r.get::<_, String>(0)?),
                    kind: r.get(1)?,
                    state: r.get(2)?,
                    triggered_by: r.get(3)?,
                    started_at: r.get(4)?,
                    finished_at: r.get(5)?,
                    counters: r.get(6)?,
                    error: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    #[test]
    fn a_job_opens_and_closes() {
        let conn = open_in_memory().unwrap();
        let journal = Journal::new(&conn);

        let job = journal
            .begin(SyncJobKind::ZoteroMetadata, JobTrigger::User)
            .unwrap();
        journal
            .finish(&job, SyncJobState::Completed, Some("{\"items\":12}"), None)
            .unwrap();

        let recent = journal.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].state, "COMPLETED");
        assert!(recent[0].finished_at.is_some());
        assert_eq!(recent[0].counters.as_deref(), Some("{\"items\":12}"));
    }

    #[test]
    fn a_schedule_may_open_a_metadata_sync() {
        let conn = open_in_memory().unwrap();
        assert!(Journal::new(&conn)
            .begin(SyncJobKind::ZoteroMetadata, JobTrigger::Schedule)
            .is_ok());
    }

    /// The rule, guarded here and again by the schema.
    #[test]
    fn a_schedule_may_not_open_a_transfer() {
        let conn = open_in_memory().unwrap();
        for kind in [
            SyncJobKind::Transfer,
            SyncJobKind::Removal,
            SyncJobKind::ZoteroExport,
        ] {
            assert!(
                Journal::new(&conn)
                    .begin(kind, JobTrigger::Schedule)
                    .is_err(),
                "{kind:?} must not be startable by a timer"
            );
            assert!(
                Journal::new(&conn)
                    .begin(kind, JobTrigger::Startup)
                    .is_err(),
                "{kind:?} must not be startable at startup"
            );
        }
    }

    #[test]
    fn recording_the_same_operation_twice_is_reported_not_duplicated() {
        let conn = open_in_memory().unwrap();
        let journal = Journal::new(&conn);
        let job = journal
            .begin(SyncJobKind::ZoteroMetadata, JobTrigger::User)
            .unwrap();

        assert!(journal
            .record(&job, 1, "UPSERT", Some("AAA"), "sync:AAA:1")
            .unwrap());
        assert!(
            !journal
                .record(&job, 2, "UPSERT", Some("AAA"), "sync:AAA:1")
                .unwrap(),
            "a repeat must report 'already done' rather than duplicate"
        );

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_operation", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn already_done_answers_before_the_work_is_attempted() {
        // How a replayed request becomes a no-op instead of a second download.
        let conn = open_in_memory().unwrap();
        let journal = Journal::new(&conn);
        let job = journal
            .begin(SyncJobKind::Transfer, JobTrigger::User)
            .unwrap();

        assert!(!journal
            .already_done("form:GEN1:0:DownloadToDevice")
            .unwrap());
        journal
            .record(
                &job,
                1,
                "DOWNLOAD",
                Some("doc-1"),
                "form:GEN1:0:DownloadToDevice",
            )
            .unwrap();
        assert!(journal
            .already_done("form:GEN1:0:DownloadToDevice")
            .unwrap());
    }

    #[test]
    fn a_failed_job_records_why() {
        let conn = open_in_memory().unwrap();
        let journal = Journal::new(&conn);
        let job = journal
            .begin(SyncJobKind::ZoteroMetadata, JobTrigger::User)
            .unwrap();

        journal
            .finish(
                &job,
                SyncJobState::Failed,
                None,
                Some("could not reach Zotero"),
            )
            .unwrap();

        let recent = journal.recent(1).unwrap();
        assert_eq!(recent[0].state, "FAILED");
        assert_eq!(recent[0].error.as_deref(), Some("could not reach Zotero"));
    }

    #[test]
    fn the_activity_view_is_newest_first() {
        let conn = open_in_memory().unwrap();
        let base = chrono::TimeZone::timestamp_opt(&chrono::Utc, 1_700_000_000, 0).unwrap();

        for (offset, _) in [(0i64, ()), (60, ()), (120, ())] {
            let clock =
                marginalia_core::clock::FixedClock::at(base + chrono::Duration::seconds(offset));
            Journal::with_clock(&conn, &clock)
                .begin(SyncJobKind::ZoteroMetadata, JobTrigger::User)
                .unwrap();
        }

        let recent = Journal::new(&conn).recent(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert!(
            recent[0].started_at > recent[2].started_at,
            "the Activity view reads newest first"
        );
    }

    #[test]
    fn the_limit_is_honoured() {
        let conn = open_in_memory().unwrap();
        let journal = Journal::new(&conn);
        for _ in 0..5 {
            journal
                .begin(SyncJobKind::ZoteroMetadata, JobTrigger::User)
                .unwrap();
        }
        assert_eq!(journal.recent(2).unwrap().len(), 2);
    }
}
