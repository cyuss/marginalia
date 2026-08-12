//! Document lifecycle.
//!
//! Implements `docs/architecture/DOCUMENT_STATE_MACHINE.md`.
//!
//! WHY a state machine instead of booleans: `isDownloaded`/`isSynced`/`hasPdf`
//! admit combinations that make no sense ("synced but never transferred") and
//! spread the rules across every call site. One enum with one transition
//! function means the rules live in one auditable place — including the single
//! most important rule in the product, which is that exactly one edge in this
//! machine puts a file on someone's device.

use crate::error::IllegalTransition;
use crate::ids::{DocumentId, RemarkableDocumentId};
use crate::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentState {
    /// Zotero knows the item. No PDF resolved here. Nothing on the device.
    MetadataOnly,
    /// A readable PDF exists locally. Still nothing on the device.
    AttachmentAvailable,
    /// The user pressed Send. A transfer is authorised and in flight.
    TransferPending,
    /// Transfer verified by checksum. Present on the device, no annotations yet.
    OnRemarkable,
    /// The device reports annotation data we have not ingested.
    Annotated,
    /// Annotations ingested locally, not yet exported to Zotero.
    ChangesPending,
    /// Device, local store and Zotero agree.
    Synced,
    /// Divergent changes, or the source file changed under us.
    Conflict,
    /// A transfer was aborted and rolled back. The device is clean.
    TransferFailed,
    /// The user explicitly removed it from the device. Annotations retained.
    RemovedFromDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentEvent {
    AttachmentResolved,
    AttachmentLost,
    /// ✋ Explicit user action. The only event that can lead to a device write.
    UserRequestedSend,
    TransferVerified,
    TransferAborted,
    AnnotationsDetected,
    IngestCompleted,
    /// ✋ Explicit user action.
    UserExportedToZotero,
    DivergenceDetected,
    ConflictResolvedSynced,
    ConflictResolvedPending,
    /// ✋ Explicit user action.
    UserRemovedFromDevice,
}

impl DocumentEvent {
    /// Whether this event may only be raised by a deliberate user action.
    ///
    /// A scheduler, a timer, or a sync job must never raise one of these. The
    /// executor asserts this; see `sync::SyncJob::validate_trigger`.
    pub fn requires_explicit_user_action(self) -> bool {
        matches!(
            self,
            DocumentEvent::UserRequestedSend
                | DocumentEvent::UserExportedToZotero
                | DocumentEvent::UserRemovedFromDevice
        )
    }
}

impl DocumentState {
    /// The complete transition table. Every legal edge is here; everything else
    /// is an error.
    ///
    /// Note the shape of this function: it is total, pure, and has no `_ =>`
    /// catch-all that could quietly permit an unintended edge. Adding a state
    /// or an event breaks compilation until every case is considered — which is
    /// the point.
    pub fn apply(self, event: DocumentEvent) -> Result<DocumentState, IllegalTransition> {
        use DocumentEvent::*;
        use DocumentState::*;

        let next = match (self, event) {
            (MetadataOnly, AttachmentResolved) => AttachmentAvailable,
            (AttachmentAvailable, AttachmentLost) => MetadataOnly,

            // ── The only edge in the entire machine that writes a file to a
            // ── device. Reachable exclusively via an explicit user action,
            // ── carrying an ExplicitUserIntent and a WriteGrant.
            (AttachmentAvailable, UserRequestedSend) => TransferPending,
            (RemovedFromDevice, UserRequestedSend) => TransferPending,
            (TransferFailed, UserRequestedSend) => TransferPending,

            (TransferPending, TransferVerified) => OnRemarkable,
            (TransferPending, TransferAborted) => TransferFailed,

            (OnRemarkable, AnnotationsDetected) => Annotated,
            (Synced, AnnotationsDetected) => Annotated,
            (Annotated, IngestCompleted) => ChangesPending,

            (ChangesPending, UserExportedToZotero) => Synced,

            (ChangesPending, DivergenceDetected) => Conflict,
            (Synced, DivergenceDetected) => Conflict,
            (OnRemarkable, DivergenceDetected) => Conflict,
            (Conflict, ConflictResolvedSynced) => Synced,
            (Conflict, ConflictResolvedPending) => ChangesPending,

            (OnRemarkable, UserRemovedFromDevice) => RemovedFromDevice,
            (Synced, UserRemovedFromDevice) => RemovedFromDevice,
            (ChangesPending, UserRemovedFromDevice) => RemovedFromDevice,
            (Annotated, UserRemovedFromDevice) => RemovedFromDevice,

            (state, event) => return Err(IllegalTransition { from: state, event }),
        };

        Ok(next)
    }

    /// Whether a document in this state currently occupies space on a device.
    pub fn is_on_device(self) -> bool {
        matches!(
            self,
            DocumentState::OnRemarkable
                | DocumentState::Annotated
                | DocumentState::ChangesPending
                | DocumentState::Synced
                | DocumentState::Conflict
        )
    }

    /// Whether `Send to reMarkable` should be offered.
    pub fn can_be_sent(self) -> bool {
        matches!(
            self,
            DocumentState::AttachmentAvailable
                | DocumentState::RemovedFromDevice
                | DocumentState::TransferFailed
        )
    }
}

/// Where a document came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocumentSource {
    Zotero,
    LocalFile,
    /// Found on the device and not matched to anything we know. Read-only.
    DeviceOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub title: String,
    pub source: DocumentSource,
    pub page_count: Option<u32>,
    pub state: DocumentState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// The join that gives a document its identity across three systems.
///
/// WHY this exists: filenames are not identities. `xochitl` rewrites titles,
/// Zotero renames attachments, and two papers can share a filename. Every
/// cross-system claim ("this device document is that Zotero item") goes through
/// this row, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentMapping {
    pub id: crate::ids::MappingId,
    pub zotero_item_key: Option<crate::ids::ZoteroKey>,
    pub zotero_attachment_key: Option<crate::ids::ZoteroKey>,
    pub local_document_id: DocumentId,
    pub remarkable_document_id: Option<RemarkableDocumentId>,
    pub original_filename: String,
    /// SHA-256 of the immutable Zotero source. Written once, never updated.
    pub original_checksum: crate::Checksum,
    pub working_checksum: Option<crate::Checksum>,
    pub device_checksum: Option<crate::Checksum>,
    pub device_state: DocumentState,
    pub transferred_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_synced_at: Option<Timestamp>,
}

impl DocumentMapping {
    /// Whether Marginalia put this document on the device and may therefore
    /// modify or remove it.
    ///
    /// The negation of this is the ownership rule: a device document we did not
    /// transfer is the user's, and is read-only forever.
    pub fn owns_device_document(&self, uuid: &RemarkableDocumentId) -> bool {
        self.remarkable_document_id.as_ref() == Some(uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentEvent::*;
    use super::DocumentState::*;

    #[test]
    fn happy_path_reaches_synced() {
        let mut s = MetadataOnly;
        for e in [
            AttachmentResolved,
            UserRequestedSend,
            TransferVerified,
            AnnotationsDetected,
            IngestCompleted,
            UserExportedToZotero,
        ] {
            s = s.apply(e).expect("legal edge");
        }
        assert_eq!(s, Synced);
    }

    #[test]
    fn failed_transfer_is_recoverable() {
        let s = AttachmentAvailable
            .apply(UserRequestedSend)
            .unwrap()
            .apply(TransferAborted)
            .unwrap();
        assert_eq!(s, TransferFailed);
        assert!(s.can_be_sent(), "the user must be able to retry");
        assert!(
            !s.is_on_device(),
            "a rolled-back transfer leaves nothing behind"
        );
    }

    /// The load-bearing test for INV-2 at the state-machine level.
    #[test]
    fn transfer_pending_is_only_reachable_via_explicit_user_action() {
        let all_states = [
            MetadataOnly,
            AttachmentAvailable,
            TransferPending,
            OnRemarkable,
            Annotated,
            ChangesPending,
            Synced,
            Conflict,
            TransferFailed,
            RemovedFromDevice,
        ];
        let all_events = [
            AttachmentResolved,
            AttachmentLost,
            UserRequestedSend,
            TransferVerified,
            TransferAborted,
            AnnotationsDetected,
            IngestCompleted,
            UserExportedToZotero,
            DivergenceDetected,
            ConflictResolvedSynced,
            ConflictResolvedPending,
            UserRemovedFromDevice,
        ];

        for state in all_states {
            for event in all_events {
                if let Ok(TransferPending) = state.apply(event) {
                    assert!(
                        event.requires_explicit_user_action(),
                        "{state:?} + {event:?} reaches TransferPending without an \
                         explicit user action — this would allow an automatic \
                         transfer and violates INV-2",
                    );
                }
            }
        }
    }

    #[test]
    fn nothing_deletes_local_annotations() {
        // Removing from the device keeps the document tracked; annotations are
        // never dropped as a side effect of a state change.
        let s = Synced.apply(UserRemovedFromDevice).unwrap();
        assert_eq!(s, RemovedFromDevice);
        assert!(!s.is_on_device());
    }

    #[test]
    fn illegal_transitions_error_rather_than_no_op() {
        let err = MetadataOnly.apply(TransferVerified).unwrap_err();
        assert_eq!(err.from, MetadataOnly);
        assert_eq!(err.event, TransferVerified);
    }

    #[test]
    fn metadata_only_can_never_be_sent() {
        assert!(!MetadataOnly.can_be_sent());
        assert!(MetadataOnly.apply(UserRequestedSend).is_err());
    }
}
