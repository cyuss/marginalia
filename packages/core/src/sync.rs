//! Sync jobs, and the transfer firewall.
//!
//! Implements `docs/architecture/SYNC_STATE_MACHINE.md`.
//!
//! # The firewall
//!
//! [`MetadataOperation`] and [`TransferOperation`] are separate types. The
//! metadata enum has **no variant capable of expressing a file transfer**, and
//! the metadata executor accepts only that enum. There is therefore no
//! program in which a metadata sync moves a PDF onto someone's device — not
//! because we remembered to check, but because the sentence cannot be written.
//!
//! If you are here to add a variant to [`MetadataOperation`] that touches a
//! file: don't. That is the one change this design exists to prevent.

use crate::ids::{DocumentId, RemarkableDocumentId, SyncJobId, SyncOperationId, ZoteroKey};
use crate::intent::ExplicitUserIntent;
use crate::zotero::AttachmentAvailability;
use crate::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncJobKind {
    ZoteroMetadata,
    DeviceScan,
    AnnotationIngest,
    Transfer,
    Removal,
    ZoteroExport,
    TagBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobTrigger {
    User,
    Schedule,
    Startup,
}

impl SyncJobKind {
    /// Whether this job kind may change anything on the physical device.
    pub fn writes_to_device(self) -> bool {
        matches!(self, SyncJobKind::Transfer | SyncJobKind::Removal)
    }

    /// Which triggers are allowed to start this kind of job.
    ///
    /// Device-writing jobs and outward exports are user-only. A schedule can
    /// never start one, which is the second of three independent guards on
    /// INV-2 (the others being the type split below and a SQL CHECK
    /// constraint).
    pub fn may_be_triggered_by(self, trigger: JobTrigger) -> bool {
        match self {
            SyncJobKind::Transfer | SyncJobKind::Removal | SyncJobKind::ZoteroExport => {
                trigger == JobTrigger::User
            }
            SyncJobKind::ZoteroMetadata
            | SyncJobKind::DeviceScan
            | SyncJobKind::AnnotationIngest
            | SyncJobKind::TagBridge => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncJobState {
    Created,
    Planned,
    Rejected,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    CompletedWithWarnings,
    Failed,
    RollingBack,
    RolledBack,
    /// Terminal and loud. Reaching this marks the device read-only until the
    /// user reviews what happened; we never improvise a second cleanup attempt.
    RollbackFailed,
}

impl SyncJobState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SyncJobState::Rejected
                | SyncJobState::Cancelled
                | SyncJobState::Completed
                | SyncJobState::CompletedWithWarnings
                | SyncJobState::RolledBack
                | SyncJobState::RollbackFailed
        )
    }

    /// Whether reaching this state should restrict the device to read-only.
    pub fn degrades_device_permissions(self) -> bool {
        self == SyncJobState::RollbackFailed
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The firewall: two operation types that cannot be substituted for each other.
// ─────────────────────────────────────────────────────────────────────────────

/// Operations a metadata job may perform.
///
/// Everything here is a local database write or an outbound *read*. Note what
/// is absent: there is no variant that copies, uploads, downloads, or otherwise
/// moves a file. `SyncExecutor` accepts only this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "kind")]
pub enum MetadataOperation {
    UpsertZoteroItem {
        key: ZoteroKey,
    },
    UpsertZoteroCollection {
        key: ZoteroKey,
    },
    /// Record *that* an attachment exists and is readable. Records a fact;
    /// moves nothing.
    UpsertAttachmentAvailability {
        key: ZoteroKey,
        availability: AttachmentAvailability,
    },
    MarkZoteroItemDeleted {
        key: ZoteroKey,
    },
    UpsertTag {
        namespace: String,
        name: String,
    },
    /// Associate a device document we discovered with a mapping we hold.
    LinkDeviceDocument {
        document_id: DocumentId,
        device_uuid: RemarkableDocumentId,
    },
    RecordAnnotationMetadata {
        document_id: DocumentId,
        count: u32,
    },
    UpdateReadingState {
        document_id: DocumentId,
        page: u32,
    },
}

impl MetadataOperation {
    /// Structural proof for the reader (and for test S8): no metadata operation
    /// transfers a file. This is a constant, not a computation, because there
    /// is nothing to compute — no variant can express a transfer.
    pub const fn transfers_a_file(&self) -> bool {
        false
    }
}

/// Operations that change what is physically on the device.
///
/// Each carries an [`ExplicitUserIntent`] **by value**, so constructing one
/// requires a human confirmation that is consumed in the process. The executor
/// additionally requires a `WriteGrant` minted by `marginalia-safety`.
#[derive(Debug)]
pub enum TransferOperation {
    /// Put exactly one validated PDF on the device.
    UploadPdf {
        document_id: DocumentId,
        intent: ExplicitUserIntent,
    },
    /// Remove exactly one document that Marginalia itself transferred.
    RemoveDeviceDocument {
        document_id: DocumentId,
        device_uuid: RemarkableDocumentId,
        intent: ExplicitUserIntent,
    },
}

impl TransferOperation {
    pub fn document_id(&self) -> &DocumentId {
        match self {
            TransferOperation::UploadPdf { document_id, .. }
            | TransferOperation::RemoveDeviceDocument { document_id, .. } => document_id,
        }
    }

    pub fn intent(&self) -> &ExplicitUserIntent {
        match self {
            TransferOperation::UploadPdf { intent, .. }
            | TransferOperation::RemoveDeviceDocument { intent, .. } => intent,
        }
    }
}

/// A plan is produced by a pure planner and can always be shown to the user
/// before anything happens. Dry-run is not a mode; it is the default shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPlan {
    pub job_kind: SyncJobKind,
    pub operations: Vec<MetadataOperation>,
}

impl SyncPlan {
    pub fn new(job_kind: SyncJobKind, operations: Vec<MetadataOperation>) -> Self {
        Self {
            job_kind,
            operations,
        }
    }

    /// Always reported to the user, precisely so they can see it is zero.
    pub fn pdf_transfer_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|op| op.transfers_a_file())
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncCounters {
    pub metadata_updated: u32,
    pub new_items: u32,
    pub tags_updated: u32,
    pub annotations_imported: u32,
    /// Displayed in every sync report, including metadata syncs where it is 0.
    pub pdfs_transferred: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncJob {
    pub id: SyncJobId,
    pub kind: SyncJobKind,
    pub state: SyncJobState,
    pub triggered_by: JobTrigger,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub counters: SyncCounters,
}

/// One atomic unit of work inside a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOperationRecord {
    pub id: SyncOperationId,
    pub job_id: SyncJobId,
    pub seq: u32,
    pub kind: String,
    pub target_ref: Option<String>,
    /// Makes a duplicate `Send` a no-op instead of a second copy on the device.
    pub idempotency_key: String,
    pub attempted_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Safety test S8, at the type level.
    #[test]
    fn no_metadata_operation_can_transfer_a_file() {
        let ops = vec![
            MetadataOperation::UpsertZoteroItem {
                key: ZoteroKey::from_string("ABCD1234"),
            },
            MetadataOperation::UpsertAttachmentAvailability {
                key: ZoteroKey::from_string("EFGH5678"),
                // Even the "we could send this" fact moves nothing.
                availability: AttachmentAvailability::AvailableLocal,
            },
            MetadataOperation::LinkDeviceDocument {
                document_id: DocumentId::new(),
                device_uuid: RemarkableDocumentId::from_string("uuid-1"),
            },
        ];

        let plan = SyncPlan::new(SyncJobKind::ZoteroMetadata, ops);
        assert_eq!(plan.pdf_transfer_count(), 0);
        assert!(plan.operations.iter().all(|op| !op.transfers_a_file()));
    }

    #[test]
    fn a_schedule_cannot_start_a_device_write() {
        for kind in [SyncJobKind::Transfer, SyncJobKind::Removal] {
            assert!(!kind.may_be_triggered_by(JobTrigger::Schedule));
            assert!(!kind.may_be_triggered_by(JobTrigger::Startup));
            assert!(kind.may_be_triggered_by(JobTrigger::User));
        }
    }

    #[test]
    fn metadata_sync_may_run_unattended() {
        // The safe jobs are exactly the ones that never write to the device.
        for kind in [
            SyncJobKind::ZoteroMetadata,
            SyncJobKind::DeviceScan,
            SyncJobKind::AnnotationIngest,
        ] {
            assert!(kind.may_be_triggered_by(JobTrigger::Schedule));
            assert!(!kind.writes_to_device());
        }
    }

    #[test]
    fn export_to_zotero_is_user_only() {
        // Not a device write, but still an outward-facing action the user must
        // ask for explicitly.
        assert!(!SyncJobKind::ZoteroExport.may_be_triggered_by(JobTrigger::Schedule));
    }

    #[test]
    fn rollback_failure_degrades_permissions() {
        assert!(SyncJobState::RollbackFailed.degrades_device_permissions());
        assert!(SyncJobState::RollbackFailed.is_terminal());
        assert!(!SyncJobState::RolledBack.degrades_device_permissions());
    }
}
