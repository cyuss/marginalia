//! Storing highlights, and telling you what is new.
//!
//! # Why this takes plain fields rather than the extractor's types
//!
//! `marginalia-annotations` knows the reMarkable's file formats; this crate
//! knows SQLite. Neither should have to change when the other does, and the
//! architecture test enforces that there is no edge between them. So the caller
//! — the agent, which already knows both — does the mapping, and what arrives
//! here is a record with no opinion about where it came from.
//!
//! # What this module will not do
//!
//! It never deletes. A highlight that vanishes from the device is marked
//! [`HighlightRepository::mark_gone`], not removed: someone's reading is not
//! the device's to retract, and a row is cheap.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

use marginalia_core::checksum::Checksum;
use marginalia_core::clock::{Clock, SystemClock};

use crate::DbResult;

/// One highlight, ready to be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct HighlightRecord {
    pub document_uuid: String,
    pub document_name: String,
    pub file_type: String,
    pub page_id: String,
    pub page_number: Option<u32>,
    pub start_offset: i64,
    pub length: i64,
    pub text: String,
    pub color: Option<i64>,
}

impl HighlightRecord {
    /// A stable identity for this passage.
    ///
    /// Content and position, not a counter: the same highlight must produce the
    /// same id on every run, or re-extraction duplicates the library. The
    /// document uuid is included so the same sentence highlighted in two
    /// documents stays two highlights.
    ///
    /// The page *number* is deliberately absent. It comes from the `.content`
    /// layout, which can become unreadable on a firmware this project has not
    /// seen; if it were part of the identity, an unreadable layout would turn
    /// every existing highlight into a new one.
    pub fn id(&self) -> String {
        let material = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.document_uuid, self.page_id, self.start_offset, self.length, self.text
        );
        Checksum::of_bytes(material.as_bytes()).as_str().to_string()
    }
}

/// What one extraction did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunSummary {
    pub documents_seen: usize,
    pub highlights_seen: usize,
    pub highlights_new: usize,
    pub documents_unreadable: usize,
}

pub struct HighlightRepository<'a> {
    conn: &'a Connection,
    clock: &'a dyn Clock,
}

impl<'a> HighlightRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            clock: &SystemClock,
        }
    }

    pub fn with_clock(conn: &'a Connection, clock: &'a dyn Clock) -> Self {
        Self { conn, clock }
    }

    /// Store an extraction, and report what had not been seen before.
    ///
    /// Idempotent: running it twice over unchanged files stores nothing new and
    /// reports zero new highlights. `last_seen_at` moves on every run, which is
    /// what makes disappearance detectable.
    ///
    /// # One transaction, for two separate reasons
    ///
    /// Correctness: an extraction is one observation of the device. Half of it
    /// committed is not a smaller observation, it is a wrong one — the run log
    /// would claim a count the highlight rows do not support.
    ///
    /// And speed, which on a reMarkable is not a nicety. The device profile
    /// uses `synchronous = FULL` on eMMC, so every implicit transaction is an
    /// fsync. Statement-per-transaction over the 2,624 highlights on the
    /// reference device meant roughly five thousand fsyncs and the command did
    /// not finish; inside one transaction it is a single flush at the end.
    pub fn record_extraction(
        &self,
        records: &[HighlightRecord],
        extraction_version: u32,
        documents_unreadable: usize,
    ) -> DbResult<RunSummary> {
        self.conn.execute_batch("BEGIN")?;
        match self.record_extraction_inner(records, extraction_version, documents_unreadable) {
            Ok(summary) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(summary)
            }
            Err(e) => {
                // Best effort: if the rollback itself fails the original error
                // is the more useful one to report.
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    fn record_extraction_inner(
        &self,
        records: &[HighlightRecord],
        extraction_version: u32,
        documents_unreadable: usize,
    ) -> DbResult<RunSummary> {
        let now = self.clock.now().to_rfc3339();
        let mut new_count = 0usize;

        for record in records {
            let id = record.id();
            let existed: Option<i64> = self
                .conn
                .query_row("SELECT 1 FROM highlight WHERE id = ?1", params![id], |r| {
                    r.get(0)
                })
                .optional()?;

            if existed.is_none() {
                new_count += 1;
            }

            // The name, page number and colour are refreshed on every run: a
            // renamed document or a firmware that starts reporting page numbers
            // should update the row, not create a second one. first_seen_at is
            // never overwritten -- that is the whole point of it.
            self.conn.execute(
                "INSERT INTO highlight (
                     id, document_uuid, document_name, file_type, page_id, page_number,
                     start_offset, length, text, color, extraction_version,
                     first_seen_at, last_seen_at, gone_from_device_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12,NULL)
                 ON CONFLICT(id) DO UPDATE SET
                     document_name      = excluded.document_name,
                     file_type          = excluded.file_type,
                     page_number        = excluded.page_number,
                     color              = excluded.color,
                     extraction_version = excluded.extraction_version,
                     last_seen_at       = excluded.last_seen_at,
                     gone_from_device_at = NULL",
                params![
                    id,
                    record.document_uuid,
                    record.document_name,
                    record.file_type,
                    record.page_id,
                    record.page_number,
                    record.start_offset,
                    record.length,
                    record.text,
                    record.color,
                    extraction_version,
                    now,
                ],
            )?;
        }

        let documents_seen = records
            .iter()
            .map(|r| r.document_uuid.as_str())
            .collect::<HashSet<_>>()
            .len();

        let summary = RunSummary {
            documents_seen,
            highlights_seen: records.len(),
            highlights_new: new_count,
            documents_unreadable,
        };

        self.conn.execute(
            "INSERT INTO extraction_run (
                 id, ran_at, extraction_version,
                 documents_seen, highlights_seen, highlights_new, documents_unreadable
             ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                Checksum::of_bytes(format!("{now}\u{1f}{}", records.len()).as_bytes())
                    .as_str()
                    .to_string(),
                now,
                extraction_version,
                summary.documents_seen as i64,
                summary.highlights_seen as i64,
                summary.highlights_new as i64,
                summary.documents_unreadable as i64,
            ],
        )?;

        Ok(summary)
    }

    /// Mark everything not in `present` as no longer on the device.
    ///
    /// Only for a run that read the whole store. Calling this after extracting
    /// one document would mark the rest of the library gone, so the caller has
    /// to be the one that knows.
    pub fn mark_gone(&self, present: &[HighlightRecord]) -> DbResult<usize> {
        self.conn.execute_batch("BEGIN")?;
        match self.mark_gone_inner(present) {
            Ok(marked) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(marked)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    fn mark_gone_inner(&self, present: &[HighlightRecord]) -> DbResult<usize> {
        let now = self.clock.now().to_rfc3339();
        let seen: HashSet<String> = present.iter().map(|r| r.id()).collect();

        let mut stmt = self
            .conn
            .prepare("SELECT id FROM highlight WHERE gone_from_device_at IS NULL")?;
        let stored: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;

        let mut marked = 0usize;
        for id in stored.iter().filter(|id| !seen.contains(*id)) {
            self.conn.execute(
                "UPDATE highlight SET gone_from_device_at = ?2 WHERE id = ?1",
                params![id, now],
            )?;
            marked += 1;
        }
        Ok(marked)
    }

    /// Highlights first seen after a timestamp, newest first.
    pub fn since(&self, rfc3339: &str) -> DbResult<Vec<StoredHighlight>> {
        let mut stmt = self.conn.prepare(
            "SELECT document_name, page_number, text, first_seen_at
             FROM highlight
             WHERE first_seen_at > ?1
             ORDER BY first_seen_at DESC, document_name, page_number",
        )?;
        let rows = stmt
            .query_map(params![rfc3339], |row| {
                Ok(StoredHighlight {
                    document_name: row.get(0)?,
                    page_number: row.get(1)?,
                    text: row.get(2)?,
                    first_seen_at: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// When the previous extraction ran, if there was one.
    pub fn previous_run_at(&self) -> DbResult<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT ran_at FROM extraction_run ORDER BY ran_at DESC LIMIT 1 OFFSET 1",
                [],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn total(&self) -> DbResult<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM highlight", [], |r| r.get(0))?)
    }

    pub fn gone_count(&self) -> DbResult<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM highlight WHERE gone_from_device_at IS NOT NULL",
            [],
            |r| r.get(0),
        )?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredHighlight {
    pub document_name: String,
    pub page_number: Option<u32>,
    pub text: String,
    pub first_seen_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        // open_in_memory already applies every migration; calling apply_all
        // again here is what "table highlight already exists" was telling us.
        crate::open_in_memory().unwrap()
    }

    fn record(uuid: &str, page: &str, start: i64, text: &str) -> HighlightRecord {
        HighlightRecord {
            document_uuid: uuid.into(),
            document_name: "A Book".into(),
            file_type: "pdf".into(),
            page_id: page.into(),
            page_number: Some(1),
            start_offset: start,
            length: text.len() as i64,
            text: text.into(),
            color: Some(1),
        }
    }

    #[test]
    fn a_first_extraction_stores_everything_as_new() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let records = vec![
            record("doc-a", "page-1", 0, "first"),
            record("doc-a", "page-1", 10, "second"),
        ];

        let summary = repo.record_extraction(&records, 1, 0).unwrap();
        assert_eq!(summary.highlights_seen, 2);
        assert_eq!(summary.highlights_new, 2);
        assert_eq!(summary.documents_seen, 1);
        assert_eq!(repo.total().unwrap(), 2);
    }

    /// The property the deterministic id exists for. Without it, a second run
    /// over 2,624 unchanged highlights would store 2,624 more.
    #[test]
    fn extracting_the_same_files_again_stores_nothing_new() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let records = vec![record("doc-a", "page-1", 0, "first")];

        repo.record_extraction(&records, 1, 0).unwrap();
        let second = repo.record_extraction(&records, 1, 0).unwrap();

        assert_eq!(second.highlights_new, 0);
        assert_eq!(repo.total().unwrap(), 1);
    }

    #[test]
    fn a_highlight_added_later_is_reported_as_new() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        repo.record_extraction(&[record("doc-a", "page-1", 0, "first")], 1, 0)
            .unwrap();

        let summary = repo
            .record_extraction(
                &[
                    record("doc-a", "page-1", 0, "first"),
                    record("doc-a", "page-1", 10, "second"),
                ],
                1,
                0,
            )
            .unwrap();

        assert_eq!(summary.highlights_new, 1);
        assert_eq!(repo.total().unwrap(), 2);
    }

    /// A renamed document must update the row, not fork it.
    #[test]
    fn renaming_a_document_updates_the_highlight_rather_than_duplicating_it() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let mut r = record("doc-a", "page-1", 0, "text");
        repo.record_extraction(&[r.clone()], 1, 0).unwrap();

        r.document_name = "A Book, renamed".into();
        let summary = repo.record_extraction(&[r], 1, 0).unwrap();

        assert_eq!(summary.highlights_new, 0);
        assert_eq!(repo.total().unwrap(), 1);
        let name: String = conn
            .query_row("SELECT document_name FROM highlight", [], |x| x.get(0))
            .unwrap();
        assert_eq!(name, "A Book, renamed");
    }

    /// Page numbers can become unavailable on an unknown firmware. If they were
    /// part of a highlight's identity, that would silently duplicate the entire
    /// library on the next run.
    #[test]
    fn losing_page_numbers_does_not_duplicate_the_library() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let mut r = record("doc-a", "page-1", 0, "text");
        repo.record_extraction(std::slice::from_ref(&r), 1, 0)
            .unwrap();

        r.page_number = None;
        let summary = repo.record_extraction(&[r], 1, 0).unwrap();

        assert_eq!(summary.highlights_new, 0);
        assert_eq!(repo.total().unwrap(), 1);
    }

    #[test]
    fn the_same_sentence_in_two_documents_is_two_highlights() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let summary = repo
            .record_extraction(
                &[
                    record("doc-a", "page-1", 0, "the same sentence"),
                    record("doc-b", "page-1", 0, "the same sentence"),
                ],
                1,
                0,
            )
            .unwrap();
        assert_eq!(summary.highlights_new, 2);
    }

    /// Nothing is ever deleted. A highlight removed on the device is marked,
    /// and the text stays readable.
    #[test]
    fn a_highlight_removed_on_the_device_is_marked_not_destroyed() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let kept = record("doc-a", "page-1", 0, "kept");
        let removed = record("doc-a", "page-1", 10, "removed later");
        repo.record_extraction(&[kept.clone(), removed], 1, 0)
            .unwrap();

        let marked = repo.mark_gone(&[kept]).unwrap();

        assert_eq!(marked, 1);
        assert_eq!(repo.total().unwrap(), 2, "nothing was deleted");
        assert_eq!(repo.gone_count().unwrap(), 1);

        let text: String = conn
            .query_row(
                "SELECT text FROM highlight WHERE gone_from_device_at IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(text, "removed later");
    }

    /// Highlighting it again should un-mark it rather than leave a lie.
    #[test]
    fn a_highlight_that_comes_back_is_no_longer_marked_gone() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        let r = record("doc-a", "page-1", 0, "text");
        repo.record_extraction(std::slice::from_ref(&r), 1, 0)
            .unwrap();
        repo.mark_gone(&[]).unwrap();
        assert_eq!(repo.gone_count().unwrap(), 1);

        repo.record_extraction(&[r], 1, 0).unwrap();
        assert_eq!(repo.gone_count().unwrap(), 0);
    }

    #[test]
    fn what_is_new_can_be_asked_for_by_time() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        repo.record_extraction(&[record("doc-a", "page-1", 0, "old")], 1, 0)
            .unwrap();

        let all = repo.since("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text, "old");

        let none = repo.since("2999-01-01T00:00:00Z").unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn the_previous_run_is_only_known_once_there_have_been_two() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        assert!(repo.previous_run_at().unwrap().is_none());

        repo.record_extraction(&[], 1, 0).unwrap();
        assert!(repo.previous_run_at().unwrap().is_none());

        repo.record_extraction(&[], 1, 0).unwrap();
        assert!(repo.previous_run_at().unwrap().is_some());
    }

    /// A run that could not read ten documents must not look like a clean one.
    /// An extraction is one observation. A failure part-way through must leave
    /// no rows and no run log, rather than a run that claims a count the
    /// highlight table cannot support.
    #[test]
    fn a_failed_extraction_leaves_nothing_behind() {
        let conn = db();

        {
            let repo = HighlightRepository::new(&conn);
            repo.record_extraction(&[record("doc-a", "page-1", 0, "already here")], 1, 0)
                .unwrap();
        }

        // Force the second run to fail at the very end: the run log's primary
        // key is derived from the timestamp and the record count, so inserting
        // the same shape twice within the same clock tick collides.
        let frozen = "2026-08-13T10:00:00Z".parse().unwrap();
        let fixed = marginalia_core::clock::FixedClock::at(frozen);
        let repo = HighlightRepository::with_clock(&conn, &fixed);
        repo.record_extraction(&[record("doc-b", "page-1", 0, "first attempt")], 1, 0)
            .unwrap();

        let before = repo.total().unwrap();
        let result =
            repo.record_extraction(&[record("doc-c", "page-1", 0, "second attempt")], 1, 0);

        assert!(result.is_err(), "the colliding run should have failed");
        assert_eq!(
            repo.total().unwrap(),
            before,
            "a failed run committed rows anyway"
        );
    }

    #[test]
    fn unreadable_documents_are_recorded_on_the_run() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        repo.record_extraction(&[record("doc-a", "page-1", 0, "x")], 1, 10)
            .unwrap();

        let unreadable: i64 = conn
            .query_row("SELECT documents_unreadable FROM extraction_run", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(unreadable, 10);
    }

    #[test]
    fn the_extraction_version_is_stored_so_rows_can_be_re_extracted_later() {
        let conn = db();
        let repo = HighlightRepository::new(&conn);
        repo.record_extraction(&[record("doc-a", "page-1", 0, "x")], 7, 0)
            .unwrap();
        let version: i64 = conn
            .query_row("SELECT extraction_version FROM highlight", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 7);
    }
}
