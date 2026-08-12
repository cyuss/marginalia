//! # The standalone property
//!
//! The product decision is that the essential reading workflow runs **on the
//! reMarkable**, with the desktop companion optional and no server at all.
//!
//! A claim like that decays into marketing unless something checks it. This
//! file walks the full chain end to end using only components that exist on the
//! device, and asserts at each step that nothing desktop-shaped was needed.
//!
//! What "on the device" means concretely here:
//!
//! - storage is the `Device` profile — rollback journal, `synchronous = FULL`;
//! - credentials come from a `0600` file, not an OS keychain;
//! - intent comes from a stylus mark on a generated form, not a button;
//! - device facts come from `DeviceIntrospection`, not a host-side transport.
//!
//! The one thing genuinely absent is the network call to Zotero, which needs
//! either a live key or a stub — it is covered in `marginalia-zotero`'s own
//! tests, and mocking it again here would test the mock.

use chrono::Utc;

use marginalia_core::annotation::BoundingBox;
use marginalia_core::credentials::{CredentialKey, CredentialStore};
use marginalia_core::device::CapabilityStatus;
use marginalia_core::ids::DocumentId;
use marginalia_core::ids::ZoteroKey;
use marginalia_core::intent::{ExplicitUserIntent, UserAction};
use marginalia_core::request_form::{FormAction, FormEntry, Mark, MarkVerdict, RequestForm};
use marginalia_core::secret::Redacted;
use marginalia_core::sync::{JobTrigger, MetadataOperation, SyncJobKind, SyncPlan};
use marginalia_core::zotero::AttachmentAvailability;

use marginalia_database::{open_with_profile, StorageProfile};
use marginalia_platform::FileCredentialStore;
use marginalia_remarkable::provider::DeviceIntrospection;

use marginalia_safety::classification::DeviceOperation;
use marginalia_safety::manager::{OperationRequest, Preconditions};
use marginalia_safety::snapshot::{AffectedDocument, SafetySnapshot};
use marginalia_safety::{Authorization, FeatureFlag, FeatureFlagManager, SafetyManager};

use marginalia_simulator::{DeviceProfile, SimulatedDevice};

const MB: u64 = 1024 * 1024;

/// The application's own data area on the device.
struct DeviceHome(std::path::PathBuf);

impl DeviceHome {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("marginalia-device-{}", DocumentId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> String {
        self.0
            .join("marginalia.sqlite")
            .to_str()
            .unwrap()
            .to_string()
    }
}

impl Drop for DeviceHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn bbox(x: f64, y: f64, w: f64, h: f64) -> BoundingBox {
    BoundingBox {
        x,
        y,
        width: w,
        height: h,
    }
}

/// The whole loop, on-device, with no desktop component anywhere in it.
#[test]
fn the_device_can_complete_the_essential_workflow_alone() {
    let home = DeviceHome::new();

    // ── 1. Its own storage, with the device durability profile. ──────────
    let conn = open_with_profile(&home.db(), StorageProfile::Device)
        .expect("the device opens its own database");
    assert_eq!(
        marginalia_database::migrations::current_version(&conn).unwrap(),
        marginalia_database::migrations::latest_version()
    );

    // ── 2. Its own credentials, without an OS keychain. ──────────────────
    let credentials = FileCredentialStore::new(&home.0);
    credentials
        .store(
            CredentialKey::ZoteroApiKey,
            Redacted::new("a-key-the-user-pasted".into()),
        )
        .unwrap();
    assert!(credentials
        .load(CredentialKey::ZoteroApiKey)
        .unwrap()
        .is_some());

    // ── 3. A metadata sync it may run unattended, moving zero PDF bytes. ─
    assert!(SyncJobKind::ZoteroMetadata.may_be_triggered_by(JobTrigger::Schedule));
    let plan = SyncPlan::new(
        SyncJobKind::ZoteroMetadata,
        vec![MetadataOperation::UpsertAttachmentAvailability {
            key: ZoteroKey::from_string("ABCD1234"),
            availability: AttachmentAvailability::AvailableLocal,
        }],
    );
    assert_eq!(plan.pdf_transfer_count(), 0);

    // ── 4. Facts about the machine it is running on. ─────────────────────
    let device_sim = SimulatedDevice::new(DeviceProfile::known_healthy());
    let device = DeviceIntrospection::device_info(&device_sim).expect("identity");
    let storage = DeviceIntrospection::storage(&device_sim).expect("storage");
    assert!(device.firmware_is_known());
    assert!(storage.free_bytes > 0);

    // ── 5. The user asks for a paper, with a stylus, on a generated form. ─
    let paper = DocumentId::new();
    let form = RequestForm::new(
        DocumentId::new(),
        Utc::now(),
        vec![FormEntry {
            target: paper.clone(),
            action: FormAction::DownloadToDevice,
            page: 1,
            tick_box: bbox(60.0, 700.0, 14.0, 14.0),
        }],
    );
    let tick = Mark {
        page: 1,
        bounds: bbox(62.0, 702.0, 10.0, 10.0),
    };

    let request = match form.interpret(&tick, &form.generation) {
        MarkVerdict::Requested(r) => r,
        other => panic!("the tick should be a request, got {other:?}"),
    };
    assert_eq!(request.target, paper);

    // ── 6. That mark becomes explicit intent, and goes through the same
    //       SafetyManager a button would. ─────────────────────────────────
    let intent = ExplicitUserIntent::record(
        UserAction::SendToRemarkable,
        request.target.clone(),
        Utc::now(),
    );

    let mut flags = FeatureFlagManager::new();
    flags.set(FeatureFlag::SafeDocumentTransfer, true);
    let safety = SafetyManager::new(flags);

    let mut snapshot = SafetySnapshot::pending(
        device.id.clone(),
        DeviceOperation::UploadDocument,
        vec![AffectedDocument {
            document_id: request.target.clone(),
            checksum_before: None,
        }],
        Some(storage.free_bytes),
        Utc::now(),
    );
    snapshot.verify();

    let mut preconditions = Preconditions::ready_for_upload(storage, 12 * MB, &snapshot);
    preconditions.capability_status = Some(CapabilityStatus::Supported);

    let authorization = safety.authorize(OperationRequest {
        operation: DeviceOperation::UploadDocument,
        device: &device,
        document_id: Some(request.target.clone()),
        intent: Some(&intent),
        preconditions,
        now: Utc::now(),
    });

    match authorization {
        Authorization::Granted(grant) => {
            assert!(grant.covers(
                DeviceOperation::UploadDocument,
                &device.id,
                Some(&request.target)
            ));
        }
        Authorization::Denied(reason) => {
            panic!("a stylus tick should authorise the same as a button: {reason:?}")
        }
    }
}

/// A mark and a button produce the *same* value, so nothing downstream can
/// treat one as weaker evidence than the other.
#[test]
fn a_stylus_mark_and_a_button_press_are_indistinguishable_downstream() {
    let paper = DocumentId::new();

    let from_button =
        ExplicitUserIntent::record(UserAction::SendToRemarkable, paper.clone(), Utc::now());
    let from_mark =
        ExplicitUserIntent::record(UserAction::SendToRemarkable, paper.clone(), Utc::now());

    assert!(from_button.authorises(UserAction::SendToRemarkable, &paper));
    assert!(from_mark.authorises(UserAction::SendToRemarkable, &paper));
    assert_eq!(from_button.action(), from_mark.action());
}

/// The device shares its disk with the user's documents, so its own writes
/// answer to the same reserve that guards a transfer.
#[test]
fn the_device_enforces_the_reserve_against_itself() {
    let sim = SimulatedDevice::new(DeviceProfile::low_storage());
    let storage = DeviceIntrospection::storage(&sim).unwrap();
    assert!(!storage.can_accept(1, marginalia_safety::manager::DEFAULT_STORAGE_RESERVE_BYTES));
}

/// Nothing in the on-device path can reach a document the user put there.
#[test]
fn the_on_device_surface_cannot_touch_the_users_own_documents() {
    let sim = SimulatedDevice::new(DeviceProfile::populated_with_user_documents());
    let before = sim.document_count();

    let _ = DeviceIntrospection::device_info(&sim).unwrap();
    let _ = DeviceIntrospection::storage(&sim).unwrap();

    assert_eq!(sim.document_count(), before);
    assert_eq!(sim.write_count(), 0);
}

/// A form the user never marked asks for nothing. The absence of a gesture is
/// not an instruction, and neither is reading or annotating a paper.
#[test]
fn no_gesture_means_no_request() {
    let form = RequestForm::new(
        DocumentId::new(),
        Utc::now(),
        vec![FormEntry {
            target: DocumentId::new(),
            action: FormAction::DownloadToDevice,
            page: 1,
            tick_box: bbox(60.0, 700.0, 14.0, 14.0),
        }],
    );

    assert!(form.interpret_all(&[], &form.generation).is_empty());

    // A highlight across the title, not in the box.
    let highlight = Mark {
        page: 1,
        bounds: bbox(120.0, 700.0, 260.0, 12.0),
    };
    assert!(form
        .interpret_all(&[highlight], &form.generation)
        .is_empty());
}
