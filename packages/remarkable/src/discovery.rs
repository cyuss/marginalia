//! Read-only device discovery.
//!
//! Phase 4. Everything here is GREEN: it reads, and there is no method that can
//! change anything. The point is to be able to describe a device accurately —
//! model, firmware, storage, and which of its documents are ours — before any
//! phase is allowed to write to one.
//!
//! # The rule this module protects
//!
//! A device document whose UUID is not in our mapping table **belongs to the
//! user**. Reconciliation classifies; it never acts. A document we do not
//! recognise is not a stray to clean up, it is someone's notebook.

use marginalia_core::device::{
    Capability, CapabilityStatus, Device, DeviceKind, StorageInfo, StorageVerdict,
};
use marginalia_core::ids::RemarkableDocumentId;
use serde::Serialize;
use std::collections::BTreeSet;

use crate::capability::CapabilityResolver;
use crate::provider::RemoteDocument;

/// What we can say about a device after looking, without touching it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceReport {
    pub model: DeviceKind,
    /// The firmware string exactly as the device gave it, even when unparsed —
    /// a bug report needs the raw value, not our interpretation of it.
    pub firmware_raw: Option<String>,
    pub firmware_recognised: bool,
    pub storage: Option<StorageInfo>,
    pub documents_total: usize,
    pub documents_ours: usize,
    /// Documents the user put here. We count them and never touch them.
    pub documents_theirs: usize,
    pub capabilities: Vec<CapabilityReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CapabilityReport {
    pub capability: Capability,
    pub status: CapabilityStatus,
    /// Whether this permits a write *right now*, on this device.
    pub permits_write: bool,
}

impl DeviceReport {
    /// Whether anything at all may be written to this device at the moment.
    ///
    /// A single `false` here is the honest summary of "read-only", and it is
    /// what the status screen shows.
    pub fn any_write_permitted(&self) -> bool {
        self.capabilities.iter().any(|c| c.permits_write)
    }

    /// One line for a human.
    pub fn summary(&self) -> String {
        let firmware = self.firmware_raw.as_deref().unwrap_or("unknown firmware");
        let mode = if self.any_write_permitted() {
            "writes permitted"
        } else {
            "read-only"
        };
        format!(
            "{:?} · {firmware} · {} documents ({} ours) · {mode}",
            self.model, self.documents_total, self.documents_ours
        )
    }
}

/// How a device document relates to us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Ownership {
    /// Marginalia transferred it. It may be removed or replaced, with a grant.
    Ours,
    /// The user's own. Read-only, forever, no exceptions.
    Theirs,
}

/// One document, classified.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedDocument {
    pub uuid: RemarkableDocumentId,
    pub visible_name: String,
    pub size_bytes: Option<u64>,
    pub ownership: Ownership,
}

impl ClassifiedDocument {
    pub fn is_ours(&self) -> bool {
        self.ownership == Ownership::Ours
    }
}

/// Classify what is on a device against the mappings we hold.
///
/// Takes the known UUIDs rather than a database handle: the rule is a pure
/// function of two sets, and keeping it that way means it can be tested
/// exhaustively and cannot accidentally acquire the ability to write.
pub fn classify_documents(
    on_device: &[RemoteDocument],
    ours: &BTreeSet<String>,
) -> Vec<ClassifiedDocument> {
    on_device
        .iter()
        .map(|doc| ClassifiedDocument {
            uuid: doc.uuid.clone(),
            visible_name: doc.visible_name.clone(),
            size_bytes: doc.size_bytes,
            // Default to Theirs. An unrecognised document is the user's, and a
            // classification bug should err towards leaving things alone.
            ownership: if ours.contains(doc.uuid.as_str()) {
                Ownership::Ours
            } else {
                Ownership::Theirs
            },
        })
        .collect()
}

/// Documents we hold a mapping for that are **not** on the device any more.
///
/// The user deleted them from the device themselves, which they are entitled to
/// do. This reports the divergence so the local state can be corrected; it is
/// never a reason to put the document back.
pub fn missing_from_device(on_device: &[RemoteDocument], ours: &BTreeSet<String>) -> Vec<String> {
    let present: BTreeSet<&str> = on_device.iter().map(|d| d.uuid.as_str()).collect();
    ours.iter()
        .filter(|uuid| !present.contains(uuid.as_str()))
        .cloned()
        .collect()
}

/// The largest documents on the device, for the storage view.
///
/// Reporting only. Marginalia never deletes anything to make room — it shows
/// what is big and lets the user decide.
pub fn largest_documents(
    documents: &[ClassifiedDocument],
    limit: usize,
) -> Vec<&ClassifiedDocument> {
    let mut sorted: Vec<&ClassifiedDocument> = documents.iter().collect();
    sorted.sort_by_key(|d| std::cmp::Reverse(d.size_bytes.unwrap_or(0)));
    sorted.truncate(limit);
    sorted
}

/// Build the report. Reads only.
pub fn describe(
    device: &Device,
    resolver: &CapabilityResolver,
    documents: &[ClassifiedDocument],
) -> DeviceReport {
    const REPORTED: &[Capability] = &[
        Capability::DeviceInfoRead,
        Capability::StorageRead,
        Capability::MetadataRead,
        Capability::AnnotationRead,
        Capability::NativeTagsRead,
        Capability::SafeDocumentTransfer,
        Capability::DocumentRemoval,
        Capability::NativeTagsWrite,
        Capability::PdfAnnotationExport,
    ];

    let capabilities = REPORTED
        .iter()
        .map(|&capability| {
            let status = resolver.resolve(device, capability);
            CapabilityReport {
                capability,
                status,
                permits_write: capability.is_write() && status.permits_write(),
            }
        })
        .collect();

    let ours = documents.iter().filter(|d| d.is_ours()).count();

    DeviceReport {
        model: device.kind,
        firmware_raw: device.firmware.as_ref().map(|f| f.raw.clone()),
        firmware_recognised: device.firmware.is_some(),
        storage: device.storage,
        documents_total: documents.len(),
        documents_ours: ours,
        documents_theirs: documents.len() - ours,
        capabilities,
    }
}

/// What the storage view says, given a hypothetical incoming document.
pub fn storage_advice(
    storage: Option<StorageInfo>,
    incoming_bytes: u64,
    reserve_bytes: u64,
) -> StorageVerdict {
    match storage {
        Some(s) => s.verdict(incoming_bytes, reserve_bytes),
        // Not knowing is not permission. An unreadable storage figure is
        // treated exactly like a full device.
        None => StorageVerdict::Critical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalia_core::device::{ConnectionKind, FirmwareVersion};
    use marginalia_core::ids::DeviceId;

    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    const RESERVE: u64 = 500 * MB;

    fn device(firmware: Option<&str>, free: u64) -> Device {
        Device {
            id: DeviceId::from_string("d1"),
            kind: DeviceKind::Rm2,
            serial_hash: Some("hash".into()),
            display_name: "reMarkable 2".into(),
            firmware: firmware.and_then(|f| FirmwareVersion::parse(f).ok()),
            connection: ConnectionKind::Usb,
            last_seen_at: None,
            storage: Some(StorageInfo {
                total_bytes: 6 * GB,
                free_bytes: free,
            }),
            safe_mode: true,
        }
    }

    fn remote(uuid: &str, name: &str, size: u64) -> RemoteDocument {
        RemoteDocument {
            uuid: RemarkableDocumentId::from_string(uuid),
            visible_name: name.into(),
            parent: None,
            size_bytes: Some(size),
            has_annotations: false,
            native_tags: vec![],
        }
    }

    fn ours(uuids: &[&str]) -> BTreeSet<String> {
        uuids.iter().map(|u| u.to_string()).collect()
    }

    // ── ownership ───────────────────────────────────────────────────────

    #[test]
    fn a_document_we_transferred_is_ours() {
        let docs = classify_documents(&[remote("a", "Attention", 12 * MB)], &ours(&["a"]));
        assert_eq!(docs[0].ownership, Ownership::Ours);
    }

    /// The rule the whole module exists to protect.
    #[test]
    fn an_unrecognised_document_is_theirs() {
        let docs = classify_documents(
            &[remote("unknown-uuid", "Research journal", 4 * MB)],
            &ours(&["a", "b"]),
        );
        assert_eq!(
            docs[0].ownership,
            Ownership::Theirs,
            "a document we do not recognise is not a stray to clean up — it is \
             someone's notebook"
        );
    }

    #[test]
    fn an_empty_mapping_table_makes_everything_theirs() {
        // A fresh install, or a lost database. Neither is licence to touch
        // anything.
        let docs = classify_documents(
            &[remote("a", "One", MB), remote("b", "Two", MB)],
            &BTreeSet::new(),
        );
        assert!(docs.iter().all(|d| d.ownership == Ownership::Theirs));
        assert_eq!(docs.iter().filter(|d| d.is_ours()).count(), 0);
    }

    #[test]
    fn ours_and_theirs_are_counted_separately() {
        let docs = classify_documents(
            &[
                remote("a", "Ours", MB),
                remote("x", "Theirs", MB),
                remote("y", "Also theirs", MB),
            ],
            &ours(&["a"]),
        );
        let report = describe(
            &device(Some("3.11.2"), 3 * GB),
            &CapabilityResolver::bundled(),
            &docs,
        );
        assert_eq!(report.documents_total, 3);
        assert_eq!(report.documents_ours, 1);
        assert_eq!(report.documents_theirs, 2);
    }

    // ── divergence ──────────────────────────────────────────────────────

    #[test]
    fn a_document_the_user_deleted_is_reported_not_restored() {
        // They are entitled to delete it. This function's only job is to say so.
        let missing = missing_from_device(&[remote("a", "Still here", MB)], &ours(&["a", "gone"]));
        assert_eq!(missing, vec!["gone".to_string()]);
    }

    #[test]
    fn nothing_is_missing_when_everything_is_present() {
        assert!(missing_from_device(&[remote("a", "A", MB)], &ours(&["a"])).is_empty());
    }

    // ── the report ──────────────────────────────────────────────────────

    #[test]
    fn a_shipped_matrix_device_is_read_only() {
        let report = describe(
            &device(Some("3.11.2"), 3 * GB),
            &CapabilityResolver::bundled(),
            &[],
        );
        assert!(
            !report.any_write_permitted(),
            "nothing has been validated on hardware, so nothing may be written"
        );
        assert!(report.summary().contains("read-only"));
    }

    #[test]
    fn unknown_firmware_is_reported_raw_and_unrecognised() {
        // A bug report needs the value the device actually gave, not our
        // interpretation of it.
        let mut d = device(None, 3 * GB);
        d.firmware = None;
        let report = describe(&d, &CapabilityResolver::bundled(), &[]);

        assert!(!report.firmware_recognised);
        assert_eq!(report.firmware_raw, None);
        assert!(!report.any_write_permitted());
    }

    #[test]
    fn a_recognised_firmware_keeps_its_raw_string() {
        let report = describe(
            &device(Some("3.11.2"), 3 * GB),
            &CapabilityResolver::bundled(),
            &[],
        );
        assert_eq!(report.firmware_raw.as_deref(), Some("3.11.2"));
        assert!(report.firmware_recognised);
    }

    #[test]
    fn every_reported_capability_carries_its_write_verdict() {
        let report = describe(
            &device(Some("3.11.2"), 3 * GB),
            &CapabilityResolver::bundled(),
            &[],
        );
        for c in &report.capabilities {
            if !c.capability.is_write() {
                assert!(!c.permits_write, "{:?} is a read", c.capability);
            }
        }
        assert_eq!(report.capabilities.len(), 9);
    }

    // ── storage ─────────────────────────────────────────────────────────

    #[test]
    fn unreadable_storage_is_treated_as_full() {
        // Not knowing is not permission.
        assert_eq!(storage_advice(None, MB, RESERVE), StorageVerdict::Critical);
    }

    #[test]
    fn storage_advice_respects_the_reserve() {
        let s = Some(StorageInfo {
            total_bytes: 6 * GB,
            free_bytes: 600 * MB,
        });
        assert_eq!(
            storage_advice(s, 200 * MB, RESERVE),
            StorageVerdict::Critical
        );
        assert_eq!(storage_advice(s, 50 * MB, RESERVE), StorageVerdict::Low);
    }

    #[test]
    fn the_largest_documents_are_reported_for_the_user_to_decide() {
        // Reporting only. Marginalia never deletes anything to make room.
        let docs = classify_documents(
            &[
                remote("a", "Small", 4 * MB),
                remote("b", "Huge", 312 * MB),
                remote("c", "Medium", 84 * MB),
            ],
            &ours(&["a"]),
        );
        let largest = largest_documents(&docs, 2);

        assert_eq!(largest.len(), 2);
        assert_eq!(largest[0].visible_name, "Huge");
        assert_eq!(largest[1].visible_name, "Medium");
        assert_eq!(
            largest[0].ownership,
            Ownership::Theirs,
            "the biggest document is often the user's own, and it is still \
             only ever reported"
        );
    }

    #[test]
    fn a_document_with_unknown_size_sorts_last_rather_than_first() {
        let docs = vec![
            ClassifiedDocument {
                uuid: RemarkableDocumentId::from_string("a"),
                visible_name: "Unknown size".into(),
                size_bytes: None,
                ownership: Ownership::Theirs,
            },
            ClassifiedDocument {
                uuid: RemarkableDocumentId::from_string("b"),
                visible_name: "Known".into(),
                size_bytes: Some(MB),
                ownership: Ownership::Theirs,
            },
        ];
        assert_eq!(largest_documents(&docs, 2)[0].visible_name, "Known");
    }
}
