//! # The mandatory safety suite
//!
//! Each test maps to an entry in `docs/safety/SAFETY_MODEL.md` §9. These are
//! never skipped and never marked flaky. A pull request with a failing test
//! here does not merge; if a test is wrong, it is fixed in its own pull request
//! with a written justification.
//!
//! Tests that require the transfer pipeline (Phase 3) or the annotation
//! extractor (Phase 4) are present as `#[ignore]` stubs naming what they will
//! assert, so the gap is visible rather than forgotten.

use chrono::{Duration, Utc};

use marginalia_core::device::{
    Capability, CapabilityStatus, ConnectionKind, Device, DeviceKind, FirmwareVersion, StorageInfo,
};
use marginalia_core::ids::ZoteroKey;
use marginalia_core::ids::{DeviceId, DocumentId, RemarkableDocumentId};
use marginalia_core::intent::{ExplicitUserIntent, UserAction};
use marginalia_core::sync::{JobTrigger, MetadataOperation, SyncJobKind, SyncPlan};
use marginalia_core::zotero::AttachmentAvailability;
use marginalia_core::Checksum;

use marginalia_remarkable::provider::{DeviceIntrospection, RemoteDeviceTransport, ValidatedPdf};
use marginalia_remarkable::CapabilityResolver;

use marginalia_safety::classification::DeviceOperation;
use marginalia_safety::manager::{
    DenialReason, OperationRequest, Preconditions, DEFAULT_STORAGE_RESERVE_BYTES,
};
use marginalia_safety::snapshot::{AffectedDocument, SafetySnapshot};
use marginalia_safety::{Authorization, FeatureFlag, FeatureFlagManager, SafetyManager};

use marginalia_simulator::{DeviceProfile, Fault, FaultScript, SimulatedDevice};

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

// ── helpers ─────────────────────────────────────────────────────────────────

fn device(firmware: Option<&str>, free_bytes: u64) -> Device {
    Device {
        id: DeviceId::from_string("sim-device"),
        kind: DeviceKind::Rm2,
        serial_hash: Some("hash".into()),
        display_name: "reMarkable 2".into(),
        firmware: firmware.map(|f| FirmwareVersion::parse(f).unwrap()),
        connection: ConnectionKind::Usb,
        last_seen_at: None,
        storage: Some(StorageInfo {
            total_bytes: 6 * GB,
            free_bytes,
        }),
        safe_mode: true,
    }
}

/// A manager with transfers enabled — i.e. as it will be once Phase 3 ships.
/// Tests that need to reach *later* checks must get past the flag gate first.
fn manager_with_transfers_enabled() -> SafetyManager {
    let mut flags = FeatureFlagManager::new();
    flags.set(FeatureFlag::SafeDocumentTransfer, true);
    SafetyManager::new(flags)
}

fn verified_snapshot(doc: &DocumentId) -> SafetySnapshot {
    let mut snap = SafetySnapshot::pending(
        DeviceId::from_string("sim-device"),
        DeviceOperation::UploadDocument,
        vec![AffectedDocument {
            document_id: doc.clone(),
            checksum_before: None,
        }],
        Some(3 * GB),
        Utc::now(),
    );
    snap.verify();
    snap
}

fn send_intent(doc: &DocumentId) -> ExplicitUserIntent {
    ExplicitUserIntent::record(UserAction::SendToRemarkable, doc.clone(), Utc::now())
}

fn sample_pdf() -> ValidatedPdf {
    ValidatedPdf::new(
        "/tmp/marginalia-working/paper.pdf",
        Checksum::of_bytes(b"a valid pdf"),
        84 * MB,
        15,
    )
}

// ── On-device introspection ────────────────────────────────────────────────

/// The standalone runtime asks the machine it runs on about itself. That must
/// be possible without a grant, without a transport, and without any ability to
/// change anything — the port's whole surface is two reads.
#[test]
fn on_device_introspection_needs_no_grant_and_cannot_write() {
    let sim = SimulatedDevice::new(DeviceProfile::known_healthy());

    let info = DeviceIntrospection::device_info(&sim).expect("identity");
    assert!(info.firmware_is_known());

    let storage = DeviceIntrospection::storage(&sim).expect("storage");
    assert!(storage.free_bytes > 0);

    // Reading told us nothing was written.
    assert_eq!(sim.write_count(), 0);
    assert_eq!(sim.document_count(), 0);
}

/// Storage introspection exists so the device can enforce its own reserve
/// before writing anything of its own -- the standalone runtime shares the
/// disk with the user's documents.
#[test]
fn on_device_storage_feeds_the_reserve_check() {
    let sim = SimulatedDevice::new(DeviceProfile::low_storage());
    let storage = DeviceIntrospection::storage(&sim).unwrap();

    assert!(
        !storage.can_accept(84 * MB, DEFAULT_STORAGE_RESERVE_BYTES),
        "a device below its reserve must refuse its own writes too, not just transfers"
    );
}

// ── S1 ──────────────────────────────────────────────────────────────────────

#[test]
fn s1_unknown_firmware_denies_every_write() {
    let manager = manager_with_transfers_enabled();
    let dev = device(None, 3 * GB); // firmware unparseable ⇒ unknown
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::FirmwareUnknown));
}

#[test]
fn s1_unknown_firmware_still_permits_reads() {
    // Degrading to read-only is the point; degrading to nothing would be a
    // worse product for no extra safety.
    let manager = SafetyManager::new(FeatureFlagManager::new());
    assert!(manager.may_read(DeviceOperation::ListDocuments, CapabilityStatus::Unknown));
    assert!(manager.may_read(DeviceOperation::ReadStorage, CapabilityStatus::Unknown));
}

#[test]
fn s1_the_capability_resolver_agrees() {
    let resolver = CapabilityResolver::bundled();
    let dev = device(None, 3 * GB);
    assert_eq!(
        resolver.resolve(&dev, Capability::SafeDocumentTransfer),
        CapabilityStatus::Unknown
    );
}

// ── S2 ──────────────────────────────────────────────────────────────────────

#[test]
fn s2_insufficient_storage_denies_the_transfer() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 400 * MB); // below the 500 MB reserve
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    match auth.denial() {
        Some(DenialReason::InsufficientStorage { reserve, .. }) => {
            assert_eq!(*reserve, DEFAULT_STORAGE_RESERVE_BYTES);
        }
        other => panic!("expected InsufficientStorage, got {other:?}"),
    }
}

#[test]
fn s2_unknown_storage_denies_rather_than_assuming_room() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.storage = None; // the read failed

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(
        auth.denial(),
        Some(&DenialReason::StorageUnknown),
        "an unknown quantity must deny; fail closed, never open"
    );
}

// ── S3 ──────────────────────────────────────────────────────────────────────

#[test]
fn s3_an_unvalidated_pdf_is_denied_before_the_device_is_contacted() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.pdf_validated = Some(false);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::PdfNotValidated));
}

#[test]
fn s3_a_changed_source_file_is_denied() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.source_checksum_ok = Some(false);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::SourceChecksumMismatch));
}

// ── S4 / S5: corrupted and interrupted transfers ───────────────────────────

#[test]
fn s4_a_corrupted_transfer_is_detectable_by_checksum() {
    // The pipeline lands in Phase 3; what Phase 0 must guarantee is that the
    // simulator can produce this failure and that a checksum comparison
    // catches it. A pipeline that "succeeds" here would be the worst bug in
    // the product.
    let mut sim = SimulatedDevice::new(DeviceProfile::known_healthy())
        .with_faults(FaultScript::once("upload_document", Fault::TruncatedWrite));

    let manager = manager_with_transfers_enabled();
    let dev = sim.device();
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.capability_status = Some(CapabilityStatus::Supported);

    let grant = match manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    }) {
        Authorization::Granted(g) => g,
        Authorization::Denied(r) => panic!("expected a grant, got {r:?}"),
    };

    let pdf = sample_pdf();
    let uuid = sim
        .upload_document(&grant, &pdf, "Attention Is All You Need")
        .unwrap();

    let on_device = sim.checksum_of(&uuid).unwrap();
    assert!(
        pdf.checksum().verify(&on_device).is_err(),
        "a corrupted transfer must fail verification"
    );

    // …and rollback must leave the device clean.
    sim.rollback_upload(&grant, &uuid).unwrap();
    assert!(!sim.contains(&uuid));
    assert_eq!(sim.document_count(), 0);
}

#[test]
fn s5_a_lost_connection_leaves_nothing_behind() {
    let mut sim = SimulatedDevice::new(DeviceProfile::known_healthy())
        .with_faults(FaultScript::once("upload_document", Fault::ConnectionLost));

    let manager = manager_with_transfers_enabled();
    let dev = sim.device();
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.capability_status = Some(CapabilityStatus::Supported);

    let grant = match manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    }) {
        Authorization::Granted(g) => g,
        Authorization::Denied(r) => panic!("expected a grant, got {r:?}"),
    };

    assert!(sim.upload_document(&grant, &sample_pdf(), "Paper").is_err());
    assert_eq!(
        sim.document_count(),
        0,
        "an interrupted transfer must not leave a partial document"
    );
}

// ── S7 ──────────────────────────────────────────────────────────────────────

#[test]
fn s7_a_duplicate_send_is_a_no_op() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.already_completed = true;

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::AlreadyCompleted));
    assert!(!auth.is_granted(), "a second copy must never be created");
}

// ── S8 / S9 / S10: the central product promise ─────────────────────────────

#[test]
fn s8_a_metadata_sync_transfers_no_pdfs() {
    let plan = SyncPlan::new(
        SyncJobKind::ZoteroMetadata,
        vec![
            MetadataOperation::UpsertZoteroItem {
                key: ZoteroKey::from_string("ABCD1234"),
            },
            MetadataOperation::UpsertAttachmentAvailability {
                key: ZoteroKey::from_string("EFGH5678"),
                availability: AttachmentAvailability::AvailableLocal,
            },
        ],
    );
    assert_eq!(plan.pdf_transfer_count(), 0);
}

#[test]
fn s9_a_library_full_of_available_pdfs_still_transfers_nothing() {
    // The realistic scenario: 500 papers, every one with a local PDF. A sync
    // must record 500 facts and move zero bytes to any device.
    let ops: Vec<MetadataOperation> = (0..500)
        .map(|i| MetadataOperation::UpsertAttachmentAvailability {
            key: ZoteroKey::from_string(format!("KEY{i:05}")),
            availability: AttachmentAvailability::AvailableLocal,
        })
        .collect();

    let plan = SyncPlan::new(SyncJobKind::ZoteroMetadata, ops);
    assert_eq!(plan.operations.len(), 500);
    assert_eq!(
        plan.pdf_transfer_count(),
        0,
        "availability is a fact we record, not an action we take"
    );

    // And a sim device confirms nothing arrived.
    let sim = SimulatedDevice::new(DeviceProfile::known_healthy());
    assert_eq!(sim.write_count(), 0);
    assert_eq!(sim.document_count(), 0);
}

#[test]
fn s10_one_click_produces_exactly_one_transfer() {
    let mut sim = SimulatedDevice::new(DeviceProfile::known_healthy());
    let manager = manager_with_transfers_enabled();
    let dev = sim.device();
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.capability_status = Some(CapabilityStatus::Supported);

    let grant = match manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    }) {
        Authorization::Granted(g) => g,
        Authorization::Denied(r) => panic!("expected a grant, got {r:?}"),
    };

    sim.upload_document(&grant, &sample_pdf(), "Attention Is All You Need")
        .unwrap();

    assert_eq!(sim.write_count(), 1, "exactly one write");
    assert_eq!(sim.document_count(), 1, "exactly one document");
}

#[test]
fn s10_a_schedule_cannot_start_a_transfer_job() {
    assert!(!SyncJobKind::Transfer.may_be_triggered_by(JobTrigger::Schedule));
    assert!(!SyncJobKind::Transfer.may_be_triggered_by(JobTrigger::Startup));
    assert!(!SyncJobKind::Removal.may_be_triggered_by(JobTrigger::Schedule));
}

// ── Intent ─────────────────────────────────────────────────────────────────

#[test]
fn a_write_without_user_intent_is_denied() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: None,
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::MissingUserIntent));
}

#[test]
fn an_intent_for_a_different_document_is_denied() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc_a = DocumentId::new();
    let doc_b = DocumentId::new();
    let snap = verified_snapshot(&doc_a);
    let intent = send_intent(&doc_b); // the user confirmed a different paper

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc_a),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    assert_eq!(
        auth.denial(),
        Some(&DenialReason::IntentDoesNotMatchRequest)
    );
}

#[test]
fn a_stale_confirmation_is_denied() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = ExplicitUserIntent::record(
        UserAction::SendToRemarkable,
        doc.clone(),
        Utc::now() - Duration::seconds(3600),
    );

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::IntentStale));
}

// ── S12 ─────────────────────────────────────────────────────────────────────

#[test]
fn s12_a_disabled_feature_cannot_be_exercised() {
    let manager = SafetyManager::new(FeatureFlagManager::new()); // all flags off
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    assert_eq!(
        auth.denial(),
        Some(&DenialReason::FeatureFlagDisabled {
            flag: FeatureFlag::SafeDocumentTransfer
        })
    );
}

#[test]
fn s12_safe_mode_forbids_experimental_operations() {
    let mut flags = FeatureFlagManager::new();
    flags.set(FeatureFlag::RemarkableCompanion, true); // even with the flag on
    let manager = SafetyManager::new(flags);

    let dev = device(Some("3.11.2"), 3 * GB); // safe_mode: true
    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::InstallCompanionApp,
        device: &dev,
        document_id: None,
        intent: None,
        preconditions: Preconditions::default(),
        now: Utc::now(),
    });

    assert!(matches!(
        auth.denial(),
        Some(DenialReason::SafeModeForbidsClass { .. })
    ));
}

// ── S13 ─────────────────────────────────────────────────────────────────────

#[test]
fn s13_every_prohibited_operation_is_refused_by_name() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);

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
        let auth = manager.authorize(OperationRequest {
            operation: op,
            device: &dev,
            document_id: None,
            intent: None,
            preconditions: Preconditions::default(),
            now: Utc::now(),
        });
        assert_eq!(
            auth.denial(),
            Some(&DenialReason::ProhibitedOperation { operation: op }),
            "{op:?} must be refused unconditionally"
        );
    }
}

#[test]
fn s13_a_red_operation_is_refused_even_with_perfect_preconditions() {
    // Every gate satisfied, safe mode off, flags on — still refused. RED is
    // not a matter of configuration.
    let mut flags = FeatureFlagManager::new();
    for flag in FeatureFlag::ALL {
        flags.set(flag, true);
    }
    let manager = SafetyManager::new(flags);

    let mut dev = device(Some("3.11.2"), 3 * GB);
    dev.safe_mode = false;

    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::PatchXochitl,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 1, &snap),
        now: Utc::now(),
    });

    assert!(matches!(
        auth.denial(),
        Some(DenialReason::ProhibitedOperation { .. })
    ));
}

// ── S14 ─────────────────────────────────────────────────────────────────────

#[test]
fn s14_a_failed_rollback_is_reported_rather_than_retried() {
    let mut sim = SimulatedDevice::new(DeviceProfile::known_healthy()).with_faults(
        FaultScript::new()
            .on("upload_document", 1, Fault::ChecksumMismatch)
            .on("rollback_upload", 1, Fault::RollbackFails),
    );

    let manager = manager_with_transfers_enabled();
    let dev = sim.device();
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.capability_status = Some(CapabilityStatus::Supported);

    let grant = match manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    }) {
        Authorization::Granted(g) => g,
        Authorization::Denied(r) => panic!("expected a grant, got {r:?}"),
    };

    let uuid = sim.upload_document(&grant, &sample_pdf(), "Paper").unwrap();
    let result = sim.rollback_upload(&grant, &uuid);

    assert!(
        result.is_err(),
        "a failed rollback must surface as an error the caller has to handle, \
         not be swallowed"
    );
    // The caller's obligation (Phase 3): mark the device read-only and raise a
    // blocking notice. Asserted end-to-end once the pipeline exists.
}

// ── S15 ─────────────────────────────────────────────────────────────────────

#[test]
fn s15_a_document_marginalia_did_not_transfer_is_never_removed() {
    let mut sim = SimulatedDevice::new(DeviceProfile::populated_with_user_documents());
    let manager = manager_with_transfers_enabled();
    let dev = sim.device();
    let doc = DocumentId::new();
    let foreign = RemarkableDocumentId::from_string("user-pdf-1");

    let pre = Preconditions {
        capability_status: Some(CapabilityStatus::Supported),
        // The mapping lookup said "not ours".
        target_is_owned_by_us: Some(false),
        rollback_plan_ready: Some(true),
        ..Default::default()
    };
    let intent =
        ExplicitUserIntent::record(UserAction::RemoveFromRemarkable, doc.clone(), Utc::now());

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::RemoveOwnedDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(
        auth.denial(),
        Some(&DenialReason::TargetNotOwnedByMarginalia)
    );

    // Belt and braces: the device still holds the user's documents.
    assert!(sim.contains(&foreign));
    assert_eq!(sim.write_count(), 0);
    let _ = &mut sim;
}

#[test]
fn s15_unverified_ownership_denies_just_as_firmly() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();

    let pre = Preconditions {
        capability_status: Some(CapabilityStatus::Supported),
        // We could not check.
        target_is_owned_by_us: None,
        rollback_plan_ready: Some(true),
        ..Default::default()
    };
    let intent =
        ExplicitUserIntent::record(UserAction::RemoveFromRemarkable, doc.clone(), Utc::now());

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::RemoveOwnedDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::OwnershipUnverified));
}

// ── Snapshot & rollback preconditions ──────────────────────────────────────

#[test]
fn an_unverified_snapshot_denies_the_operation() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let intent = send_intent(&doc);

    // Created but never verified.
    let snap = SafetySnapshot::pending(
        DeviceId::from_string("sim-device"),
        DeviceOperation::UploadDocument,
        vec![AffectedDocument {
            document_id: doc.clone(),
            checksum_before: None,
        }],
        Some(3 * GB),
        Utc::now(),
    );

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    assert_eq!(
        auth.denial(),
        Some(&DenialReason::SnapshotMissingOrUnverified)
    );
}

#[test]
fn no_rollback_plan_means_no_operation() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let mut pre = Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap);
    pre.rollback_plan_ready = None;

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });

    assert_eq!(auth.denial(), Some(&DenialReason::RollbackPlanUnavailable));
}

// ── The happy path, so the suite is not only about refusals ────────────────

#[test]
fn a_fully_satisfied_request_is_granted_exactly_once() {
    let manager = manager_with_transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = send_intent(&doc);

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    let grant = match auth {
        Authorization::Granted(g) => g,
        Authorization::Denied(r) => panic!("expected a grant, got {r:?}"),
    };

    assert!(grant.covers(DeviceOperation::UploadDocument, &dev.id, Some(&doc)));
    assert!(grant.is_valid_at(Utc::now()));
    assert!(
        !grant.is_valid_at(Utc::now() + Duration::seconds(600)),
        "a grant must expire"
    );
}

// ── Phase-gated stubs — visible gaps, not forgotten ones ───────────────────

#[test]
#[ignore = "Phase 4: requires the PDF engine. Asserts the Zotero source file is byte-identical before and after a failed Send."]
fn s11_a_failed_send_leaves_the_zotero_source_untouched() {}

#[test]
#[ignore = "Phase 4: requires the annotation extractor. Asserts a merge failure discards the derived copy and leaves the original intact."]
fn s6_an_annotation_merge_failure_leaves_the_original_untouched() {}
