//! Every operation that can touch a device, and how dangerous it is.
//!
//! Implements the classification table in `docs/safety/SAFETY_MODEL.md` §2.

use marginalia_core::device::Capability;
use serde::{Deserialize, Serialize};

/// The four-level classification.
///
/// Ordering matters: `Green < Yellow < Orange < Red`, so policy checks can be
/// written as comparisons rather than as easily-forgotten match arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SafetyClass {
    /// Read-only. No device state changes.
    Green,
    /// Controlled, reversible, verified write. User-initiated.
    Yellow,
    /// Experimental integration. Flag-gated OFF, removable, never under Safe Mode.
    Orange,
    /// System modification. Never implemented, under any flag, in any mode.
    Red,
}

impl SafetyClass {
    // `matches!` rather than `self > SafetyClass::Green`: comparison operators
    // are not usable in const fn, and these need to be const so that the
    // classification table below is evaluated at compile time.
    pub const fn changes_device_state(self) -> bool {
        matches!(
            self,
            SafetyClass::Yellow | SafetyClass::Orange | SafetyClass::Red
        )
    }

    /// Whether this class is permanently prohibited.
    pub const fn is_prohibited(self) -> bool {
        matches!(self, SafetyClass::Red)
    }
}

/// The complete set of device-touching operations Marginalia can name.
///
/// The `Red` variants are here so the safety layer can refuse them *by name*
/// and log the attempt. There is no implementation behind any of them, and
/// adding one would be a change to `DEVICE_WRITE_POLICY.md` first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceOperation {
    // ── GREEN ────────────────────────────────────────────────────────────
    DetectDevice,
    ReadDeviceInfo,
    ReadFirmwareVersion,
    ReadStorage,
    ListDocuments,
    ReadDocumentMetadata,
    ReadNativeTags,
    ReadAnnotations,
    ReadThumbnail,

    // ── YELLOW ───────────────────────────────────────────────────────────
    /// The four whitelisted writes, and nothing else.
    UploadDocument,
    RemoveOwnedDocument,
    WriteNativeTags,
    WriteDerivedAnnotatedPdf,

    // ── ORANGE ───────────────────────────────────────────────────────────
    InstallCompanionApp,
    ExperimentalRmUi,

    // ── RED — named only so they can be refused ──────────────────────────
    PatchXochitl,
    ModifySystemPartition,
    ModifyBootloaderOrKernel,
    ReplaceSystemLibrary,
    DisableFirmwareUpdates,
    InstallPackageManager,
    DeleteUserDocument,
    OverwriteOriginalPdf,
}

impl DeviceOperation {
    /// The classification. Total, with no catch-all: adding an operation forces
    /// a deliberate decision about how dangerous it is.
    pub const fn classify(self) -> SafetyClass {
        use DeviceOperation::*;
        match self {
            DetectDevice | ReadDeviceInfo | ReadFirmwareVersion | ReadStorage | ListDocuments
            | ReadDocumentMetadata | ReadNativeTags | ReadAnnotations | ReadThumbnail => {
                SafetyClass::Green
            }

            UploadDocument | RemoveOwnedDocument | WriteNativeTags | WriteDerivedAnnotatedPdf => {
                SafetyClass::Yellow
            }

            InstallCompanionApp | ExperimentalRmUi => SafetyClass::Orange,

            PatchXochitl
            | ModifySystemPartition
            | ModifyBootloaderOrKernel
            | ReplaceSystemLibrary
            | DisableFirmwareUpdates
            | InstallPackageManager
            | DeleteUserDocument
            | OverwriteOriginalPdf => SafetyClass::Red,
        }
    }

    /// The device capability this operation needs.
    pub const fn required_capability(self) -> Capability {
        use DeviceOperation::*;
        match self {
            DetectDevice | ReadDeviceInfo | ReadFirmwareVersion => Capability::DeviceInfoRead,
            ReadStorage => Capability::StorageRead,
            ListDocuments | ReadDocumentMetadata | ReadThumbnail => Capability::MetadataRead,
            ReadNativeTags => Capability::NativeTagsRead,
            ReadAnnotations => Capability::AnnotationRead,

            UploadDocument => Capability::SafeDocumentTransfer,
            RemoveOwnedDocument => Capability::DocumentRemoval,
            WriteNativeTags => Capability::NativeTagsWrite,
            WriteDerivedAnnotatedPdf => Capability::PdfAnnotationExport,

            InstallCompanionApp => Capability::CompanionApp,
            ExperimentalRmUi => Capability::ExperimentalRmUi,

            PatchXochitl
            | ModifySystemPartition
            | ModifyBootloaderOrKernel
            | ReplaceSystemLibrary
            | DisableFirmwareUpdates
            | InstallPackageManager
            | DeleteUserDocument
            | OverwriteOriginalPdf => Capability::SystemModification,
        }
    }

    /// Whether this operation requires a verified snapshot beforehand.
    pub const fn requires_snapshot(self) -> bool {
        matches!(
            self,
            DeviceOperation::UploadDocument
                | DeviceOperation::RemoveOwnedDocument
                | DeviceOperation::WriteDerivedAnnotatedPdf
        )
    }

    /// Whether this operation requires a fresh `ExplicitUserIntent`.
    pub const fn requires_user_intent(self) -> bool {
        self.classify().changes_device_state()
    }

    /// The four operations on the write whitelist.
    pub const WHITELISTED_WRITES: [DeviceOperation; 4] = [
        DeviceOperation::UploadDocument,
        DeviceOperation::RemoveOwnedDocument,
        DeviceOperation::WriteNativeTags,
        DeviceOperation::WriteDerivedAnnotatedPdf,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[DeviceOperation] = &[
        DeviceOperation::DetectDevice,
        DeviceOperation::ReadDeviceInfo,
        DeviceOperation::ReadFirmwareVersion,
        DeviceOperation::ReadStorage,
        DeviceOperation::ListDocuments,
        DeviceOperation::ReadDocumentMetadata,
        DeviceOperation::ReadNativeTags,
        DeviceOperation::ReadAnnotations,
        DeviceOperation::ReadThumbnail,
        DeviceOperation::UploadDocument,
        DeviceOperation::RemoveOwnedDocument,
        DeviceOperation::WriteNativeTags,
        DeviceOperation::WriteDerivedAnnotatedPdf,
        DeviceOperation::InstallCompanionApp,
        DeviceOperation::ExperimentalRmUi,
        DeviceOperation::PatchXochitl,
        DeviceOperation::ModifySystemPartition,
        DeviceOperation::ModifyBootloaderOrKernel,
        DeviceOperation::ReplaceSystemLibrary,
        DeviceOperation::DisableFirmwareUpdates,
        DeviceOperation::InstallPackageManager,
        DeviceOperation::DeleteUserDocument,
        DeviceOperation::OverwriteOriginalPdf,
    ];

    #[test]
    fn every_operation_is_classified() {
        // Compilation already proves totality; this asserts the table is sane.
        for op in ALL {
            let class = op.classify();
            if class == SafetyClass::Green {
                assert!(!class.changes_device_state(), "{op:?}");
            }
        }
    }

    #[test]
    fn the_write_whitelist_is_exactly_four_operations() {
        let yellow: Vec<_> = ALL
            .iter()
            .copied()
            .filter(|op| op.classify() == SafetyClass::Yellow)
            .collect();
        assert_eq!(
            yellow.len(),
            4,
            "the device write policy allows exactly four writes; \
             adding a fifth requires changing DEVICE_WRITE_POLICY.md first"
        );
        assert_eq!(yellow, DeviceOperation::WHITELISTED_WRITES.to_vec());
    }

    #[test]
    fn everything_dangerous_is_red() {
        for op in [
            DeviceOperation::PatchXochitl,
            DeviceOperation::ModifySystemPartition,
            DeviceOperation::ModifyBootloaderOrKernel,
            DeviceOperation::ReplaceSystemLibrary,
            DeviceOperation::DisableFirmwareUpdates,
            DeviceOperation::InstallPackageManager,
            DeviceOperation::DeleteUserDocument,
            DeviceOperation::OverwriteOriginalPdf,
        ] {
            assert!(op.classify().is_prohibited(), "{op:?} must be RED");
            assert_eq!(op.required_capability(), Capability::SystemModification);
        }
    }

    #[test]
    fn reads_never_require_intent_or_snapshots() {
        for op in ALL.iter().filter(|o| o.classify() == SafetyClass::Green) {
            assert!(!op.requires_user_intent(), "{op:?}");
            assert!(!op.requires_snapshot(), "{op:?}");
        }
    }

    #[test]
    fn class_ordering_supports_policy_comparisons() {
        assert!(SafetyClass::Green < SafetyClass::Yellow);
        assert!(SafetyClass::Yellow < SafetyClass::Orange);
        assert!(SafetyClass::Orange < SafetyClass::Red);
    }
}
