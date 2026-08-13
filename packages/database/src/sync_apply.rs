//! Applying a metadata plan to the database.
//!
//! The planner in `marginalia-zotero` decides *what* to write; this decides
//! *how*, and makes it atomic. The two are separate crates because the planner
//! must stay pure — it is where the "a sync cannot move a file" guarantee
//! lives — and this is where SQLite does.
//!
//! # Two rules
//!
//! 1. **A page applies entirely or not at all.** A half-applied page would
//!    leave the database describing a library state that never existed.
//! 2. **The cursor moves separately, and last.** Committing rows and advancing
//!    the watermark in one step looks tidier and is wrong: it makes "we wrote
//!    this page" and "we need never fetch it again" the same fact, so a crash
//!    between the final page and the final commit silently loses data.

use marginalia_core::clock::{Clock, SYSTEM_CLOCK};
use marginalia_core::sync::MetadataOperation;
use marginalia_core::zotero::AttachmentAvailability;
use rusqlite::{params, Connection, OptionalExtension};

use crate::DbResult;

/// What a page's application changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppliedCounts {
    pub items_upserted: u32,
    pub collections_upserted: u32,
    pub availability_recorded: u32,
    pub items_marked_deleted: u32,
    pub tags_upserted: u32,
    pub other: u32,
    /// Always zero. Present because the sync report shows it to the user, and a
    /// number they can read beats a promise they cannot check.
    pub pdfs_transferred: u32,
}

impl AppliedCounts {
    pub fn total(&self) -> u32 {
        self.items_upserted
            + self.collections_upserted
            + self.availability_recorded
            + self.items_marked_deleted
            + self.tags_upserted
            + self.other
    }
}

fn availability_str(a: AttachmentAvailability) -> &'static str {
    match a {
        AttachmentAvailability::Unknown => "UNKNOWN",
        AttachmentAvailability::NotPresent => "NOT_PRESENT",
        AttachmentAvailability::AvailableLocal => "AVAILABLE_LOCAL",
        AttachmentAvailability::Unreadable => "UNREADABLE",
    }
}

/// Applies metadata operations. Holds no state of its own.
/// One folder, ready to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCollection {
    pub key: String,
    pub name: String,
    pub depth: usize,
    pub children: usize,
}

pub struct MetadataApplier<'a> {
    conn: &'a Connection,
    clock: &'a dyn Clock,
}

impl<'a> MetadataApplier<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            clock: &SYSTEM_CLOCK,
        }
    }

    pub fn with_clock(conn: &'a Connection, clock: &'a dyn Clock) -> Self {
        Self { conn, clock }
    }

    /// Apply a whole page, atomically.
    ///
    /// Any failure rolls the page back completely. The caller then retries the
    /// same page — which is safe, because the cursor has not moved.
    pub fn apply(&self, operations: &[MetadataOperation]) -> DbResult<AppliedCounts> {
        let mut counts = AppliedCounts::default();
        let now = self.clock.now().to_rfc3339();

        self.conn.execute_batch("BEGIN")?;

        let result = (|| -> DbResult<()> {
            for op in operations {
                self.apply_one(op, &now, &mut counts)?;
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(counts)
            }
            Err(e) => {
                // Best effort: if the rollback itself fails the connection is
                // in trouble, and the original error is the more useful one to
                // report.
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Resolve every collection's parent, once they all exist.
    ///
    /// Runs after the upserts so that page order cannot matter. A parent key
    /// naming a collection we have never seen leaves the child at the top
    /// level rather than failing: an unreachable parent is a stale reference,
    /// not a reason to lose the folder.
    pub fn link_collection_parents(
        &self,
        links: &[(
            marginalia_core::ids::ZoteroKey,
            Option<marginalia_core::ids::ZoteroKey>,
        )],
    ) -> DbResult<usize> {
        let mut linked = 0usize;
        for (child, parent) in links {
            let changed = match parent {
                Some(parent) => self.conn.execute(
                    "UPDATE zotero_collection
                        SET parent_collection_id =
                              (SELECT id FROM zotero_collection WHERE zotero_key = ?2)
                      WHERE zotero_key = ?1",
                    params![child.as_str(), parent.as_str()],
                )?,
                None => self.conn.execute(
                    "UPDATE zotero_collection SET parent_collection_id = NULL
                      WHERE zotero_key = ?1",
                    params![child.as_str()],
                )?,
            };
            linked += changed;
        }
        Ok(linked)
    }

    /// The folder tree, as stored, in reading order.
    ///
    /// Depth is computed here rather than by the caller so that a cycle — which
    /// Zotero should never produce, but a corrupt row could — cannot become an
    /// infinite loop in a renderer.
    pub fn collection_tree(&self) -> DbResult<Vec<StoredCollection>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.zotero_key, c.name, p.zotero_key
               FROM zotero_collection c
               LEFT JOIN zotero_collection p ON p.id = c.parent_collection_id
              ORDER BY LOWER(c.name)",
        )?;
        let rows: Vec<(String, String, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;

        let mut children: std::collections::BTreeMap<Option<String>, Vec<(String, String)>> =
            std::collections::BTreeMap::new();
        for (key, name, parent) in &rows {
            children
                .entry(parent.clone())
                .or_default()
                .push((key.clone(), name.clone()));
        }

        fn walk(
            children: &std::collections::BTreeMap<Option<String>, Vec<(String, String)>>,
            parent: Option<String>,
            depth: usize,
            seen: &mut std::collections::HashSet<String>,
            out: &mut Vec<StoredCollection>,
        ) {
            let Some(kids) = children.get(&parent) else {
                return;
            };
            for (key, name) in kids {
                // A cycle should be impossible, but a corrupt parent link must
                // not become an infinite descent in a renderer.
                if !seen.insert(key.clone()) {
                    continue;
                }
                out.push(StoredCollection {
                    key: key.clone(),
                    name: name.clone(),
                    depth,
                    children: children.get(&Some(key.clone())).map_or(0, Vec::len),
                });
                walk(children, Some(key.clone()), depth + 1, seen, out);
            }
        }

        let mut out = Vec::new();
        walk(
            &children,
            None,
            0,
            &mut std::collections::HashSet::new(),
            &mut out,
        );
        Ok(out)
    }

    fn apply_one(
        &self,
        op: &MetadataOperation,
        now: &str,
        counts: &mut AppliedCounts,
    ) -> DbResult<()> {
        match op {
            MetadataOperation::UpsertZoteroItem { key } => {
                // A placeholder row until the full payload lands: the point of
                // this pass is that the key exists and can be referred to.
                self.conn.execute(
                    "INSERT INTO zotero_item
                       (id, zotero_key, zotero_version, library_id, item_type, raw,
                        created_at, updated_at)
                     VALUES (?1, ?2, 0, '', 'unknown', '{}', ?3, ?3)
                     ON CONFLICT(zotero_key) DO UPDATE SET updated_at = ?3",
                    params![
                        marginalia_core::ids::ZoteroItemId::new().as_str(),
                        key.as_str(),
                        now
                    ],
                )?;
                counts.items_upserted += 1;
            }

            MetadataOperation::UpsertAttachmentAvailability { key, availability } => {
                // Recording a fact about a file. Note what is absent: no path
                // is opened, no bytes are read, nothing is copied.
                let changed = self.conn.execute(
                    "UPDATE zotero_attachment
                        SET availability = ?2, updated_at = ?3
                      WHERE zotero_key = ?1",
                    params![key.as_str(), availability_str(*availability), now],
                )?;
                // An availability for an attachment we have not mirrored yet is
                // not an error; the next full pass will carry the row.
                let _ = changed;
                counts.availability_recorded += 1;
            }

            MetadataOperation::MarkZoteroItemDeleted { key } => {
                // Marks. Deliberately does not cascade: the user's highlights
                // on a paper are theirs, and Zotero deleting its copy is not a
                // request to destroy their reading of it.
                self.conn.execute(
                    "UPDATE zotero_item
                        SET deleted_remote = 1, updated_at = ?2
                      WHERE zotero_key = ?1",
                    params![key.as_str(), now],
                )?;
                counts.items_marked_deleted += 1;
            }

            MetadataOperation::UpsertTag { namespace, name } => {
                self.conn.execute(
                    "INSERT INTO tag (id, namespace, name, normalized_name)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(namespace, name) DO NOTHING",
                    params![
                        marginalia_core::ids::TagId::new().as_str(),
                        namespace,
                        name,
                        marginalia_core::tag::Tag::normalize(name)
                    ],
                )?;
                counts.tags_upserted += 1;
            }

            MetadataOperation::UpsertZoteroCollection {
                key, name, version, ..
            } => {
                // The parent is set in a second pass, by `link_collection_parents`.
                //
                // WHY not here: `parent_collection_id` is a foreign key onto
                // this table's own `id`, and a child can arrive on an earlier
                // page than its parent. Resolving inline would fail for exactly
                // the collections that have a parent, which is most of them.
                // Two passes make page order irrelevant.
                self.conn.execute(
                    "INSERT INTO zotero_collection
                       (id, zotero_key, zotero_version, name, library_id)
                     VALUES (?1, ?2, ?4, ?3, '')
                     ON CONFLICT(zotero_key) DO UPDATE SET
                        name = excluded.name,
                        zotero_version = excluded.zotero_version",
                    params![
                        marginalia_core::ids::ZoteroItemId::new().as_str(),
                        key.as_str(),
                        name,
                        version
                    ],
                )?;
                counts.collections_upserted += 1;
            }

            // Recorded for the counters; their storage arrives with the phases
            // that own them.
            MetadataOperation::LinkDeviceDocument { .. }
            | MetadataOperation::RecordAnnotationMetadata { .. }
            | MetadataOperation::UpdateReadingState { .. } => {
                counts.other += 1;
            }
        }
        Ok(())
    }
}

/// Reading and advancing a library's sync watermark.
pub struct SyncStateRepository<'a> {
    conn: &'a Connection,
    clock: &'a dyn Clock,
}

impl<'a> SyncStateRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self {
            conn,
            clock: &SYSTEM_CLOCK,
        }
    }

    pub fn with_clock(conn: &'a Connection, clock: &'a dyn Clock) -> Self {
        Self { conn, clock }
    }

    /// The version a library has been synced to. `0` means never.
    pub fn version_for(&self, library_key: &str) -> DbResult<i64> {
        let version: Option<i64> = self
            .conn
            .query_row(
                "SELECT last_version FROM zotero_sync_state WHERE library_key = ?1",
                params![library_key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(version.unwrap_or(0))
    }

    /// Record that a library is synced up to `version`.
    ///
    /// Call this **only** after the final page of a run has been applied. The
    /// separation is the point: rows and cursor move in different transactions
    /// so a crash between them costs a re-fetch rather than data.
    pub fn commit_version(&self, library_key: &str, version: i64) -> DbResult<()> {
        let now = self.clock.now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO zotero_sync_state
               (library_key, last_version, last_synced_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(library_key) DO UPDATE SET
               last_version = ?2, last_synced_at = ?3, updated_at = ?3",
            params![library_key, version, now],
        )?;
        Ok(())
    }

    /// Forget a library's progress, so the next sync re-mirrors it.
    ///
    /// Does not touch the mirrored data — this is our bookkeeping, and a user
    /// asking to re-sync is not asking to lose anything.
    pub fn reset(&self, library_key: &str) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM zotero_sync_state WHERE library_key = ?1",
            params![library_key],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;
    use marginalia_core::ids::ZoteroKey;

    fn key(s: &str) -> ZoteroKey {
        ZoteroKey::from_string(s)
    }

    fn item_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM zotero_item", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_page_of_items_is_applied() {
        let conn = open_in_memory().unwrap();
        let counts = MetadataApplier::new(&conn)
            .apply(&[
                MetadataOperation::UpsertZoteroItem { key: key("AAA") },
                MetadataOperation::UpsertZoteroItem { key: key("BBB") },
            ])
            .unwrap();

        assert_eq!(counts.items_upserted, 2);
        assert_eq!(counts.pdfs_transferred, 0);
        assert_eq!(item_count(&conn), 2);
    }

    #[test]
    fn applying_the_same_page_twice_does_not_duplicate() {
        // The realistic case: a sync is interrupted after applying a page but
        // before the cursor moves, so the next run re-applies it.
        let conn = open_in_memory().unwrap();
        let ops = [MetadataOperation::UpsertZoteroItem { key: key("AAA") }];

        MetadataApplier::new(&conn).apply(&ops).unwrap();
        MetadataApplier::new(&conn).apply(&ops).unwrap();

        assert_eq!(item_count(&conn), 1, "re-applying a page must be safe");
    }

    /// Rule 1: a page applies entirely or not at all.
    #[test]
    fn a_failing_operation_rolls_the_whole_page_back() {
        let conn = open_in_memory().unwrap();

        // A tag with an invalid namespace violates the schema CHECK, mid-page.
        let result = MetadataApplier::new(&conn).apply(&[
            MetadataOperation::UpsertZoteroItem { key: key("AAA") },
            MetadataOperation::UpsertTag {
                namespace: "NOT_A_NAMESPACE".into(),
                name: "x".into(),
            },
            MetadataOperation::UpsertZoteroItem { key: key("BBB") },
        ]);

        assert!(result.is_err());
        assert_eq!(
            item_count(&conn),
            0,
            "a half-applied page would describe a library state that never existed"
        );
    }

    #[test]
    fn a_rolled_back_page_can_be_retried() {
        let conn = open_in_memory().unwrap();
        let bad = [
            MetadataOperation::UpsertZoteroItem { key: key("AAA") },
            MetadataOperation::UpsertTag {
                namespace: "BAD".into(),
                name: "x".into(),
            },
        ];
        assert!(MetadataApplier::new(&conn).apply(&bad).is_err());

        // The connection is still usable and the page succeeds once fixed.
        let good = [MetadataOperation::UpsertZoteroItem { key: key("AAA") }];
        assert!(MetadataApplier::new(&conn).apply(&good).is_ok());
        assert_eq!(item_count(&conn), 1);
    }

    #[test]
    fn collections_are_upserted_idempotently() {
        let conn = open_in_memory().unwrap();
        let applier = MetadataApplier::new(&conn);
        let ops = [MetadataOperation::UpsertZoteroCollection {
            key: key("COLL1"),
            name: "Machine Learning".into(),
            parent_key: None,
            version: 7,
        }];

        applier.apply(&ops).unwrap();
        applier.apply(&ops).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM zotero_collection", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn tags_are_upserted_idempotently() {
        let conn = open_in_memory().unwrap();
        let applier = MetadataApplier::new(&conn);
        let ops = [
            MetadataOperation::UpsertTag {
                namespace: "ZOTERO".into(),
                name: "AI".into(),
            },
            MetadataOperation::UpsertTag {
                namespace: "ZOTERO".into(),
                name: "AI".into(),
            },
        ];
        applier.apply(&ops).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "the same tag twice in one page is one row");
    }

    #[test]
    fn a_remote_deletion_marks_without_removing() {
        let conn = open_in_memory().unwrap();
        let applier = MetadataApplier::new(&conn);

        applier
            .apply(&[MetadataOperation::UpsertZoteroItem { key: key("AAA") }])
            .unwrap();
        applier
            .apply(&[MetadataOperation::MarkZoteroItemDeleted { key: key("AAA") }])
            .unwrap();

        assert_eq!(item_count(&conn), 1, "the row must survive the deletion");
        let deleted: i64 = conn
            .query_row(
                "SELECT deleted_remote FROM zotero_item WHERE zotero_key = 'AAA'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn availability_for_an_unmirrored_attachment_is_not_an_error() {
        // Zotero can mention an attachment before we have mirrored its row.
        // Failing here would abort a whole page over a harmless ordering.
        let conn = open_in_memory().unwrap();
        let counts = MetadataApplier::new(&conn)
            .apply(&[MetadataOperation::UpsertAttachmentAvailability {
                key: key("NEVER_SEEN"),
                availability: AttachmentAvailability::AvailableLocal,
            }])
            .unwrap();
        assert_eq!(counts.availability_recorded, 1);
    }

    // ── the watermark ───────────────────────────────────────────────────

    #[test]
    fn an_unsynced_library_reports_version_zero() {
        let conn = open_in_memory().unwrap();
        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            0
        );
    }

    #[test]
    fn the_watermark_round_trips() {
        let conn = open_in_memory().unwrap();
        let repo = SyncStateRepository::new(&conn);

        repo.commit_version("users/12345", 900).unwrap();
        assert_eq!(repo.version_for("users/12345").unwrap(), 900);

        repo.commit_version("users/12345", 1200).unwrap();
        assert_eq!(repo.version_for("users/12345").unwrap(), 1200);
    }

    #[test]
    fn libraries_have_independent_watermarks() {
        // The same numeric id can exist as both a user and a group, and their
        // versions are unrelated. Sharing a cursor would corrupt both.
        let conn = open_in_memory().unwrap();
        let repo = SyncStateRepository::new(&conn);

        repo.commit_version("users/12345", 900).unwrap();
        repo.commit_version("groups/12345", 40).unwrap();

        assert_eq!(repo.version_for("users/12345").unwrap(), 900);
        assert_eq!(repo.version_for("groups/12345").unwrap(), 40);
    }

    /// Rule 2, and the reason the two are separate calls.
    #[test]
    fn applying_a_page_does_not_move_the_watermark() {
        let conn = open_in_memory().unwrap();
        MetadataApplier::new(&conn)
            .apply(&[MetadataOperation::UpsertZoteroItem { key: key("AAA") }])
            .unwrap();

        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            0,
            "writing rows and declaring the library synced are different facts"
        );
    }

    #[test]
    fn a_crash_after_the_last_page_costs_a_refetch_not_data() {
        // Page applied, process dies before commit_version. On restart the
        // cursor is unchanged, so that page is fetched and applied again --
        // which the idempotency test above shows is safe.
        let conn = open_in_memory().unwrap();
        let ops = [MetadataOperation::UpsertZoteroItem { key: key("AAA") }];

        MetadataApplier::new(&conn).apply(&ops).unwrap();
        // ... crash ...
        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            0
        );

        MetadataApplier::new(&conn).apply(&ops).unwrap();
        SyncStateRepository::new(&conn)
            .commit_version("users/12345", 900)
            .unwrap();

        assert_eq!(item_count(&conn), 1);
        assert_eq!(
            SyncStateRepository::new(&conn)
                .version_for("users/12345")
                .unwrap(),
            900
        );
    }

    #[test]
    fn resetting_progress_keeps_the_mirrored_data() {
        // A user asking to re-sync is not asking to lose anything.
        let conn = open_in_memory().unwrap();
        MetadataApplier::new(&conn)
            .apply(&[MetadataOperation::UpsertZoteroItem { key: key("AAA") }])
            .unwrap();
        let repo = SyncStateRepository::new(&conn);
        repo.commit_version("users/12345", 900).unwrap();

        repo.reset("users/12345").unwrap();

        assert_eq!(repo.version_for("users/12345").unwrap(), 0);
        assert_eq!(
            item_count(&conn),
            1,
            "the mirror must survive a cursor reset"
        );
    }

    #[test]
    fn timestamps_come_from_the_injected_clock() {
        let conn = open_in_memory().unwrap();
        let frozen = chrono::TimeZone::timestamp_opt(&chrono::Utc, 1_700_000_000, 0).unwrap();
        let clock = marginalia_core::clock::FixedClock::at(frozen);

        SyncStateRepository::with_clock(&conn, &clock)
            .commit_version("users/1", 5)
            .unwrap();

        let at: String = conn
            .query_row(
                "SELECT last_synced_at FROM zotero_sync_state WHERE library_key = 'users/1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(at, frozen.to_rfc3339());
    }

    #[cfg(test)]
    mod collection_tree_tests {
        use super::*;
        use marginalia_core::ids::ZoteroKey;

        fn upsert(key: &str, name: &str, parent: Option<&str>) -> MetadataOperation {
            MetadataOperation::UpsertZoteroCollection {
                key: ZoteroKey::from_string(key),
                name: name.into(),
                parent_key: parent.map(ZoteroKey::from_string),
                version: 1,
            }
        }

        /// The property the two-pass design exists for: a child may arrive on an
        /// earlier page than its parent, and the tree must come out the same.
        #[test]
        fn a_child_seen_before_its_parent_still_lands_under_it() {
            let conn = crate::open_in_memory().unwrap();
            let applier = MetadataApplier::new(&conn);

            applier
                .apply(&[
                    upsert("CHILD", "Active Learning", Some("PARENT")),
                    upsert("PARENT", "Machine Learning", None),
                ])
                .unwrap();
            applier
                .link_collection_parents(&[
                    (
                        ZoteroKey::from_string("CHILD"),
                        Some(ZoteroKey::from_string("PARENT")),
                    ),
                    (ZoteroKey::from_string("PARENT"), None),
                ])
                .unwrap();

            let tree = applier.collection_tree().unwrap();
            assert_eq!(
                tree.iter()
                    .map(|c| (c.name.as_str(), c.depth))
                    .collect::<Vec<_>>(),
                [("Machine Learning", 0), ("Active Learning", 1)]
            );
            assert_eq!(tree[0].children, 1);
        }

        /// Two folders may share a name under different parents — this library has
        /// two called "General Knowledge". Identity is the Zotero key, never the
        /// name, or 259 references silently merge into one folder.
        #[test]
        fn folders_sharing_a_name_stay_separate() {
            let conn = crate::open_in_memory().unwrap();
            let applier = MetadataApplier::new(&conn);

            applier
                .apply(&[
                    upsert("ML", "Machine Learning", None),
                    upsert("MATHS", "Mathematics & CS", None),
                    upsert("GK1", "General Knowledge", Some("ML")),
                    upsert("GK2", "General Knowledge", Some("MATHS")),
                ])
                .unwrap();
            applier
                .link_collection_parents(&[
                    (
                        ZoteroKey::from_string("GK1"),
                        Some(ZoteroKey::from_string("ML")),
                    ),
                    (
                        ZoteroKey::from_string("GK2"),
                        Some(ZoteroKey::from_string("MATHS")),
                    ),
                ])
                .unwrap();

            let tree = applier.collection_tree().unwrap();
            assert_eq!(tree.len(), 4, "a shared name must not collapse two folders");
            assert_eq!(
                tree.iter()
                    .filter(|c| c.name == "General Knowledge")
                    .count(),
                2
            );
        }

        /// A parent Zotero no longer reports leaves the folder at the top level.
        /// Losing the folder would be worse than showing it unnested.
        #[test]
        fn an_unknown_parent_leaves_the_folder_at_the_top_rather_than_dropping_it() {
            let conn = crate::open_in_memory().unwrap();
            let applier = MetadataApplier::new(&conn);

            applier
                .apply(&[upsert("ORPHAN", "Divers", Some("GONE"))])
                .unwrap();
            applier
                .link_collection_parents(&[(
                    ZoteroKey::from_string("ORPHAN"),
                    Some(ZoteroKey::from_string("GONE")),
                )])
                .unwrap();

            let tree = applier.collection_tree().unwrap();
            assert_eq!(tree.len(), 1);
            assert_eq!(tree[0].depth, 0);
        }

        /// Re-syncing must update names, not fork rows.
        #[test]
        fn a_renamed_folder_updates_in_place() {
            let conn = crate::open_in_memory().unwrap();
            let applier = MetadataApplier::new(&conn);

            applier.apply(&[upsert("K", "Old name", None)]).unwrap();
            applier.apply(&[upsert("K", "New name", None)]).unwrap();

            let tree = applier.collection_tree().unwrap();
            assert_eq!(tree.len(), 1);
            assert_eq!(tree[0].name, "New name");
        }
    }
}
