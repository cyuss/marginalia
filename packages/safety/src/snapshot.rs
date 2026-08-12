//! Safety snapshots.
//!
//! Before a write, we record what we are about to affect and what its state
//! was. WHY it must be *verified* rather than merely created: an unverifiable
//! snapshot gives false confidence, which is worse than no snapshot at all —
//! it would let an operation proceed on the belief that it can be undone.
//! An unverified snapshot is therefore treated as absent, and absent means the
//! operation is denied.

use marginalia_core::ids::{DeviceId, DocumentId, SnapshotId};
use marginalia_core::{Checksum, Timestamp};
use serde::{Deserialize, Serialize};

use crate::classification::DeviceOperation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SnapshotStatus {
    /// Created, contents not yet verified.
    Pending,
    /// Verified and usable as a rollback reference.
    Verified,
    /// Verification failed. Treated as no snapshot.
    Failed,
    /// Used by a completed operation.
    Consumed,
    /// A rollback was performed from this snapshot.
    Restored,
}

impl SnapshotStatus {
    /// The only status that satisfies the snapshot precondition.
    pub fn is_usable(self) -> bool {
        self == SnapshotStatus::Verified
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AffectedDocument {
    pub document_id: DocumentId,
    /// The checksum before the operation, when there was already content.
    pub checksum_before: Option<Checksum>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetySnapshot {
    pub id: SnapshotId,
    pub device_id: DeviceId,
    pub created_at: Timestamp,
    pub operation: DeviceOperation,
    pub affected_documents: Vec<AffectedDocument>,
    pub storage_free_before: Option<u64>,
    pub status: SnapshotStatus,
}

impl SafetySnapshot {
    pub fn pending(
        device_id: DeviceId,
        operation: DeviceOperation,
        affected_documents: Vec<AffectedDocument>,
        storage_free_before: Option<u64>,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id: SnapshotId::new(),
            device_id,
            created_at,
            operation,
            affected_documents,
            storage_free_before,
            status: SnapshotStatus::Pending,
        }
    }

    /// Mark the snapshot verified, but only if it actually describes something.
    ///
    /// An empty snapshot for an operation that affects documents is not a
    /// snapshot; refusing to verify it here means the operation will be denied
    /// downstream rather than proceeding on an empty promise.
    pub fn verify(&mut self) -> SnapshotStatus {
        self.status = if self.affected_documents.is_empty() {
            SnapshotStatus::Failed
        } else {
            SnapshotStatus::Verified
        };
        self.status
    }

    pub fn is_usable(&self) -> bool {
        self.status.is_usable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn snapshot_with(docs: Vec<AffectedDocument>) -> SafetySnapshot {
        SafetySnapshot::pending(
            DeviceId::new(),
            DeviceOperation::UploadDocument,
            docs,
            Some(3_000_000_000),
            Utc::now(),
        )
    }

    #[test]
    fn a_new_snapshot_is_not_yet_usable() {
        let s = snapshot_with(vec![AffectedDocument {
            document_id: DocumentId::new(),
            checksum_before: None,
        }]);
        assert_eq!(s.status, SnapshotStatus::Pending);
        assert!(!s.is_usable(), "pending must not satisfy the precondition");
    }

    #[test]
    fn verification_makes_it_usable() {
        let mut s = snapshot_with(vec![AffectedDocument {
            document_id: DocumentId::new(),
            checksum_before: None,
        }]);
        assert_eq!(s.verify(), SnapshotStatus::Verified);
        assert!(s.is_usable());
    }

    #[test]
    fn an_empty_snapshot_cannot_be_verified() {
        let mut s = snapshot_with(vec![]);
        assert_eq!(s.verify(), SnapshotStatus::Failed);
        assert!(
            !s.is_usable(),
            "an empty snapshot must not authorise anything"
        );
    }

    #[test]
    fn only_verified_is_usable() {
        for status in [
            SnapshotStatus::Pending,
            SnapshotStatus::Failed,
            SnapshotStatus::Consumed,
            SnapshotStatus::Restored,
        ] {
            assert!(
                !status.is_usable(),
                "{status:?} must not satisfy the precondition"
            );
        }
        assert!(SnapshotStatus::Verified.is_usable());
    }
}
