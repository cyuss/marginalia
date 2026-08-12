//! Incremental metadata synchronisation.
//!
//! Implements the algorithm in `docs/zotero/ZOTERO_SYNC_MODEL.md` §4, as pure
//! functions over values. The network lives in [`crate::http`]; everything that
//! *decides* — what to fetch next, what to write, when to retry, when it is
//! safe to advance the watermark — is here, and is tested without a network.
//!
//! # The rule this module cannot break
//!
//! [`SyncPlanner`] produces [`MetadataOperation`]s, and that enum has no
//! variant capable of moving a file. A sync can record that an attachment
//! exists; it cannot fetch one. See `marginalia_core::sync`.

use std::time::Duration;

use marginalia_core::ids::ZoteroKey;
use marginalia_core::sync::{MetadataOperation, SyncJobKind, SyncPlan};
use marginalia_core::zotero::AttachmentAvailability;

use crate::credentials::LibraryRef;

/// How far a library has been synced.
///
/// Zotero versions a library monotonically: everything changed since version
/// `N` can be requested in one query. This is that `N`, plus the library it
/// belongs to — a cursor from one library must never be used against another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCursor {
    pub library: LibraryRef,
    /// `0` means "never synced": fetch everything.
    pub last_version: i64,
}

impl SyncCursor {
    pub fn initial(library: LibraryRef) -> Self {
        Self {
            library,
            last_version: 0,
        }
    }

    pub fn is_first_sync(&self) -> bool {
        self.last_version == 0
    }

    /// The query for the next request.
    pub fn query(&self, start: u32, limit: u32) -> String {
        format!(
            "{}/items?since={}&start={}&limit={}",
            self.library.base_path(),
            self.last_version,
            start,
            limit
        )
    }
}

/// One item as Zotero reported it. Metadata only — there is nowhere in this
/// struct for file bytes to live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteItem {
    pub key: ZoteroKey,
    pub version: i64,
    pub item_type: String,
    /// Set for attachments whose content type is a PDF. Records a fact; the
    /// file is not read, copied, or fetched.
    pub is_pdf_attachment: bool,
    /// Whether the file is present on this machine. `Unknown` until resolved
    /// locally, which is a `stat`, not a download.
    pub availability: AttachmentAvailability,
}

/// One page of results, plus what Zotero said about the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemPage {
    pub items: Vec<RemoteItem>,
    /// `Last-Modified-Version` for the whole library at the time of the
    /// request. This — not the highest item version — is what the watermark
    /// eventually becomes.
    pub library_version: i64,
    /// Offset for the next request, from the `Link: rel="next"` header.
    /// `None` means this was the last page.
    pub next_start: Option<u32>,
}

impl ItemPage {
    pub fn is_last(&self) -> bool {
        self.next_start.is_none()
    }
}

/// Keys Zotero reports as deleted since a version.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeletedKeys {
    pub items: Vec<ZoteroKey>,
    pub collections: Vec<ZoteroKey>,
}

/// Turns pages into operations. Pure: no I/O, no clock, no randomness.
#[derive(Debug, Default)]
pub struct SyncPlanner;

impl SyncPlanner {
    pub fn new() -> Self {
        Self
    }

    /// Plan the writes for one page.
    pub fn plan_page(&self, page: &ItemPage) -> SyncPlan {
        let mut ops = Vec::with_capacity(page.items.len() * 2);

        for item in &page.items {
            ops.push(MetadataOperation::UpsertZoteroItem {
                key: item.key.clone(),
            });

            // Availability is recorded for every attachment, including the
            // ones we could fetch. Knowing a PDF exists is a fact about the
            // library; fetching it is a separate, user-initiated operation.
            if item.is_pdf_attachment {
                ops.push(MetadataOperation::UpsertAttachmentAvailability {
                    key: item.key.clone(),
                    availability: item.availability,
                });
            }
        }

        SyncPlan::new(SyncJobKind::ZoteroMetadata, ops)
    }

    /// Plan the writes for a set of remote deletions.
    ///
    /// A remote deletion marks the item. It never cascades into deleting the
    /// user's local annotations — those are theirs, and Zotero deleting its
    /// copy of a paper is not a request to destroy their reading of it.
    pub fn plan_deletions(&self, deleted: &DeletedKeys) -> SyncPlan {
        let ops = deleted
            .items
            .iter()
            .map(|key| MetadataOperation::MarkZoteroItemDeleted { key: key.clone() })
            .collect();

        SyncPlan::new(SyncJobKind::ZoteroMetadata, ops)
    }

    /// The watermark to store, given the page just committed.
    ///
    /// Returns `None` while pages remain: advancing early is how an interrupted
    /// sync skips data it never wrote. The cursor only moves once the last page
    /// is safely committed, so a crash re-runs a page rather than losing it.
    pub fn advance(&self, cursor: &SyncCursor, page: &ItemPage) -> Option<SyncCursor> {
        page.is_last().then(|| SyncCursor {
            library: cursor.library.clone(),
            last_version: page.library_version,
        })
    }
}

/// When to try again after a failure.
///
/// Two rules, in order: Zotero's own `Retry-After` always wins, because it
/// knows something we do not; otherwise exponential with a ceiling, so a device
/// on bad wifi does not hammer the API or spin its radio flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffPolicy {
    pub base: Duration,
    pub ceiling: Duration,
    pub max_attempts: u32,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(2),
            ceiling: Duration::from_secs(300),
            max_attempts: 5,
        }
    }
}

/// What to do after a failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Wait, then repeat the same request.
    After(Duration),
    /// Stop. The job is resumable from its cursor, which has not moved.
    GiveUp,
}

impl BackoffPolicy {
    /// `attempt` is 1 for the first retry.
    pub fn decide(
        &self,
        attempt: u32,
        server_hint: Option<Duration>,
        transient: bool,
    ) -> RetryDecision {
        // A permanent failure is not made permanent-er by waiting. Retrying an
        // Unauthorized forever is how a key gets locked out.
        if !transient {
            return RetryDecision::GiveUp;
        }
        if attempt > self.max_attempts {
            return RetryDecision::GiveUp;
        }

        // Zotero asked for a specific delay. Honour it, even if it is longer
        // than our ceiling — it is their service.
        if let Some(hint) = server_hint {
            return RetryDecision::After(hint);
        }

        let exponent = attempt.saturating_sub(1).min(16);
        let delay = self
            .base
            .saturating_mul(2u32.saturating_pow(exponent))
            .min(self.ceiling);

        RetryDecision::After(delay)
    }
}

/// Counters for the sync report.
///
/// `pdfs_transferred` is here, and is always zero for a metadata sync, because
/// the report shows it to the user. A number they can read is worth more than
/// a promise they cannot check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncTally {
    pub pages: u32,
    pub items_seen: u32,
    pub attachments_seen: u32,
    pub deletions: u32,
    pub pdfs_transferred: u32,
}

impl SyncTally {
    pub fn record_page(&mut self, page: &ItemPage) {
        self.pages += 1;
        self.items_seen += page.items.len() as u32;
        self.attachments_seen += page.items.iter().filter(|i| i.is_pdf_attachment).count() as u32;
    }

    pub fn record_deletions(&mut self, deleted: &DeletedKeys) {
        self.deletions += deleted.items.len() as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> ZoteroKey {
        ZoteroKey::from_string(s)
    }

    fn item(k: &str, version: i64) -> RemoteItem {
        RemoteItem {
            key: key(k),
            version,
            item_type: "journalArticle".into(),
            is_pdf_attachment: false,
            availability: AttachmentAvailability::Unknown,
        }
    }

    fn pdf(k: &str, version: i64, availability: AttachmentAvailability) -> RemoteItem {
        RemoteItem {
            key: key(k),
            version,
            item_type: "attachment".into(),
            is_pdf_attachment: true,
            availability,
        }
    }

    fn page(items: Vec<RemoteItem>, library_version: i64, next: Option<u32>) -> ItemPage {
        ItemPage {
            items,
            library_version,
            next_start: next,
        }
    }

    // ── the firewall ────────────────────────────────────────────────────

    /// Safety test S9, at the planner. A library where every paper has a
    /// downloadable PDF still moves nothing.
    #[test]
    fn a_page_full_of_available_pdfs_plans_no_transfer() {
        let items = (0..50)
            .map(|i| {
                pdf(
                    &format!("KEY{i:05}"),
                    i as i64,
                    AttachmentAvailability::AvailableLocal,
                )
            })
            .collect();

        let plan = SyncPlanner::new().plan_page(&page(items, 500, None));

        assert_eq!(plan.pdf_transfer_count(), 0);
        assert!(plan.operations.iter().all(|op| !op.transfers_a_file()));
        assert_eq!(plan.job_kind, SyncJobKind::ZoteroMetadata);
    }

    #[test]
    fn availability_is_recorded_for_attachments_and_only_for_attachments() {
        let plan = SyncPlanner::new().plan_page(&page(
            vec![
                item("PAPER1", 1),
                pdf("FILE1", 2, AttachmentAvailability::AvailableLocal),
            ],
            10,
            None,
        ));

        let availability_ops = plan
            .operations
            .iter()
            .filter(|op| matches!(op, MetadataOperation::UpsertAttachmentAvailability { .. }))
            .count();
        assert_eq!(availability_ops, 1, "one attachment, one availability fact");
        assert_eq!(
            plan.operations.len(),
            3,
            "two upserts plus one availability"
        );
    }

    // ── the watermark ───────────────────────────────────────────────────

    /// The rule that makes an interrupted sync safe.
    #[test]
    fn the_watermark_does_not_move_while_pages_remain() {
        let planner = SyncPlanner::new();
        let cursor = SyncCursor::initial(LibraryRef::user("12345"));

        let middle = page(vec![item("A", 1)], 900, Some(100));
        assert_eq!(
            planner.advance(&cursor, &middle),
            None,
            "advancing before the last page is how an interrupted sync skips \
             data it never wrote"
        );
    }

    #[test]
    fn the_watermark_moves_once_the_last_page_commits() {
        let planner = SyncPlanner::new();
        let cursor = SyncCursor::initial(LibraryRef::user("12345"));

        let last = page(vec![item("Z", 899)], 900, None);
        let advanced = planner.advance(&cursor, &last).expect("last page advances");

        assert_eq!(advanced.last_version, 900);
        assert_eq!(
            advanced.library, cursor.library,
            "the library must not change"
        );
    }

    #[test]
    fn the_watermark_is_the_library_version_not_the_highest_item() {
        // An item can be older than the library's current version; using the
        // item's would re-fetch everything between the two, forever.
        let planner = SyncPlanner::new();
        let cursor = SyncCursor::initial(LibraryRef::user("1"));
        let last = page(vec![item("A", 42)], 900, None);

        assert_eq!(planner.advance(&cursor, &last).unwrap().last_version, 900);
    }

    #[test]
    fn an_interrupted_sync_re_requests_the_same_page() {
        // The cursor never moved, so the next run asks for the same `since`.
        let cursor = SyncCursor {
            library: LibraryRef::user("12345"),
            last_version: 800,
        };
        assert_eq!(
            cursor.query(0, 100),
            "/users/12345/items?since=800&start=0&limit=100"
        );
    }

    #[test]
    fn a_first_sync_asks_for_everything() {
        let cursor = SyncCursor::initial(LibraryRef::user("12345"));
        assert!(cursor.is_first_sync());
        assert!(cursor.query(0, 100).contains("since=0"));
    }

    #[test]
    fn a_group_library_uses_the_group_path() {
        let cursor = SyncCursor::initial(LibraryRef::group("98765"));
        assert!(cursor.query(0, 100).starts_with("/groups/98765/items"));
    }

    // ── deletions ───────────────────────────────────────────────────────

    #[test]
    fn a_remote_deletion_marks_but_does_not_cascade() {
        let deleted = DeletedKeys {
            items: vec![key("GONE1"), key("GONE2")],
            collections: vec![],
        };
        let plan = SyncPlanner::new().plan_deletions(&deleted);

        assert_eq!(plan.operations.len(), 2);
        assert!(plan
            .operations
            .iter()
            .all(|op| matches!(op, MetadataOperation::MarkZoteroItemDeleted { .. })));

        // Nothing in the plan can remove a highlight or a note. The user's
        // reading of a paper is theirs, and Zotero deleting its copy is not a
        // request to destroy it.
        assert_eq!(plan.pdf_transfer_count(), 0);
    }

    // ── backoff ─────────────────────────────────────────────────────────

    #[test]
    fn a_server_hint_always_wins() {
        // Zotero knows something we do not. Even a hint longer than our
        // ceiling is honoured -- it is their service.
        let policy = BackoffPolicy::default();
        assert_eq!(
            policy.decide(1, Some(Duration::from_secs(900)), true),
            RetryDecision::After(Duration::from_secs(900))
        );
    }

    #[test]
    fn backoff_grows_and_then_stops_growing() {
        let policy = BackoffPolicy::default();
        let delay = |attempt| match policy.decide(attempt, None, true) {
            RetryDecision::After(d) => d,
            RetryDecision::GiveUp => panic!("gave up at attempt {attempt}"),
        };

        assert_eq!(delay(1), Duration::from_secs(2));
        assert_eq!(delay(2), Duration::from_secs(4));
        assert_eq!(delay(3), Duration::from_secs(8));
        assert_eq!(delay(4), Duration::from_secs(16));
        assert!(delay(5) <= policy.ceiling);
    }

    #[test]
    fn a_permanent_failure_is_not_retried() {
        // Retrying an Unauthorized forever is how a key gets locked out.
        let policy = BackoffPolicy::default();
        assert_eq!(policy.decide(1, None, false), RetryDecision::GiveUp);
        assert_eq!(
            policy.decide(1, Some(Duration::from_secs(5)), false),
            RetryDecision::GiveUp,
            "not even a server hint makes a permanent failure worth retrying"
        );
    }

    #[test]
    fn retries_are_bounded() {
        let policy = BackoffPolicy::default();
        assert_eq!(
            policy.decide(policy.max_attempts + 1, None, true),
            RetryDecision::GiveUp
        );
    }

    #[test]
    fn giving_up_leaves_the_job_resumable() {
        // Nothing about giving up moves the cursor, so the next run continues
        // from where this one stopped.
        let planner = SyncPlanner::new();
        let cursor = SyncCursor {
            library: LibraryRef::user("1"),
            last_version: 700,
        };
        let interrupted = page(vec![item("A", 701)], 900, Some(100));

        assert_eq!(planner.advance(&cursor, &interrupted), None);
        assert_eq!(cursor.last_version, 700);
    }

    // ── the report ──────────────────────────────────────────────────────

    #[test]
    fn the_tally_always_reports_zero_transfers() {
        let mut tally = SyncTally::default();
        tally.record_page(&page(
            vec![
                item("A", 1),
                pdf("B", 2, AttachmentAvailability::AvailableLocal),
                pdf("C", 3, AttachmentAvailability::NotPresent),
            ],
            10,
            None,
        ));
        tally.record_deletions(&DeletedKeys {
            items: vec![key("D")],
            collections: vec![],
        });

        assert_eq!(tally.pages, 1);
        assert_eq!(tally.items_seen, 3);
        assert_eq!(tally.attachments_seen, 2);
        assert_eq!(tally.deletions, 1);
        assert_eq!(
            tally.pdfs_transferred, 0,
            "a metadata sync has no way to make this non-zero"
        );
    }

    #[test]
    fn a_multi_page_sync_tallies_every_page() {
        let mut tally = SyncTally::default();
        for _ in 0..4 {
            tally.record_page(&page(vec![item("A", 1), item("B", 2)], 10, Some(1)));
        }
        assert_eq!(tally.pages, 4);
        assert_eq!(tally.items_seen, 8);
        assert_eq!(tally.pdfs_transferred, 0);
    }
}
