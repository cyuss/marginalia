//! # Characterization tests
//!
//! These pin the behaviour of Phase 0 *as it is today*, before any of it moves
//! during the standalone-reMarkable extraction.
//!
//! They are not aspirational. A characterization test does not say "this is
//! right"; it says "this is what the code does, and if an extraction changes
//! it, that must be a decision rather than an accident". Several of them would
//! be poor unit tests — they assert exact orderings and exact table contents —
//! and that is the point.
//!
//! Deleting or loosening one of these is legitimate, but only alongside a note
//! in `docs/migration/phase-0-to-standalone-rm2.md` explaining what changed and
//! why.

use chrono::{Duration, Utc};

use marginalia_core::device::{
    Capability, CapabilityStatus, ConnectionKind, Device, DeviceKind, FirmwareVersion, StorageInfo,
};
use marginalia_core::document::{DocumentEvent, DocumentState};
use marginalia_core::ids::{DeviceId, DocumentId};
use marginalia_core::intent::{ExplicitUserIntent, UserAction};
use marginalia_core::sync::{JobTrigger, SyncJobKind};

use marginalia_safety::classification::{DeviceOperation, SafetyClass};
use marginalia_safety::manager::{
    DenialReason, OperationRequest, Preconditions, DEFAULT_STORAGE_RESERVE_BYTES,
};
use marginalia_safety::snapshot::{AffectedDocument, SafetySnapshot};
use marginalia_safety::{FeatureFlag, FeatureFlagManager, SafetyManager};

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;

// ── fixtures ────────────────────────────────────────────────────────────────

const ALL_STATES: &[DocumentState] = &[
    DocumentState::MetadataOnly,
    DocumentState::AttachmentAvailable,
    DocumentState::TransferPending,
    DocumentState::OnRemarkable,
    DocumentState::Annotated,
    DocumentState::ChangesPending,
    DocumentState::Synced,
    DocumentState::Conflict,
    DocumentState::TransferFailed,
    DocumentState::RemovedFromDevice,
];

const ALL_EVENTS: &[DocumentEvent] = &[
    DocumentEvent::AttachmentResolved,
    DocumentEvent::AttachmentLost,
    DocumentEvent::UserRequestedSend,
    DocumentEvent::TransferVerified,
    DocumentEvent::TransferAborted,
    DocumentEvent::AnnotationsDetected,
    DocumentEvent::IngestCompleted,
    DocumentEvent::UserExportedToZotero,
    DocumentEvent::DivergenceDetected,
    DocumentEvent::ConflictResolvedSynced,
    DocumentEvent::ConflictResolvedPending,
    DocumentEvent::UserRemovedFromDevice,
];

const ALL_OPERATIONS: &[DeviceOperation] = &[
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

fn device(firmware: Option<&str>, free_bytes: u64) -> Device {
    Device {
        id: DeviceId::from_string("char-device"),
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

fn verified_snapshot(doc: &DocumentId) -> SafetySnapshot {
    let mut s = SafetySnapshot::pending(
        DeviceId::from_string("char-device"),
        DeviceOperation::UploadDocument,
        vec![AffectedDocument {
            document_id: doc.clone(),
            checksum_before: None,
        }],
        Some(3 * GB),
        Utc::now(),
    );
    s.verify();
    s
}

fn transfers_enabled() -> SafetyManager {
    let mut flags = FeatureFlagManager::new();
    flags.set(FeatureFlag::SafeDocumentTransfer, true);
    SafetyManager::new(flags)
}

// ── 1. The document state machine, in full ──────────────────────────────────

/// Snapshot of every legal edge. 20 edges out of the 120 possible
/// (state, event) pairs — the other 100 are errors, not silent no-ops.
///
/// If an extraction changes this table, this test lists exactly which edges
/// appeared or vanished.
#[test]
fn the_transition_table_is_exactly_these_20_edges() {
    let mut legal: Vec<String> = Vec::new();

    for &state in ALL_STATES {
        for &event in ALL_EVENTS {
            if let Ok(next) = state.apply(event) {
                legal.push(format!("{state:?} + {event:?} -> {next:?}"));
            }
        }
    }
    legal.sort();

    let expected: Vec<&str> = vec![
        "Annotated + IngestCompleted -> ChangesPending",
        "Annotated + UserRemovedFromDevice -> RemovedFromDevice",
        "AttachmentAvailable + AttachmentLost -> MetadataOnly",
        "AttachmentAvailable + UserRequestedSend -> TransferPending",
        "ChangesPending + DivergenceDetected -> Conflict",
        "ChangesPending + UserExportedToZotero -> Synced",
        "ChangesPending + UserRemovedFromDevice -> RemovedFromDevice",
        "Conflict + ConflictResolvedPending -> ChangesPending",
        "Conflict + ConflictResolvedSynced -> Synced",
        "MetadataOnly + AttachmentResolved -> AttachmentAvailable",
        "OnRemarkable + AnnotationsDetected -> Annotated",
        "OnRemarkable + DivergenceDetected -> Conflict",
        "OnRemarkable + UserRemovedFromDevice -> RemovedFromDevice",
        "RemovedFromDevice + UserRequestedSend -> TransferPending",
        "Synced + AnnotationsDetected -> Annotated",
        "Synced + DivergenceDetected -> Conflict",
        "Synced + UserRemovedFromDevice -> RemovedFromDevice",
        "TransferFailed + UserRequestedSend -> TransferPending",
        "TransferPending + TransferAborted -> TransferFailed",
        "TransferPending + TransferVerified -> OnRemarkable",
    ];

    assert_eq!(
        legal, expected,
        "\nthe transition table changed.\n\
         If that is intentional, update this test AND \
         docs/architecture/DOCUMENT_STATE_MACHINE.md in the same commit."
    );
}

/// The single most important structural fact in the product, pinned so an
/// extraction cannot quietly add a second route.
#[test]
fn exactly_one_state_event_pair_reaches_transfer_pending_without_a_user() {
    let routes: Vec<_> = ALL_STATES
        .iter()
        .flat_map(|&s| ALL_EVENTS.iter().map(move |&e| (s, e)))
        .filter(|(s, e)| matches!(s.apply(*e), Ok(DocumentState::TransferPending)))
        .collect();

    assert_eq!(routes.len(), 3, "expected exactly three Send routes");
    for (state, event) in routes {
        assert_eq!(
            event,
            DocumentEvent::UserRequestedSend,
            "{state:?} reaches TransferPending via {event:?}, which is not an \
             explicit user action"
        );
    }
}

// ── 2. Denial precedence in SafetyManager ───────────────────────────────────

/// The order in which `authorize` checks things is observable behaviour: it
/// determines which message the user sees when several things are wrong at
/// once. Extraction must not reorder it silently.
///
/// Each case below fails *several* gates simultaneously and asserts which one
/// reports first.
#[test]
fn denial_precedence_is_stable() {
    let doc = DocumentId::new();
    let snap = verified_snapshot(&doc);
    let intent = ExplicitUserIntent::record(UserAction::SendToRemarkable, doc.clone(), Utc::now());

    // A RED operation with everything else also wrong: classification wins.
    let manager = transfers_enabled();
    let dev = device(None, 0); // unknown firmware AND no storage
    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::PatchXochitl,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: Preconditions::default(),
        now: Utc::now(),
    });
    assert!(
        matches!(
            auth.denial(),
            Some(DenialReason::ProhibitedOperation { .. })
        ),
        "a prohibited operation must be refused before anything else is even \
         considered — got {:?}",
        auth.denial()
    );

    // Flag off AND unknown firmware: the flag is checked first.
    let manager = SafetyManager::new(FeatureFlagManager::new());
    let dev = device(None, 3 * GB);
    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), MB, &snap),
        now: Utc::now(),
    });
    assert_eq!(
        auth.denial(),
        Some(&DenialReason::FeatureFlagDisabled {
            flag: FeatureFlag::SafeDocumentTransfer
        }),
        "feature flag is checked before firmware"
    );

    // Unknown firmware AND missing intent AND no storage: firmware wins.
    let manager = transfers_enabled();
    let dev = device(None, 3 * GB);
    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: None,
        preconditions: Preconditions::default(),
        now: Utc::now(),
    });
    assert_eq!(
        auth.denial(),
        Some(&DenialReason::FirmwareUnknown),
        "firmware is checked before intent and preconditions"
    );

    // Known firmware, capability unresolved, missing intent: capability wins.
    let dev = device(Some("3.11.2"), 3 * GB);
    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: None,
        preconditions: Preconditions::default(),
        now: Utc::now(),
    });
    assert_eq!(
        auth.denial(),
        Some(&DenialReason::CapabilityUnresolved),
        "capability is checked before intent"
    );

    // Everything fine except: already completed AND storage too small.
    // Idempotency wins, because repeating finished work is not an error.
    let mut pre = Preconditions::ready_for_upload(
        StorageInfo {
            total_bytes: 6 * GB,
            free_bytes: 10 * MB,
        },
        5 * GB,
        &snap,
    );
    pre.already_completed = true;
    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: pre,
        now: Utc::now(),
    });
    assert_eq!(
        auth.denial(),
        Some(&DenialReason::AlreadyCompleted),
        "idempotency is checked before storage"
    );
}

/// Every denial has a message a human can act on. Pinned so that a refactor
/// cannot leave a variant with an empty or placeholder string.
#[test]
fn every_denial_reason_has_a_plain_language_message() {
    let reasons = [
        DenialReason::ProhibitedOperation {
            operation: DeviceOperation::PatchXochitl,
        },
        DenialReason::FeatureFlagDisabled {
            flag: FeatureFlag::SafeDocumentTransfer,
        },
        DenialReason::SafeModeForbidsClass {
            class: SafetyClass::Orange,
        },
        DenialReason::DeviceNotIdentified,
        DenialReason::FirmwareUnknown,
        DenialReason::CapabilityNotSupported {
            status: CapabilityStatus::Unknown,
        },
        DenialReason::CapabilityUnresolved,
        DenialReason::MissingUserIntent,
        DenialReason::IntentDoesNotMatchRequest,
        DenialReason::IntentStale,
        DenialReason::PdfNotValidated,
        DenialReason::SourceChecksumMismatch,
        DenialReason::StorageUnknown,
        DenialReason::InsufficientStorage {
            required: 1,
            available: 0,
            reserve: 0,
        },
        DenialReason::PayloadSizeUnknown,
        DenialReason::TargetNotOwnedByMarginalia,
        DenialReason::OwnershipUnverified,
        DenialReason::SnapshotMissingOrUnverified,
        DenialReason::RollbackPlanUnavailable,
        DenialReason::AlreadyCompleted,
    ];

    for reason in &reasons {
        let msg = reason.user_message();
        assert!(msg.len() > 20, "{reason:?} has a stub message: {msg:?}");
        assert!(
            !msg.contains("TODO") && !msg.contains("error code"),
            "{reason:?} message is not user-facing: {msg:?}"
        );
    }
}

// ── 3. The classification table ─────────────────────────────────────────────

#[test]
fn the_classification_table_is_pinned() {
    let mut rows: Vec<String> = ALL_OPERATIONS
        .iter()
        .map(|op| format!("{op:?}={:?}", op.classify()))
        .collect();
    rows.sort();

    let green = rows.iter().filter(|r| r.ends_with("Green")).count();
    let yellow = rows.iter().filter(|r| r.ends_with("Yellow")).count();
    let orange = rows.iter().filter(|r| r.ends_with("Orange")).count();
    let red = rows.iter().filter(|r| r.ends_with("Red")).count();

    assert_eq!(
        (green, yellow, orange, red),
        (9, 4, 2, 8),
        "the classification counts changed: {green} green, {yellow} yellow, \
         {orange} orange, {red} red.\n\
         A fifth YELLOW operation means a fifth allowed device write, which \
         requires changing docs/safety/DEVICE_WRITE_POLICY.md first."
    );
}

#[test]
fn feature_flag_defaults_are_all_off() {
    let flags = FeatureFlagManager::new();
    for flag in FeatureFlag::ALL {
        assert!(
            !flags.is_enabled(flag),
            "{flag:?} defaults to ON. Phase 0 ships with everything off; each \
             phase turns on its own flag after its safety tests pass."
        );
    }
    assert_eq!(FeatureFlag::ALL.len(), 8, "the flag set changed");
}

#[test]
fn job_trigger_rules_are_pinned() {
    let rules: Vec<(SyncJobKind, bool)> = [
        SyncJobKind::ZoteroMetadata,
        SyncJobKind::DeviceScan,
        SyncJobKind::AnnotationIngest,
        SyncJobKind::Transfer,
        SyncJobKind::Removal,
        SyncJobKind::ZoteroExport,
        SyncJobKind::TagBridge,
    ]
    .into_iter()
    .map(|k| (k, k.may_be_triggered_by(JobTrigger::Schedule)))
    .collect();

    assert_eq!(
        rules,
        vec![
            (SyncJobKind::ZoteroMetadata, true),
            (SyncJobKind::DeviceScan, true),
            (SyncJobKind::AnnotationIngest, true),
            (SyncJobKind::Transfer, false),
            (SyncJobKind::Removal, false),
            (SyncJobKind::ZoteroExport, false),
            (SyncJobKind::TagBridge, true),
        ],
        "which jobs may run unattended changed"
    );
}

// ── 4. Storage arithmetic ───────────────────────────────────────────────────

/// The reserve is the rule that stops a device being driven to full. Pinned
/// with concrete numbers so a refactor of the arithmetic is visible.
#[test]
fn storage_reserve_arithmetic_is_pinned() {
    assert_eq!(DEFAULT_STORAGE_RESERVE_BYTES, 500 * MB);

    let s = StorageInfo {
        total_bytes: 6 * GB,
        free_bytes: 3 * GB,
    };
    assert!(s.can_accept(84 * MB, DEFAULT_STORAGE_RESERVE_BYTES));

    // Exactly at the reserve boundary: permitted.
    let s = StorageInfo {
        total_bytes: 6 * GB,
        free_bytes: 600 * MB,
    };
    assert!(s.can_accept(100 * MB, DEFAULT_STORAGE_RESERVE_BYTES));

    // One byte over: refused.
    assert!(!s.can_accept(100 * MB + 1, DEFAULT_STORAGE_RESERVE_BYTES));
}

// ── 5. Capability resolution ────────────────────────────────────────────────

#[test]
fn capability_status_permissions_are_pinned() {
    let table: Vec<(CapabilityStatus, bool, bool)> = [
        CapabilityStatus::Supported,
        CapabilityStatus::ReadOnly,
        CapabilityStatus::Experimental,
        CapabilityStatus::Unsupported,
        CapabilityStatus::Unknown,
    ]
    .into_iter()
    .map(|s| (s, s.permits_read(), s.permits_write()))
    .collect();

    assert_eq!(
        table,
        vec![
            (CapabilityStatus::Supported, true, true),
            (CapabilityStatus::ReadOnly, true, false),
            (CapabilityStatus::Experimental, true, false),
            (CapabilityStatus::Unsupported, false, false),
            // Unknown reads but never writes: degrade to read-only, not to
            // nothing.
            (CapabilityStatus::Unknown, true, false),
        ]
    );
    assert_eq!(CapabilityStatus::default(), CapabilityStatus::Unknown);
}

#[test]
fn the_shipped_matrix_grants_nothing() {
    let resolver = marginalia_remarkable::CapabilityResolver::bundled();
    let dev = device(Some("3.11.2"), 3 * GB);

    for capability in [
        Capability::DeviceInfoRead,
        Capability::MetadataRead,
        Capability::StorageRead,
        Capability::AnnotationRead,
        Capability::NativeTagsRead,
        Capability::SafeDocumentTransfer,
        Capability::DocumentRemoval,
        Capability::NativeTagsWrite,
        Capability::PdfAnnotationExport,
    ] {
        assert!(
            !resolver.resolve(&dev, capability).permits_write(),
            "{capability:?} permits a write in the shipped matrix. Nothing has \
             been validated on hardware yet."
        );
    }
}

// ── 6. Storage schema surface ───────────────────────────────────────────────

/// The tables Phase 0 created. The standalone reMarkable runtime will reuse
/// this schema, so a rename during extraction is a data-migration decision, not
/// a tidy-up.
#[test]
fn the_schema_surface_is_pinned() {
    let conn = marginalia_database::open_in_memory().expect("open db");
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(
        tables,
        vec![
            "device",
            "device_capability",
            "document",
            "document_mapping",
            "highlight",
            "reading_state",
            "safety_log",
            "safety_snapshot",
            "schema_migrations",
            "side_note",
            "sticky_note",
            "sync_job",
            "sync_operation",
            "tag",
            "tag_mapping",
            "zotero_attachment",
            "zotero_collection",
            "zotero_item",
            "zotero_item_collection",
        ],
        "the schema changed; a rename is a migration, not a rename"
    );
}

#[test]
fn the_migration_version_is_pinned() {
    let conn = marginalia_database::open_in_memory().expect("open db");
    assert_eq!(
        marginalia_database::migrations::current_version(&conn).unwrap(),
        1,
        "schema version changed; record it in the migration report"
    );
}

// ── 7. Intent and grant lifetimes ───────────────────────────────────────────

#[test]
fn intent_and_grant_lifetimes_are_pinned() {
    let doc = DocumentId::new();

    // Intent: 300 seconds.
    let fresh = ExplicitUserIntent::record(UserAction::SendToRemarkable, doc.clone(), Utc::now());
    assert!(fresh.is_fresh(Utc::now() + Duration::seconds(299), 300));
    assert!(!fresh.is_fresh(Utc::now() + Duration::seconds(301), 300));

    // Grant: 120 seconds, asserted through the manager since grants cannot be
    // constructed directly outside the safety crate — which is itself the
    // property being preserved.
    let manager = transfers_enabled();
    let dev = device(Some("3.11.2"), 3 * GB);
    let snap = verified_snapshot(&doc);
    let intent = ExplicitUserIntent::record(UserAction::SendToRemarkable, doc.clone(), Utc::now());

    let auth = manager.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &dev,
        document_id: Some(doc.clone()),
        intent: Some(&intent),
        preconditions: Preconditions::ready_for_upload(dev.storage.unwrap(), 84 * MB, &snap),
        now: Utc::now(),
    });

    let grant = match auth {
        marginalia_safety::Authorization::Granted(g) => g,
        marginalia_safety::Authorization::Denied(r) => panic!("expected grant, got {r:?}"),
    };
    assert!(grant.is_valid_at(Utc::now() + Duration::seconds(119)));
    assert!(!grant.is_valid_at(Utc::now() + Duration::seconds(121)));
}
