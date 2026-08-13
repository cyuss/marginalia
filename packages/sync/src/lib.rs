//! The sync use case.
//!
//! This is the application layer: it composes the pure planner from
//! `marginalia-zotero`, the atomic applier and journal from
//! `marginalia-database`, and a `ZoteroClient` it is handed. It owns no rules
//! of its own — its job is to run the loop in the right order and stop safely
//! when something goes wrong.
//!
//! The crate exists now, and did not earlier, because there is finally a real
//! seam: two collaborators that must not know about each other, and a consumer
//! (the on-device agent) that must not know about either.
//!
//! # The order, and why it is that order
//!
//! ```text
//! read cursor
//!   └─► fetch page ──► plan ──► apply (one transaction) ──► record in journal
//!          ▲                                                      │
//!          └──────────────── more pages? ─────────────────────────┘
//!                                  │ no
//!                                  ▼
//!                          commit cursor  ← only here, only once
//! ```
//!
//! The cursor moves last and only once. Every other ordering loses data on a
//! crash, quietly.

use marginalia_core::sync::{JobTrigger, SyncJobKind, SyncJobState};
use marginalia_database::journal::Journal;
use marginalia_database::sync_apply::{MetadataApplier, SyncStateRepository};
use marginalia_zotero::credentials::ZoteroCredentials;
use marginalia_zotero::sync::{SyncCursor, SyncPlanner, SyncTally};
use marginalia_zotero::{ZoteroClient, ZoteroError};
use rusqlite::Connection;
use thiserror::Error;

/// How many items to request per page.
///
/// Zotero permits more, but a device on a slow connection is better served by
/// smaller, more frequent commits: an interruption then costs less work.
pub const PAGE_SIZE: u32 = 50;

/// A ceiling on pages per run, so a first sync of an enormous library cannot
/// run unbounded on a battery-powered device. Hitting it is not a failure; the
/// cursor is unchanged and the next run continues.
pub const MAX_PAGES_PER_RUN: u32 = 200;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Zotero(#[from] ZoteroError),

    #[error(transparent)]
    Database(#[from] marginalia_database::DbError),
}

impl SyncError {
    /// Whether trying again later could plausibly work.
    pub fn is_transient(&self) -> bool {
        match self {
            SyncError::Zotero(e) => e.is_transient(),
            SyncError::Database(_) => false,
        }
    }

    /// What to tell the user.
    pub fn user_message(&self) -> String {
        match self {
            SyncError::Zotero(e) => e.user_message().to_string(),
            SyncError::Database(_) => {
                "Marginalia could not write to its own database. Your library \
                 and annotations were not changed."
                    .into()
            }
        }
    }
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub tally: SyncTally,
    /// `true` when the run reached the last page and moved the cursor.
    pub completed: bool,
    /// Version the library is now synced to, if the run completed.
    pub synced_to: Option<i64>,
    /// Set when the page ceiling stopped the run. Not an error.
    pub stopped_at_page_limit: bool,
    /// Folders written to the database — not merely seen.
    pub collections_stored: usize,
}

impl SyncReport {
    /// The line the user sees. Always names the transfer count, including —
    /// especially — when it is zero.
    pub fn summary(&self) -> String {
        format!(
            "{} folders · {} items · {} attachments · {} deletions · {} PDFs transferred",
            self.collections_stored,
            self.tally.items_seen,
            self.tally.attachments_seen,
            self.tally.deletions,
            self.tally.pdfs_transferred
        )
    }
}

/// Runs a metadata sync.
pub struct SyncRunner<'a> {
    conn: &'a Connection,
    client: &'a dyn ZoteroClient,
}

impl<'a> SyncRunner<'a> {
    pub fn new(conn: &'a Connection, client: &'a dyn ZoteroClient) -> Self {
        Self { conn, client }
    }

    /// Synchronise one library's metadata.
    ///
    /// Never transfers a file. It cannot: the only thing it hands the database
    /// is a `MetadataOperation`, and that type has no variant that can express
    /// one.
    /// Mirror the library's folders, then resolve their parents.
    ///
    /// Returns how many rows the database actually holds afterwards — not how
    /// many were seen. A summary that counts what went past rather than what
    /// was kept is how a sync reports success while storing nothing, which is
    /// precisely what this one did before folders were carried.
    fn sync_collections(
        &self,
        credentials: &ZoteroCredentials,
        planner: &SyncPlanner,
        applier: &MetadataApplier,
    ) -> Result<usize, SyncError> {
        let mut start = 0u32;
        let mut links = Vec::new();

        loop {
            let page = self
                .client
                .fetch_collections(credentials, start, PAGE_SIZE)?;

            // The planner owns the mapping, here as everywhere else.
            applier.apply(&planner.plan_collections(&page).operations)?;

            links.extend(
                page.collections
                    .iter()
                    .map(|c| (c.key.clone(), c.parent_key.clone())),
            );

            match page.next_start {
                Some(next) => start = next,
                None => break,
            }
        }

        // Second pass: every folder now exists, so a parent can be resolved
        // whatever page it arrived on.
        applier.link_collection_parents(&links)?;
        Ok(links.len())
    }

    pub fn run(
        &self,
        credentials: &ZoteroCredentials,
        trigger: JobTrigger,
    ) -> Result<SyncReport, SyncError> {
        let library = credentials.library().clone();
        let library_key = library.base_path().trim_start_matches('/').to_string();

        let journal = Journal::new(self.conn);
        let state = SyncStateRepository::new(self.conn);
        let applier = MetadataApplier::new(self.conn);
        let planner = SyncPlanner::new();

        let job = journal.begin(SyncJobKind::ZoteroMetadata, trigger)?;

        let cursor = SyncCursor {
            library,
            last_version: state.version_for(&library_key)?,
        };

        // Folders first. They are few, they are cheap, and a tree that arrives
        // before the items it contains is still useful; the reverse is not.
        let collections_stored = self.sync_collections(credentials, &planner, &applier)?;

        match self.pump(&cursor, credentials, &planner, &applier, &journal, &job) {
            Ok((tally, reached_end, library_version, hit_limit)) => {
                // The cursor moves here and nowhere else, after every page of
                // this run has been committed.
                let synced_to = if reached_end {
                    state.commit_version(&library_key, library_version)?;
                    Some(library_version)
                } else {
                    None
                };

                let report = SyncReport {
                    tally,
                    completed: reached_end,
                    synced_to,
                    stopped_at_page_limit: hit_limit,
                    collections_stored,
                };

                journal.finish(
                    &job,
                    if reached_end {
                        SyncJobState::Completed
                    } else {
                        SyncJobState::CompletedWithWarnings
                    },
                    Some(&report.summary()),
                    None,
                )?;
                Ok(report)
            }
            Err(e) => {
                // The cursor has not moved, so this run is resumable from
                // exactly where it stopped.
                journal.finish(&job, SyncJobState::Failed, None, Some(&e.user_message()))?;
                Err(e)
            }
        }
    }

    /// The page loop. Returns (tally, reached the end, library version, hit the ceiling).
    #[allow(clippy::type_complexity)]
    fn pump(
        &self,
        cursor: &SyncCursor,
        credentials: &ZoteroCredentials,
        planner: &SyncPlanner,
        applier: &MetadataApplier<'_>,
        journal: &Journal<'_>,
        job: &marginalia_core::ids::SyncJobId,
    ) -> Result<(SyncTally, bool, i64, bool), SyncError> {
        let mut tally = SyncTally::default();
        let mut start = 0u32;
        let mut pages = 0u32;
        // Assigned from the first page before anything reads it; the loop
        // always runs at least once.
        let mut library_version;

        loop {
            let page = self
                .client
                .fetch_items(credentials, cursor, start, PAGE_SIZE)?;

            let plan = planner.plan_page(&page);
            // A page applies entirely or not at all. On failure the cursor is
            // untouched, so the next run re-requests this same page.
            applier.apply(&plan.operations)?;

            tally.record_page(&page);
            library_version = page.library_version;
            pages += 1;

            journal.record(
                job,
                pages,
                "ZOTERO_PAGE",
                Some(&cursor.library.base_path()),
                &format!(
                    "sync:{}:{}:{}",
                    cursor.library.base_path(),
                    cursor.last_version,
                    start
                ),
            )?;

            match page.next_start {
                None => return Ok((tally, true, library_version, false)),
                Some(next) => {
                    if pages >= MAX_PAGES_PER_RUN {
                        // Stop cleanly rather than run a battery flat. The
                        // cursor stays put and the next run picks up here.
                        return Ok((tally, false, library_version, true));
                    }
                    start = next;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalia_core::credentials::{CredentialKey, CredentialStore};
    use marginalia_core::secret::Redacted;
    use marginalia_core::zotero::AttachmentAvailability;
    use marginalia_database::open_in_memory;
    use marginalia_zotero::credentials::LibraryRef;
    use marginalia_zotero::sync::{ItemPage, RemoteItem};
    use marginalia_zotero::{KeyDescription, KeyVerification};
    use std::cell::RefCell;

    /// A client that serves a scripted sequence of pages, and can be made to
    /// fail at a chosen one.
    struct ScriptedClient {
        pages: RefCell<Vec<Result<ItemPage, ZoteroError>>>,
        requests: RefCell<Vec<(i64, u32)>>,
    }

    impl ScriptedClient {
        fn new(pages: Vec<Result<ItemPage, ZoteroError>>) -> Self {
            Self {
                pages: RefCell::new(pages),
                requests: RefCell::new(Vec::new()),
            }
        }
        fn request_count(&self) -> usize {
            self.requests.borrow().len()
        }
    }

    impl ZoteroClient for ScriptedClient {
        fn fetch_collections(
            &self,
            _c: &ZoteroCredentials,
            _start: u32,
            _limit: u32,
        ) -> Result<marginalia_zotero::sync::CollectionPage, ZoteroError> {
            // No folders in these fixtures: they exercise item paging and the
            // cursor, and inventing a tree here would test the stub.
            Ok(marginalia_zotero::sync::CollectionPage {
                collections: Vec::new(),
                library_version: 0,
                next_start: None,
            })
        }

        fn verify(&self, _c: &ZoteroCredentials) -> Result<KeyVerification, ZoteroError> {
            unreachable!("sync does not verify")
        }
        fn describe_key(&self, _k: &Redacted<String>) -> Result<KeyDescription, ZoteroError> {
            unreachable!("sync does not describe")
        }
        fn fetch_items(
            &self,
            _c: &ZoteroCredentials,
            cursor: &SyncCursor,
            start: u32,
            _limit: u32,
        ) -> Result<ItemPage, ZoteroError> {
            self.requests
                .borrow_mut()
                .push((cursor.last_version, start));
            let mut pages = self.pages.borrow_mut();
            if pages.is_empty() {
                return Err(ZoteroError::Protocol("no more scripted pages".into()));
            }
            pages.remove(0)
        }
    }

    fn creds() -> ZoteroCredentials {
        ZoteroCredentials::new(
            Redacted::new("aaaaaaaaaaaaaaaaaaaaaaaa".into()),
            LibraryRef::user("12345"),
        )
    }

    fn item(k: &str) -> RemoteItem {
        RemoteItem {
            key: marginalia_core::ids::ZoteroKey::from_string(k),
            version: 1,
            item_type: "journalArticle".into(),
            is_pdf_attachment: false,
            availability: AttachmentAvailability::Unknown,
            tags: Vec::new(),
        }
    }

    fn pdf(k: &str) -> RemoteItem {
        RemoteItem {
            key: marginalia_core::ids::ZoteroKey::from_string(k),
            version: 1,
            item_type: "attachment".into(),
            is_pdf_attachment: true,
            availability: AttachmentAvailability::AvailableLocal,
            tags: Vec::new(),
        }
    }

    fn page(items: Vec<RemoteItem>, version: i64, next: Option<u32>) -> ItemPage {
        ItemPage {
            items,
            library_version: version,
            next_start: next,
        }
    }

    #[test]
    fn a_single_page_sync_completes_and_moves_the_cursor() {
        let conn = open_in_memory().unwrap();
        let client = ScriptedClient::new(vec![Ok(page(vec![item("A"), item("B")], 900, None))]);

        let report = SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        assert!(report.completed);
        assert_eq!(report.synced_to, Some(900));
        assert_eq!(report.tally.items_seen, 2);
        assert_eq!(report.tally.pdfs_transferred, 0);

        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            900
        );
    }

    #[test]
    fn pagination_follows_next_start() {
        let conn = open_in_memory().unwrap();
        let client = ScriptedClient::new(vec![
            Ok(page(vec![item("A")], 900, Some(50))),
            Ok(page(vec![item("B")], 900, Some(100))),
            Ok(page(vec![item("C")], 900, None)),
        ]);

        let report = SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        assert!(report.completed);
        assert_eq!(report.tally.pages, 3);
        assert_eq!(report.tally.items_seen, 3);
        assert_eq!(
            *client.requests.borrow(),
            vec![(0, 0), (0, 50), (0, 100)],
            "each request continues from the previous page's offset"
        );
    }

    /// The property the whole ordering exists for.
    #[test]
    fn a_failure_midway_leaves_the_cursor_untouched() {
        let conn = open_in_memory().unwrap();
        let client = ScriptedClient::new(vec![
            Ok(page(vec![item("A")], 900, Some(50))),
            Err(ZoteroError::Network("connection lost".into())),
        ]);

        let err = SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::User)
            .unwrap_err();
        assert!(err.is_transient());

        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            0,
            "a partial run must not claim the library is synced"
        );

        // The first page's rows are there, and re-applying them is safe.
        let items: i64 = conn
            .query_row("SELECT COUNT(*) FROM zotero_item", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 1);
    }

    #[test]
    fn a_resumed_run_asks_from_the_same_version() {
        let conn = open_in_memory().unwrap();

        // First run fails on page two.
        let failing = ScriptedClient::new(vec![
            Ok(page(vec![item("A")], 900, Some(50))),
            Err(ZoteroError::Network("dropped".into())),
        ]);
        let _ = SyncRunner::new(&conn, &failing).run(&creds(), JobTrigger::User);

        // Second run starts from `since=0` again, because the cursor never moved.
        let retry = ScriptedClient::new(vec![Ok(page(vec![item("A"), item("B")], 900, None))]);
        let report = SyncRunner::new(&conn, &retry)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        assert_eq!(*retry.requests.borrow(), vec![(0, 0)]);
        assert!(report.completed);

        // "A" was applied twice across the two runs and exists once.
        let items: i64 = conn
            .query_row("SELECT COUNT(*) FROM zotero_item", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 2);
    }

    #[test]
    fn a_second_sync_continues_from_the_stored_version() {
        let conn = open_in_memory().unwrap();

        let first = ScriptedClient::new(vec![Ok(page(vec![item("A")], 900, None))]);
        SyncRunner::new(&conn, &first)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        let second = ScriptedClient::new(vec![Ok(page(vec![item("B")], 950, None))]);
        SyncRunner::new(&conn, &second)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        assert_eq!(
            *second.requests.borrow(),
            vec![(900, 0)],
            "the second run asks only for what changed since 900"
        );
        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            950
        );
    }

    /// Safety test S8/S9 at the use-case level: the whole loop, on a library
    /// where every item is a downloadable PDF.
    #[test]
    fn a_full_sync_of_downloadable_pdfs_transfers_nothing() {
        let conn = open_in_memory().unwrap();
        let items: Vec<_> = (0..40).map(|i| pdf(&format!("KEY{i:05}"))).collect();
        let client = ScriptedClient::new(vec![Ok(page(items, 900, None))]);

        let report = SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::Schedule)
            .unwrap();

        assert_eq!(report.tally.attachments_seen, 40);
        assert_eq!(report.tally.pdfs_transferred, 0);
        assert!(report.summary().contains("0 PDFs transferred"));
    }

    #[test]
    fn the_page_ceiling_stops_cleanly_without_claiming_completion() {
        let conn = open_in_memory().unwrap();
        // Always another page: an enormous first sync.
        let endless: Vec<_> = (0..(MAX_PAGES_PER_RUN + 5))
            .map(|i| Ok(page(vec![item("X")], 900, Some((i + 1) * PAGE_SIZE))))
            .collect();
        let client = ScriptedClient::new(endless);

        let report = SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::Schedule)
            .unwrap();

        assert!(report.stopped_at_page_limit);
        assert!(!report.completed);
        assert_eq!(report.synced_to, None);
        assert_eq!(report.tally.pages, MAX_PAGES_PER_RUN);
        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            0,
            "stopping at the ceiling is not completing"
        );
    }

    #[test]
    fn every_run_is_recorded_in_the_journal() {
        let conn = open_in_memory().unwrap();
        let client = ScriptedClient::new(vec![Ok(page(vec![item("A")], 900, None))]);
        SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        let recent = Journal::new(&conn).recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].kind, "ZOTERO_METADATA");
        assert_eq!(recent[0].state, "COMPLETED");
        assert!(recent[0].counters.as_deref().unwrap().contains("0 PDFs"));
    }

    #[test]
    fn a_failed_run_is_recorded_with_a_readable_reason() {
        let conn = open_in_memory().unwrap();
        let client = ScriptedClient::new(vec![Err(ZoteroError::Unauthorized)]);
        let err = SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::User)
            .unwrap_err();

        assert!(!err.is_transient());

        let recent = Journal::new(&conn).recent(1).unwrap();
        assert_eq!(recent[0].state, "FAILED");
        let reason = recent[0].error.as_deref().unwrap();
        assert!(reason.contains("API key"), "got: {reason}");
        assert!(!reason.contains("401"), "protocol detail must not leak");
    }

    #[test]
    fn a_scheduled_metadata_sync_is_allowed() {
        // The counterpart of the rule: unattended syncing is fine precisely
        // because it cannot move files.
        let conn = open_in_memory().unwrap();
        let client = ScriptedClient::new(vec![Ok(page(vec![], 900, None))]);
        assert!(SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::Schedule)
            .is_ok());
    }

    #[test]
    fn credentials_are_never_written_by_a_sync() {
        // A sync reads a key; it has no business storing one.
        let conn = open_in_memory().unwrap();
        let store = marginalia_core::credentials::InMemoryCredentialStore::new();
        let client = ScriptedClient::new(vec![Ok(page(vec![item("A")], 900, None))]);

        SyncRunner::new(&conn, &client)
            .run(&creds(), JobTrigger::User)
            .unwrap();

        assert_eq!(
            store.load(CredentialKey::ZoteroApiKey).unwrap().map(|_| ()),
            None
        );
        assert_eq!(client.request_count(), 1);
    }
}
