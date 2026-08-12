//! Device profiles — the situations we need to be able to reproduce.

use marginalia_core::device::{ConnectionKind, Device, DeviceKind, FirmwareVersion, StorageInfo};
use marginalia_core::ids::{DeviceId, RemarkableDocumentId};
use marginalia_core::Checksum;

use crate::SimDocument;

pub const MB: u64 = 1024 * 1024;
pub const GB: u64 = 1024 * MB;

#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub device: Device,
    pub connected: bool,
    pub initial_documents: Vec<SimDocument>,
}

fn base_device(firmware: Option<&str>, free: u64) -> Device {
    Device {
        id: DeviceId::from_string("sim-device"),
        kind: DeviceKind::Rm2,
        serial_hash: Some("sim-serial-hash".into()),
        display_name: "reMarkable 2 (simulated)".into(),
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

impl DeviceProfile {
    /// Firmware we have validated, plenty of room. The happy path.
    pub fn known_healthy() -> Self {
        Self {
            device: base_device(Some("3.11.2"), 3 * GB),
            connected: true,
            initial_documents: Vec::new(),
        }
    }

    /// Firmware we have never tested. Everything must degrade to read-only.
    pub fn unknown_firmware() -> Self {
        Self {
            device: base_device(None, 3 * GB),
            connected: true,
            initial_documents: Vec::new(),
        }
    }

    /// Below the storage reserve.
    pub fn low_storage() -> Self {
        Self {
            device: base_device(Some("3.11.2"), 400 * MB),
            connected: true,
            initial_documents: Vec::new(),
        }
    }

    pub fn disconnected() -> Self {
        Self {
            device: base_device(Some("3.11.2"), 3 * GB),
            connected: false,
            initial_documents: Vec::new(),
        }
    }

    /// A device with the user's own documents on it, which Marginalia must
    /// never touch.
    pub fn populated_with_user_documents() -> Self {
        Self {
            device: base_device(Some("3.11.2"), 1200 * MB),
            connected: true,
            initial_documents: vec![
                SimDocument {
                    uuid: RemarkableDocumentId::from_string("user-notebook-1"),
                    visible_name: "Research journal".into(),
                    size_bytes: 12 * MB,
                    checksum: Checksum::of_bytes(b"user-notebook"),
                    placed_by_marginalia: false,
                    tags: vec!["Personal".into()],
                },
                SimDocument {
                    uuid: RemarkableDocumentId::from_string("user-pdf-1"),
                    visible_name: "A book the user added themselves".into(),
                    size_bytes: 312 * MB,
                    checksum: Checksum::of_bytes(b"user-book"),
                    placed_by_marginalia: false,
                    tags: vec![],
                },
            ],
        }
    }

    /// A document Marginalia transferred, so removal tests have a legal target.
    pub fn with_marginalia_document() -> Self {
        let mut profile = Self::known_healthy();
        profile.initial_documents.push(SimDocument {
            uuid: RemarkableDocumentId::from_string("marginalia-doc-1"),
            visible_name: "Attention Is All You Need".into(),
            size_bytes: 84 * MB,
            checksum: Checksum::of_bytes(b"attention-pdf"),
            placed_by_marginalia: true,
            tags: vec![],
        });
        profile
    }
}
