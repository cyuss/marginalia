//! The write grant — a capability token for device mutations.
//!
//! # Why this type is shaped the way it is
//!
//! The obvious way to keep a device safe is a check inside every write
//! function: `if !safety.is_allowed(op) { return Err(...) }`. That works right
//! up until someone adds a new write path and forgets the check — and the
//! failure mode is a damaged device, discovered by a user, not by CI.
//!
//! So the check is moved into the type system. Device write functions take a
//! `&WriteGrant`. `WriteGrant` holds a field of type [`Seal`], which is private
//! to this module. Rust therefore forbids constructing one anywhere else: there
//! is no struct literal, no `Default`, no deserialisation path. The *only* way
//! to obtain a grant is [`crate::SafetyManager::authorize`].
//!
//! A forgotten safety check is now a compile error rather than a bug report.

use chrono::Duration;
use marginalia_core::ids::{DeviceId, DocumentId, SnapshotId};
use marginalia_core::Timestamp;

use crate::classification::DeviceOperation;

/// Private constructor witness. Nothing outside this module can name it, so
/// nothing outside this crate can build a [`WriteGrant`].
#[derive(Debug)]
pub(crate) struct Seal;

/// Authorisation to perform exactly one device write.
///
/// Deliberately not `Clone`, not `Copy`, and not `Serialize`/`Deserialize`: a
/// grant must not be duplicated, stored, queued, or replayed. It is created,
/// used once, and dropped.
#[derive(Debug)]
pub struct WriteGrant {
    operation: DeviceOperation,
    device_id: DeviceId,
    document_id: Option<DocumentId>,
    snapshot_id: Option<SnapshotId>,
    issued_at: Timestamp,
    ttl_secs: i64,
    #[allow(dead_code)]
    seal: Seal,
}

impl WriteGrant {
    /// Crate-private. Called by [`crate::SafetyManager`] and nowhere else.
    pub(crate) fn issue(
        operation: DeviceOperation,
        device_id: DeviceId,
        document_id: Option<DocumentId>,
        snapshot_id: Option<SnapshotId>,
        issued_at: Timestamp,
        ttl_secs: i64,
    ) -> Self {
        Self {
            operation,
            device_id,
            document_id,
            snapshot_id,
            issued_at,
            ttl_secs,
            seal: Seal,
        }
    }

    pub fn operation(&self) -> DeviceOperation {
        self.operation
    }

    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    pub fn document_id(&self) -> Option<&DocumentId> {
        self.document_id.as_ref()
    }

    pub fn snapshot_id(&self) -> Option<&SnapshotId> {
        self.snapshot_id.as_ref()
    }

    pub fn issued_at(&self) -> Timestamp {
        self.issued_at
    }

    /// Whether the grant is still valid at `now`.
    ///
    /// A grant that has sat around while the world changed — device
    /// disconnected, storage filled, firmware updated — must not be honoured.
    pub fn is_valid_at(&self, now: Timestamp) -> bool {
        let age = now.signed_duration_since(self.issued_at);
        age >= Duration::zero() && age <= Duration::seconds(self.ttl_secs)
    }

    /// Whether this grant covers the operation the caller is about to perform.
    ///
    /// The executor calls this immediately before acting, so that a grant for
    /// "upload document X" cannot be used to remove document Y.
    pub fn covers(
        &self,
        operation: DeviceOperation,
        device: &DeviceId,
        document: Option<&DocumentId>,
    ) -> bool {
        self.operation == operation
            && &self.device_id == device
            && self.document_id.as_ref() == document
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn grant(op: DeviceOperation, device: &DeviceId, doc: Option<&DocumentId>) -> WriteGrant {
        WriteGrant::issue(
            op,
            device.clone(),
            doc.cloned(),
            Some(SnapshotId::new()),
            Utc::now(),
            300,
        )
    }

    #[test]
    fn a_grant_covers_only_its_own_operation() {
        let device = DeviceId::new();
        let doc = DocumentId::new();
        let g = grant(DeviceOperation::UploadDocument, &device, Some(&doc));

        assert!(g.covers(DeviceOperation::UploadDocument, &device, Some(&doc)));
        assert!(!g.covers(DeviceOperation::RemoveOwnedDocument, &device, Some(&doc)));
    }

    #[test]
    fn a_grant_covers_only_its_own_document() {
        let device = DeviceId::new();
        let doc_a = DocumentId::new();
        let doc_b = DocumentId::new();
        let g = grant(DeviceOperation::UploadDocument, &device, Some(&doc_a));

        assert!(!g.covers(DeviceOperation::UploadDocument, &device, Some(&doc_b)));
    }

    #[test]
    fn a_grant_covers_only_its_own_device() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let doc = DocumentId::new();
        let g = grant(DeviceOperation::UploadDocument, &device_a, Some(&doc));

        assert!(!g.covers(DeviceOperation::UploadDocument, &device_b, Some(&doc)));
    }

    #[test]
    fn a_grant_expires() {
        let device = DeviceId::new();
        let issued = Utc::now() - Duration::seconds(600);
        let g = WriteGrant::issue(
            DeviceOperation::UploadDocument,
            device,
            None,
            None,
            issued,
            300,
        );
        assert!(!g.is_valid_at(Utc::now()));
    }

    #[test]
    fn a_grant_from_the_future_is_invalid() {
        // Clock skew must not extend a grant's life.
        let device = DeviceId::new();
        let issued = Utc::now() + Duration::seconds(120);
        let g = WriteGrant::issue(
            DeviceOperation::UploadDocument,
            device,
            None,
            None,
            issued,
            300,
        );
        assert!(!g.is_valid_at(Utc::now()));
    }
}
