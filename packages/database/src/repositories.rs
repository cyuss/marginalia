//! Repositories.
//!
//! Phase 0 implements the ones the safety story depends on: documents,
//! mappings, sync jobs and the safety log. The Zotero and annotation
//! repositories arrive with the phases that need them.

use chrono::Utc;
use marginalia_core::document::{Document, DocumentSource, DocumentState};
use marginalia_core::ids::{DocumentId, RemarkableDocumentId};
use marginalia_core::sync::{JobTrigger, SyncJobKind};
use rusqlite::{params, Connection, OptionalExtension};

use crate::DbResult;

fn state_str(state: DocumentState) -> &'static str {
    match state {
        DocumentState::MetadataOnly => "METADATA_ONLY",
        DocumentState::AttachmentAvailable => "ATTACHMENT_AVAILABLE",
        DocumentState::TransferPending => "TRANSFER_PENDING",
        DocumentState::OnRemarkable => "ON_REMARKABLE",
        DocumentState::Annotated => "ANNOTATED",
        DocumentState::ChangesPending => "CHANGES_PENDING",
        DocumentState::Synced => "SYNCED",
        DocumentState::Conflict => "CONFLICT",
        DocumentState::TransferFailed => "TRANSFER_FAILED",
        DocumentState::RemovedFromDevice => "REMOVED_FROM_DEVICE",
    }
}

fn parse_state(s: &str) -> Option<DocumentState> {
    Some(match s {
        "METADATA_ONLY" => DocumentState::MetadataOnly,
        "ATTACHMENT_AVAILABLE" => DocumentState::AttachmentAvailable,
        "TRANSFER_PENDING" => DocumentState::TransferPending,
        "ON_REMARKABLE" => DocumentState::OnRemarkable,
        "ANNOTATED" => DocumentState::Annotated,
        "CHANGES_PENDING" => DocumentState::ChangesPending,
        "SYNCED" => DocumentState::Synced,
        "CONFLICT" => DocumentState::Conflict,
        "TRANSFER_FAILED" => DocumentState::TransferFailed,
        "REMOVED_FROM_DEVICE" => DocumentState::RemovedFromDevice,
        _ => return None,
    })
}

fn source_str(source: DocumentSource) -> &'static str {
    match source {
        DocumentSource::Zotero => "ZOTERO",
        DocumentSource::LocalFile => "LOCAL_FILE",
        DocumentSource::DeviceOnly => "DEVICE_ONLY",
    }
}

pub struct DocumentRepository<'a> {
    conn: &'a Connection,
}

impl<'a> DocumentRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert(&self, doc: &Document) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO document (id, title, source, page_count, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                doc.id.as_str(),
                doc.title,
                source_str(doc.source),
                doc.page_count,
                state_str(doc.state),
                doc.created_at.to_rfc3339(),
                doc.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn state_of(&self, id: &DocumentId) -> DbResult<Option<DocumentState>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM document WHERE id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw.as_deref().and_then(parse_state))
    }

    pub fn set_state(&self, id: &DocumentId, state: DocumentState) -> DbResult<()> {
        self.conn.execute(
            "UPDATE document SET state = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.as_str(), state_str(state), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

pub struct MappingRepository<'a> {
    conn: &'a Connection,
}

impl<'a> MappingRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Whether Marginalia transferred the device document with this uuid, and
    /// may therefore modify or remove it.
    ///
    /// This is the ownership rule from `DEVICE_WRITE_POLICY.md` §3, in one
    /// query. A `false` here means the document belongs to the user and is
    /// read-only forever.
    pub fn owns_device_document(&self, uuid: &RemarkableDocumentId) -> DbResult<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM document_mapping WHERE remarkable_document_id = ?1",
            params![uuid.as_str()],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }
}

pub struct SyncJobRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SyncJobRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(&self, id: &str, kind: SyncJobKind, trigger: JobTrigger) -> DbResult<()> {
        let kind_str = match kind {
            SyncJobKind::ZoteroMetadata => "ZOTERO_METADATA",
            SyncJobKind::DeviceScan => "DEVICE_SCAN",
            SyncJobKind::AnnotationIngest => "ANNOTATION_INGEST",
            SyncJobKind::Transfer => "TRANSFER",
            SyncJobKind::Removal => "REMOVAL",
            SyncJobKind::ZoteroExport => "ZOTERO_EXPORT",
            SyncJobKind::TagBridge => "TAG_BRIDGE",
        };
        let trigger_str = match trigger {
            JobTrigger::User => "USER",
            JobTrigger::Schedule => "SCHEDULE",
            JobTrigger::Startup => "STARTUP",
        };
        self.conn.execute(
            "INSERT INTO sync_job (id, kind, state, triggered_by, started_at)
             VALUES (?1, ?2, 'CREATED', ?3, ?4)",
            params![id, kind_str, trigger_str, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;
    use marginalia_core::ids::MappingId;

    fn sample_document() -> Document {
        Document {
            id: DocumentId::new(),
            title: "Attention Is All You Need".into(),
            source: DocumentSource::Zotero,
            page_count: Some(15),
            state: DocumentState::MetadataOnly,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn documents_round_trip_through_their_state() {
        let conn = open_in_memory().unwrap();
        let repo = DocumentRepository::new(&conn);
        let doc = sample_document();
        repo.insert(&doc).unwrap();

        assert_eq!(
            repo.state_of(&doc.id).unwrap(),
            Some(DocumentState::MetadataOnly)
        );
        repo.set_state(&doc.id, DocumentState::AttachmentAvailable)
            .unwrap();
        assert_eq!(
            repo.state_of(&doc.id).unwrap(),
            Some(DocumentState::AttachmentAvailable)
        );
    }

    #[test]
    fn an_invalid_state_is_rejected_by_the_schema() {
        let conn = open_in_memory().unwrap();
        let err = conn.execute(
            "INSERT INTO document (id, title, source, state, created_at, updated_at)
             VALUES ('d1', 't', 'ZOTERO', 'NOT_A_REAL_STATE', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        );
        assert!(err.is_err(), "boolean soup and typos must not reach the DB");
    }

    /// Safety test S15: a device document we did not transfer is not ours.
    #[test]
    fn foreign_device_documents_are_not_owned() {
        let conn = open_in_memory().unwrap();
        let doc = sample_document();
        DocumentRepository::new(&conn).insert(&doc).unwrap();

        let ours = RemarkableDocumentId::from_string("uuid-ours");
        conn.execute(
            "INSERT INTO document_mapping
               (id, local_document_id, remarkable_document_id, original_filename,
                original_checksum, device_state, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'paper.pdf', 'abc', 'ON_REMARKABLE', ?4, ?4)",
            params![
                MappingId::new().as_str(),
                doc.id.as_str(),
                ours.as_str(),
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();

        let repo = MappingRepository::new(&conn);
        assert!(repo.owns_device_document(&ours).unwrap());
        assert!(
            !repo
                .owns_device_document(&RemarkableDocumentId::from_string("uuid-the-users-own"))
                .unwrap(),
            "a document Marginalia never transferred must never be considered ours"
        );
    }

    /// INV-3 at the storage layer.
    #[test]
    fn the_original_checksum_cannot_be_changed() {
        let conn = open_in_memory().unwrap();
        let doc = sample_document();
        DocumentRepository::new(&conn).insert(&doc).unwrap();
        conn.execute(
            "INSERT INTO document_mapping
               (id, local_document_id, original_filename, original_checksum,
                device_state, created_at, updated_at)
             VALUES ('m1', ?1, 'paper.pdf', 'original-hash', 'METADATA_ONLY', ?2, ?2)",
            params![doc.id.as_str(), Utc::now().to_rfc3339()],
        )
        .unwrap();

        let err = conn.execute(
            "UPDATE document_mapping SET original_checksum = 'different-hash' WHERE id = 'm1'",
            [],
        );
        assert!(err.is_err(), "the original checksum is immutable");

        // Writing the same value is not a change, and must still be allowed.
        conn.execute(
            "UPDATE document_mapping SET original_checksum = 'original-hash' WHERE id = 'm1'",
            [],
        )
        .unwrap();
    }

    /// Safety test S8, third guard: the schema itself refuses a scheduled
    /// transfer job.
    #[test]
    fn a_scheduled_transfer_job_is_rejected_by_the_schema() {
        let conn = open_in_memory().unwrap();
        let err = conn.execute(
            "INSERT INTO sync_job (id, kind, state, triggered_by, started_at)
             VALUES ('j1', 'TRANSFER', 'CREATED', 'SCHEDULE', '2026-01-01T00:00:00Z')",
            [],
        );
        assert!(
            err.is_err(),
            "only a user may start a transfer; a schedule must be refused"
        );
    }

    #[test]
    fn a_scheduled_metadata_sync_is_fine() {
        let conn = open_in_memory().unwrap();
        SyncJobRepository::new(&conn)
            .create("j2", SyncJobKind::ZoteroMetadata, JobTrigger::Schedule)
            .unwrap();
    }

    #[test]
    fn duplicate_idempotency_keys_are_rejected() {
        let conn = open_in_memory().unwrap();
        SyncJobRepository::new(&conn)
            .create("j3", SyncJobKind::Transfer, JobTrigger::User)
            .unwrap();

        let insert = |id: &str| {
            conn.execute(
                "INSERT INTO sync_operation
                   (id, sync_job_id, seq, kind, state, idempotency_key, attempted_at)
                 VALUES (?1, 'j3', 1, 'UPLOAD', 'DONE', 'send:doc-1:hash', ?2)",
                params![id, Utc::now().to_rfc3339()],
            )
        };
        insert("op1").unwrap();
        assert!(
            insert("op2").is_err(),
            "a duplicate Send must be a no-op, not a second copy on the device"
        );
    }
}
