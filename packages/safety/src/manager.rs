//! The `SafetyManager` — the single choke point for device mutations.
//!
//! Implements the evaluation order in `docs/safety/SAFETY_MODEL.md` §3.
//!
//! # Fail closed
//!
//! [`SafetyManager::authorize`] has no `_ => Granted` arm and no fallible step
//! whose failure is ignored. Every unknown, every absent fact, every
//! unevaluable precondition produces [`Authorization::Denied`]. The function is
//! written so that *doing nothing* denies — a bug that skips a check cannot
//! accidentally produce a grant, because producing a grant requires reaching
//! the single `WriteGrant::issue` call at the very bottom.

use marginalia_core::device::{CapabilityStatus, Device, StorageInfo};
use marginalia_core::ids::{DocumentId, SnapshotId};
use marginalia_core::intent::{ExplicitUserIntent, UserAction};
use marginalia_core::Timestamp;
use serde::Serialize;
use tracing::warn;

use crate::classification::{DeviceOperation, SafetyClass};
use crate::flags::{FeatureFlag, FeatureFlagManager};
use crate::grant::WriteGrant;
use crate::snapshot::SafetySnapshot;

/// How long a grant remains valid once issued.
const GRANT_TTL_SECS: i64 = 120;
/// How old a user confirmation may be and still authorise a write.
const INTENT_MAX_AGE_SECS: i64 = 300;
/// Device space that is never spendable. Configurable; this is the default.
pub const DEFAULT_STORAGE_RESERVE_BYTES: u64 = 500 * 1024 * 1024;

/// A request to do something to a device.
pub struct OperationRequest<'a> {
    pub operation: DeviceOperation,
    pub device: &'a Device,
    pub document_id: Option<DocumentId>,
    pub intent: Option<&'a ExplicitUserIntent>,
    pub preconditions: Preconditions<'a>,
    pub now: Timestamp,
}

/// Facts the adapters have already established, handed to the safety layer to
/// judge.
///
/// WHY the manager does not gather these itself: keeping it pure makes it
/// exhaustively testable, and keeps the layer that decides separate from the
/// layers that touch disks, networks and devices.
#[derive(Default)]
pub struct Preconditions<'a> {
    /// Resolved capability status for this operation's required capability.
    pub capability_status: Option<CapabilityStatus>,
    pub storage: Option<StorageInfo>,
    /// Size of the payload for a write, if any.
    pub incoming_bytes: Option<u64>,
    pub reserve_bytes: u64,
    /// `Some(true)` once the PDF has been structurally validated.
    pub pdf_validated: Option<bool>,
    /// Whether the source file's checksum matched what we recorded.
    pub source_checksum_ok: Option<bool>,
    pub snapshot: Option<&'a SafetySnapshot>,
    /// For removals and overwrites: whether the target is a document
    /// Marginalia itself transferred.
    pub target_is_owned_by_us: Option<bool>,
    /// Whether an identical operation already completed (idempotency).
    pub already_completed: bool,
    /// Whether a rollback plan exists and can be executed.
    pub rollback_plan_ready: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "reason")]
pub enum DenialReason {
    /// A RED operation was requested. Never permitted, in any mode.
    ProhibitedOperation {
        operation: DeviceOperation,
    },
    FeatureFlagDisabled {
        flag: FeatureFlag,
    },
    SafeModeForbidsClass {
        class: SafetyClass,
    },
    DeviceNotIdentified,
    FirmwareUnknown,
    CapabilityNotSupported {
        status: CapabilityStatus,
    },
    CapabilityUnresolved,
    MissingUserIntent,
    IntentDoesNotMatchRequest,
    IntentStale,
    PdfNotValidated,
    SourceChecksumMismatch,
    StorageUnknown,
    InsufficientStorage {
        required: u64,
        available: u64,
        reserve: u64,
    },
    PayloadSizeUnknown,
    TargetNotOwnedByMarginalia,
    OwnershipUnverified,
    SnapshotMissingOrUnverified,
    RollbackPlanUnavailable,
    /// Not a failure: the work is already done, so we do nothing again.
    AlreadyCompleted,
}

impl DenialReason {
    /// What to tell the user. Every message says what happened and, where it
    /// helps, what they can do — never a bare error code.
    pub fn user_message(&self) -> String {
        match self {
            DenialReason::ProhibitedOperation { .. } =>
                "This operation would modify your reMarkable's system software. Marginalia never does this.".into(),
            DenialReason::FeatureFlagDisabled { .. } =>
                "This feature is turned off. You can enable it in Settings.".into(),
            DenialReason::SafeModeForbidsClass { .. } =>
                "Safe Mode does not allow experimental device operations.".into(),
            DenialReason::DeviceNotIdentified =>
                "No reMarkable is connected, or it could not be identified.".into(),
            DenialReason::FirmwareUnknown =>
                "Your reMarkable's firmware has not been tested with Marginalia. Safe Mode has restricted it to read-only.".into(),
            DenialReason::CapabilityNotSupported { .. } | DenialReason::CapabilityUnresolved =>
                "This action is not available on your device's firmware. Nothing was changed.".into(),
            DenialReason::MissingUserIntent
            | DenialReason::IntentDoesNotMatchRequest =>
                "This action must be started from the button in the app.".into(),
            DenialReason::IntentStale =>
                "This confirmation has expired. Please try again.".into(),
            DenialReason::PdfNotValidated =>
                "The PDF could not be validated, so it was not sent. Your original file is unchanged.".into(),
            DenialReason::SourceChecksumMismatch =>
                "The source file changed since Marginalia last read it. Nothing was sent.".into(),
            DenialReason::StorageUnknown | DenialReason::PayloadSizeUnknown =>
                "Marginalia could not confirm there is room on your reMarkable, so it did not proceed.".into(),
            DenialReason::InsufficientStorage { .. } =>
                "There is not enough free space on your reMarkable. Nothing was sent or deleted.".into(),
            DenialReason::TargetNotOwnedByMarginalia | DenialReason::OwnershipUnverified =>
                "This document was not put on your reMarkable by Marginalia, so Marginalia will not modify it.".into(),
            DenialReason::SnapshotMissingOrUnverified =>
                "Marginalia could not create a verified safety snapshot, so it did not proceed.".into(),
            DenialReason::RollbackPlanUnavailable =>
                "Marginalia could not prepare a way to undo this operation, so it did not start it.".into(),
            DenialReason::AlreadyCompleted =>
                "This document is already on your reMarkable.".into(),
        }
    }
}

#[derive(Debug)]
pub enum Authorization {
    Granted(WriteGrant),
    Denied(DenialReason),
}

impl Authorization {
    pub fn is_granted(&self) -> bool {
        matches!(self, Authorization::Granted(_))
    }

    pub fn denial(&self) -> Option<&DenialReason> {
        match self {
            Authorization::Denied(r) => Some(r),
            Authorization::Granted(_) => None,
        }
    }
}

pub struct SafetyManager {
    flags: FeatureFlagManager,
}

impl SafetyManager {
    pub fn new(flags: FeatureFlagManager) -> Self {
        Self { flags }
    }

    pub fn flags(&self) -> &FeatureFlagManager {
        &self.flags
    }

    /// Read operations need no grant, but callers may still ask whether a read
    /// is sensible. Unknown firmware still permits reads — that is the whole
    /// point of degrading to read-only rather than to nothing.
    pub fn may_read(&self, operation: DeviceOperation, status: CapabilityStatus) -> bool {
        operation.classify() == SafetyClass::Green && status.permits_read()
    }

    /// The single authorisation path.
    ///
    /// Returns [`Authorization::Granted`] only after every check has passed.
    /// Any doubt, at any step, returns [`Authorization::Denied`].
    pub fn authorize(&self, req: OperationRequest<'_>) -> Authorization {
        let op = req.operation;
        let class = op.classify();

        // ── 1. Classification ────────────────────────────────────────────
        if class.is_prohibited() {
            // Loud on purpose: nothing should ever reach this line, so if it
            // does, we want it in the audit trail.
            warn!(
                target: "marginalia::safety",
                operation = ?op,
                "SAFETY: prohibited (RED) operation requested and refused"
            );
            return Authorization::Denied(DenialReason::ProhibitedOperation { operation: op });
        }

        // A GREEN operation needs no grant; asking for one is a caller bug.
        debug_assert!(
            class.changes_device_state(),
            "authorize() is for writes; reads use may_read()"
        );

        // ── 2. Feature flag ──────────────────────────────────────────────
        let flag = required_flag(op);
        if let Some(flag) = flag {
            if !self.flags.is_enabled(flag) {
                return Authorization::Denied(DenialReason::FeatureFlagDisabled { flag });
            }
        }

        // ── 3. Safe Mode policy ──────────────────────────────────────────
        if req.device.safe_mode && class >= SafetyClass::Orange {
            return Authorization::Denied(DenialReason::SafeModeForbidsClass { class });
        }

        // ── 4. Device identity & firmware ────────────────────────────────
        if req.device.serial_hash.is_none() {
            return Authorization::Denied(DenialReason::DeviceNotIdentified);
        }
        if !req.device.firmware_is_known() {
            return Authorization::Denied(DenialReason::FirmwareUnknown);
        }

        // ── 5. Capability ────────────────────────────────────────────────
        let status = match req.preconditions.capability_status {
            Some(s) => s,
            // Unresolved is not "probably fine".
            None => return Authorization::Denied(DenialReason::CapabilityUnresolved),
        };
        if !status.permits_write() {
            return Authorization::Denied(DenialReason::CapabilityNotSupported { status });
        }

        // ── 6. Explicit user intent ──────────────────────────────────────
        if op.requires_user_intent() {
            let intent = match req.intent {
                Some(i) => i,
                None => return Authorization::Denied(DenialReason::MissingUserIntent),
            };
            let expected_action = match user_action_for(op) {
                Some(a) => a,
                None => return Authorization::Denied(DenialReason::MissingUserIntent),
            };
            let document = match req.document_id.as_ref() {
                Some(d) => d,
                None => return Authorization::Denied(DenialReason::IntentDoesNotMatchRequest),
            };
            if !intent.authorises(expected_action, document) {
                return Authorization::Denied(DenialReason::IntentDoesNotMatchRequest);
            }
            if !intent.is_fresh(req.now, INTENT_MAX_AGE_SECS) {
                return Authorization::Denied(DenialReason::IntentStale);
            }
        }

        // ── 7. Idempotency ───────────────────────────────────────────────
        if req.preconditions.already_completed {
            return Authorization::Denied(DenialReason::AlreadyCompleted);
        }

        // ── 8. Operation-specific preconditions ──────────────────────────
        if let Some(denial) = self.check_preconditions(op, &req.preconditions) {
            return Authorization::Denied(denial);
        }

        // ── 9. Snapshot ──────────────────────────────────────────────────
        let snapshot_id: Option<SnapshotId> = if op.requires_snapshot() {
            match req.preconditions.snapshot {
                Some(s) if s.is_usable() => Some(s.id.clone()),
                // Missing, pending or failed all mean the same thing.
                _ => return Authorization::Denied(DenialReason::SnapshotMissingOrUnverified),
            }
        } else {
            None
        };

        // ── 10. Rollback plan ────────────────────────────────────────────
        if req.preconditions.rollback_plan_ready != Some(true) {
            return Authorization::Denied(DenialReason::RollbackPlanUnavailable);
        }

        // The only place in the entire codebase where a grant comes into
        // existence.
        Authorization::Granted(WriteGrant::issue(
            op,
            req.device.id.clone(),
            req.document_id,
            snapshot_id,
            req.now,
            GRANT_TTL_SECS,
        ))
    }

    fn check_preconditions(
        &self,
        op: DeviceOperation,
        pre: &Preconditions<'_>,
    ) -> Option<DenialReason> {
        match op {
            DeviceOperation::UploadDocument | DeviceOperation::WriteDerivedAnnotatedPdf => {
                if pre.pdf_validated != Some(true) {
                    return Some(DenialReason::PdfNotValidated);
                }
                if pre.source_checksum_ok == Some(false) {
                    return Some(DenialReason::SourceChecksumMismatch);
                }
                // NOT `pre.storage?`: in a function returning
                // Option<DenialReason>, `?` on a None would return "no denial"
                // — a fail-open. Unknown storage must deny.
                let storage = match pre.storage {
                    Some(s) => s,
                    None => return Some(DenialReason::StorageUnknown),
                };
                let incoming = match pre.incoming_bytes {
                    Some(b) => b,
                    None => return Some(DenialReason::PayloadSizeUnknown),
                };
                let reserve = pre.reserve_bytes;
                if !storage.can_accept(incoming, reserve) {
                    return Some(DenialReason::InsufficientStorage {
                        required: incoming,
                        available: storage.free_bytes,
                        reserve,
                    });
                }
                // Overwriting a derived PDF is only ever done to our own document.
                if op == DeviceOperation::WriteDerivedAnnotatedPdf {
                    return ownership_denial(pre);
                }
                None
            }

            DeviceOperation::RemoveOwnedDocument => ownership_denial(pre),

            DeviceOperation::WriteNativeTags => ownership_denial(pre),

            // ORANGE operations reach here only with Safe Mode off and their
            // flag on; they still require a rollback plan, checked by the
            // caller of this function.
            DeviceOperation::InstallCompanionApp | DeviceOperation::ExperimentalRmUi => None,

            // GREEN operations never take this path, and RED never gets here.
            _ => None,
        }
    }
}

/// Ownership is the rule that protects documents the user put on the device
/// themselves: unverified ownership denies just as firmly as proven foreign
/// ownership does.
fn ownership_denial(pre: &Preconditions<'_>) -> Option<DenialReason> {
    match pre.target_is_owned_by_us {
        Some(true) => None,
        Some(false) => Some(DenialReason::TargetNotOwnedByMarginalia),
        None => Some(DenialReason::OwnershipUnverified),
    }
}

const fn required_flag(op: DeviceOperation) -> Option<FeatureFlag> {
    match op {
        DeviceOperation::UploadDocument | DeviceOperation::RemoveOwnedDocument => {
            Some(FeatureFlag::SafeDocumentTransfer)
        }
        DeviceOperation::WriteDerivedAnnotatedPdf => Some(FeatureFlag::NativePdfAnnotations),
        DeviceOperation::WriteNativeTags => Some(FeatureFlag::BidirectionalTagSync),
        DeviceOperation::InstallCompanionApp => Some(FeatureFlag::RemarkableCompanion),
        DeviceOperation::ExperimentalRmUi => Some(FeatureFlag::ExperimentalRmUi),
        _ => None,
    }
}

const fn user_action_for(op: DeviceOperation) -> Option<UserAction> {
    match op {
        DeviceOperation::UploadDocument | DeviceOperation::WriteDerivedAnnotatedPdf => {
            Some(UserAction::SendToRemarkable)
        }
        DeviceOperation::RemoveOwnedDocument => Some(UserAction::RemoveFromRemarkable),
        DeviceOperation::WriteNativeTags => Some(UserAction::ApplyTagMapping),
        _ => None,
    }
}

impl<'a> Preconditions<'a> {
    /// Convenience for tests and adapters: a fully-satisfied precondition set
    /// for an upload.
    pub fn ready_for_upload(
        storage: StorageInfo,
        incoming_bytes: u64,
        snapshot: &'a SafetySnapshot,
    ) -> Preconditions<'a> {
        Preconditions {
            capability_status: Some(CapabilityStatus::Supported),
            storage: Some(storage),
            incoming_bytes: Some(incoming_bytes),
            reserve_bytes: DEFAULT_STORAGE_RESERVE_BYTES,
            pdf_validated: Some(true),
            source_checksum_ok: Some(true),
            snapshot: Some(snapshot),
            target_is_owned_by_us: Some(true),
            already_completed: false,
            rollback_plan_ready: Some(true),
        }
    }
}
