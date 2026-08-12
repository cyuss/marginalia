//! The reMarkable simulator.
//!
//! A physical device is a bad development environment: you cannot make it run
//! out of storage on demand, you cannot unplug it at byte 4,194,304 of a
//! transfer, and you certainly cannot afford to brick it to find out what
//! happens. The simulator makes all of that routine.
//!
//! It is deterministic (no clocks, no randomness), observable (every call is
//! journalled), and **assertive**: it panics the test if the code under test
//! does something the safety model forbids, such as writing without a valid
//! grant or touching a document Marginalia never transferred.
//!
//! See `tests/remarkable-simulator/SPECIFICATION.md`.

pub mod faults;
pub mod profiles;

use std::collections::BTreeMap;

use marginalia_core::device::{Device, StorageInfo};
use marginalia_core::ids::RemarkableDocumentId;
use marginalia_core::Checksum;
use marginalia_remarkable::provider::{
    DeviceProvider, DeviceProviderError, DeviceResult, RemoteDocument, ValidatedPdf,
};
use marginalia_safety::classification::DeviceOperation;
use marginalia_safety::WriteGrant;

pub use faults::{Fault, FaultScript};
pub use profiles::DeviceProfile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimDocument {
    pub uuid: RemarkableDocumentId,
    pub visible_name: String,
    pub size_bytes: u64,
    pub checksum: Checksum,
    /// Whether Marginalia put it here. Documents the *user* created have this
    /// set to false, and the simulator enforces that we never modify them.
    pub placed_by_marginalia: bool,
    pub tags: Vec<String>,
}

/// Everything the simulator was asked to do, for assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimEvent {
    Detect,
    ReadStorage,
    ListDocuments,
    ReadAnnotations(RemarkableDocumentId),
    ReadNativeTags(RemarkableDocumentId),
    ChecksumOf(RemarkableDocumentId),
    Upload { name: String, bytes: u64 },
    Remove(RemarkableDocumentId),
    WriteTags(RemarkableDocumentId),
    RollbackUpload(RemarkableDocumentId),
}

impl SimEvent {
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            SimEvent::Upload { .. }
                | SimEvent::Remove(_)
                | SimEvent::WriteTags(_)
                | SimEvent::RollbackUpload(_)
        )
    }
}

pub struct SimulatedDevice {
    profile: DeviceProfile,
    documents: BTreeMap<String, SimDocument>,
    script: FaultScript,
    journal: Vec<SimEvent>,
    call_counts: BTreeMap<&'static str, u32>,
    next_uuid: u32,
}

impl SimulatedDevice {
    pub fn new(profile: DeviceProfile) -> Self {
        let documents = profile
            .initial_documents
            .iter()
            .cloned()
            .map(|d| (d.uuid.to_string(), d))
            .collect();
        Self {
            profile,
            documents,
            script: FaultScript::default(),
            journal: Vec::new(),
            call_counts: BTreeMap::new(),
            next_uuid: 1,
        }
    }

    pub fn with_faults(mut self, script: FaultScript) -> Self {
        self.script = script;
        self
    }

    pub fn device(&self) -> Device {
        self.profile.device.clone()
    }

    pub fn journal(&self) -> &[SimEvent] {
        &self.journal
    }

    /// Number of documents currently on the simulated device.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    pub fn contains(&self, uuid: &RemarkableDocumentId) -> bool {
        self.documents.contains_key(uuid.as_str())
    }

    /// The assertion the whole simulator exists for: how many times did the
    /// code under test actually write to a device?
    pub fn write_count(&self) -> usize {
        self.journal.iter().filter(|e| e.is_write()).count()
    }

    fn record(&mut self, event: SimEvent) {
        self.journal.push(event);
    }

    fn next_fault(&mut self, call: &'static str) -> Option<Fault> {
        let n = self.call_counts.entry(call).or_insert(0);
        *n += 1;
        self.script.fault_for(call, *n)
    }

    /// Enforce the invariants the safety model claims to guarantee.
    ///
    /// A violation panics rather than returning an error: this is not a device
    /// misbehaving, it is *our* code doing something it promised never to do,
    /// and the test must fail loudly.
    fn assert_grant_is_valid(
        &self,
        grant: &WriteGrant,
        expected: DeviceOperation,
        uuid: Option<&RemarkableDocumentId>,
    ) {
        assert_eq!(
            grant.operation(),
            expected,
            "SAFETY VIOLATION: grant for {:?} used to perform {:?}",
            grant.operation(),
            expected
        );
        assert_eq!(
            grant.device_id(),
            &self.profile.device.id,
            "SAFETY VIOLATION: grant issued for a different device"
        );

        if let Some(uuid) = uuid {
            if let Some(doc) = self.documents.get(uuid.as_str()) {
                assert!(
                    doc.placed_by_marginalia,
                    "SAFETY VIOLATION: attempted to modify '{}', a document \
                     Marginalia did not transfer. Foreign documents are read-only.",
                    doc.visible_name
                );
            }
        }
    }
}

impl DeviceProvider for SimulatedDevice {
    fn detect(&self) -> DeviceResult<Device> {
        if !self.profile.connected {
            return Err(DeviceProviderError::NotConnected);
        }
        Ok(self.profile.device.clone())
    }

    fn read_storage(&self) -> DeviceResult<StorageInfo> {
        self.profile
            .device
            .storage
            .ok_or(DeviceProviderError::NotConnected)
    }

    fn list_documents(&self) -> DeviceResult<Vec<RemoteDocument>> {
        Ok(self
            .documents
            .values()
            .map(|d| RemoteDocument {
                uuid: d.uuid.clone(),
                visible_name: d.visible_name.clone(),
                parent: None,
                size_bytes: Some(d.size_bytes),
                has_annotations: false,
                native_tags: d.tags.clone(),
            })
            .collect())
    }

    fn read_native_tags(&self, uuid: &RemarkableDocumentId) -> DeviceResult<Vec<String>> {
        self.documents
            .get(uuid.as_str())
            .map(|d| d.tags.clone())
            .ok_or_else(|| DeviceProviderError::Refused("no such document".into()))
    }

    fn read_annotations(&self, uuid: &RemarkableDocumentId) -> DeviceResult<Vec<u8>> {
        if !self.contains(uuid) {
            return Err(DeviceProviderError::Refused("no such document".into()));
        }
        Ok(Vec::new())
    }

    fn checksum_of(&self, uuid: &RemarkableDocumentId) -> DeviceResult<Checksum> {
        self.documents
            .get(uuid.as_str())
            .map(|d| d.checksum.clone())
            .ok_or_else(|| DeviceProviderError::Refused("no such document".into()))
    }

    fn upload_document(
        &mut self,
        grant: &WriteGrant,
        pdf: &ValidatedPdf,
        visible_name: &str,
    ) -> DeviceResult<RemarkableDocumentId> {
        self.assert_grant_is_valid(grant, DeviceOperation::UploadDocument, None);
        self.record(SimEvent::Upload {
            name: visible_name.to_string(),
            bytes: pdf.size_bytes(),
        });

        match self.next_fault("upload_document") {
            Some(Fault::ConnectionLost) => return Err(DeviceProviderError::ConnectionLost),
            Some(Fault::PermissionDenied) => {
                return Err(DeviceProviderError::Refused("permission denied".into()))
            }
            Some(Fault::TruncatedWrite) | Some(Fault::ChecksumMismatch) => {
                // The document lands, but corrupted — the nastiest realistic
                // failure, because a naive implementation would call it success.
                let uuid = RemarkableDocumentId::from_string(format!("sim-{}", self.next_uuid));
                self.next_uuid += 1;
                self.documents.insert(
                    uuid.to_string(),
                    SimDocument {
                        uuid: uuid.clone(),
                        visible_name: visible_name.to_string(),
                        size_bytes: pdf.size_bytes(),
                        checksum: Checksum::of_bytes(b"corrupted-on-arrival"),
                        placed_by_marginalia: true,
                        tags: Vec::new(),
                    },
                );
                return Ok(uuid);
            }
            Some(Fault::ListingOmitsUploadedDoc) => {
                let uuid = RemarkableDocumentId::from_string(format!("sim-{}", self.next_uuid));
                self.next_uuid += 1;
                return Ok(uuid); // never actually stored
            }
            // A rollback fault is scripted against `rollback_upload`, not here.
            Some(Fault::RollbackFails) | None => {}
        }

        let uuid = RemarkableDocumentId::from_string(format!("sim-{}", self.next_uuid));
        self.next_uuid += 1;
        self.documents.insert(
            uuid.to_string(),
            SimDocument {
                uuid: uuid.clone(),
                visible_name: visible_name.to_string(),
                size_bytes: pdf.size_bytes(),
                checksum: pdf.checksum().clone(),
                placed_by_marginalia: true,
                tags: Vec::new(),
            },
        );
        Ok(uuid)
    }

    fn remove_document(
        &mut self,
        grant: &WriteGrant,
        uuid: &RemarkableDocumentId,
    ) -> DeviceResult<()> {
        self.assert_grant_is_valid(grant, DeviceOperation::RemoveOwnedDocument, Some(uuid));
        self.record(SimEvent::Remove(uuid.clone()));

        match self.documents.get(uuid.as_str()) {
            Some(doc) if !doc.placed_by_marginalia => {
                Err(DeviceProviderError::NotOwnedByMarginalia)
            }
            Some(_) => {
                self.documents.remove(uuid.as_str());
                Ok(())
            }
            None => Err(DeviceProviderError::Refused("no such document".into())),
        }
    }

    fn write_native_tags(
        &mut self,
        grant: &WriteGrant,
        uuid: &RemarkableDocumentId,
        tags: &[String],
    ) -> DeviceResult<()> {
        self.assert_grant_is_valid(grant, DeviceOperation::WriteNativeTags, Some(uuid));
        self.record(SimEvent::WriteTags(uuid.clone()));

        match self.documents.get_mut(uuid.as_str()) {
            Some(doc) if doc.placed_by_marginalia => {
                doc.tags = tags.to_vec();
                Ok(())
            }
            Some(_) => Err(DeviceProviderError::NotOwnedByMarginalia),
            None => Err(DeviceProviderError::Refused("no such document".into())),
        }
    }

    fn rollback_upload(
        &mut self,
        grant: &WriteGrant,
        uuid: &RemarkableDocumentId,
    ) -> DeviceResult<()> {
        self.record(SimEvent::RollbackUpload(uuid.clone()));
        let _ = grant;

        if let Some(Fault::RollbackFails) = self.next_fault("rollback_upload") {
            return Err(DeviceProviderError::Transport(
                "rollback could not be completed".into(),
            ));
        }

        self.documents.remove(uuid.as_str());
        Ok(())
    }
}
