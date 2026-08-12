//! Capability resolution.
//!
//! Resolution order (highest precedence first):
//!
//! ```text
//! 1. USER_OVERRIDE   may only DOWNGRADE, never upgrade
//! 2. PROBED          read-only probe result from this session
//! 3. MATRIX          the versioned table
//! 4. default         UNKNOWN
//! ```
//!
//! WHY a user override cannot upgrade: "I know what I'm doing, enable writes
//! on untested firmware" is exactly the switch that turns a careful design into
//! a support thread about a bricked device. There is no such switch.

use marginalia_core::device::{Capability, CapabilityStatus, Device, DeviceKind};
use std::collections::BTreeMap;

use crate::compatibility::CompatibilityMatrix;

#[derive(Debug, Default)]
pub struct CapabilityResolver {
    matrix: CompatibilityMatrix,
    probed: BTreeMap<Capability, CapabilityStatus>,
    overrides: BTreeMap<Capability, CapabilityStatus>,
}

/// Rank statuses by how much they permit, so a downgrade can be recognised.
fn permissiveness(status: CapabilityStatus) -> u8 {
    match status {
        CapabilityStatus::Supported => 4,
        CapabilityStatus::Experimental => 3,
        CapabilityStatus::ReadOnly => 2,
        CapabilityStatus::Unknown => 1,
        CapabilityStatus::Unsupported => 0,
    }
}

impl CapabilityResolver {
    pub fn new(matrix: CompatibilityMatrix) -> Self {
        Self {
            matrix,
            probed: BTreeMap::new(),
            overrides: BTreeMap::new(),
        }
    }

    pub fn bundled() -> Self {
        Self::new(CompatibilityMatrix::bundled())
    }

    /// Record a read-only probe result.
    pub fn record_probe(&mut self, capability: Capability, status: CapabilityStatus) {
        self.probed.insert(capability, status);
    }

    /// Apply a user override.
    ///
    /// Returns `false` and changes nothing if the override would permit more
    /// than the resolved status already does.
    pub fn set_user_override(
        &mut self,
        device: &Device,
        capability: Capability,
        status: CapabilityStatus,
    ) -> bool {
        let current = self.resolve(device, capability);
        if permissiveness(status) > permissiveness(current) {
            return false;
        }
        self.overrides.insert(capability, status);
        true
    }

    /// The resolved status for this device and capability.
    pub fn resolve(&self, device: &Device, capability: Capability) -> CapabilityStatus {
        // A capability that would modify the system is never anything but
        // unsupported, regardless of what any source claims.
        if capability == Capability::SystemModification {
            return CapabilityStatus::Unsupported;
        }

        // Unknown firmware short-circuits everything: we cannot resolve against
        // a matrix we cannot index.
        let firmware = match device.firmware.as_ref() {
            Some(f) => f,
            None => return CapabilityStatus::Unknown,
        };

        let base = self
            .probed
            .get(&capability)
            .copied()
            .unwrap_or_else(|| self.matrix.resolve(device.kind, firmware, capability));

        match self.overrides.get(&capability).copied() {
            // An override can only ever restrict.
            Some(o) if permissiveness(o) <= permissiveness(base) => o,
            _ => base,
        }
    }

    /// Whether a device of unknown model should be trusted at all.
    pub fn model_is_supported(kind: DeviceKind) -> bool {
        kind == DeviceKind::Rm2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalia_core::device::{ConnectionKind, FirmwareVersion};
    use marginalia_core::ids::DeviceId;

    fn device(firmware: Option<&str>) -> Device {
        Device {
            id: DeviceId::new(),
            kind: DeviceKind::Rm2,
            serial_hash: Some("hash".into()),
            display_name: "reMarkable 2".into(),
            firmware: firmware.map(|f| FirmwareVersion::parse(f).unwrap()),
            connection: ConnectionKind::Usb,
            last_seen_at: None,
            storage: None,
            safe_mode: true,
        }
    }

    #[test]
    fn unknown_firmware_resolves_to_unknown() {
        let r = CapabilityResolver::bundled();
        let d = device(None);
        assert_eq!(
            r.resolve(&d, Capability::MetadataRead),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn the_shipped_state_permits_no_writes_at_all() {
        let r = CapabilityResolver::bundled();
        let d = device(Some("3.11.2"));
        for capability in [
            Capability::SafeDocumentTransfer,
            Capability::DocumentRemoval,
            Capability::NativeTagsWrite,
            Capability::PdfAnnotationExport,
        ] {
            assert!(!r.resolve(&d, capability).permits_write(), "{capability:?}");
        }
    }

    #[test]
    fn a_probe_can_raise_a_capability_above_the_matrix() {
        // A probe is first-hand evidence from this session, so it outranks the
        // shipped table.
        let mut r = CapabilityResolver::bundled();
        let d = device(Some("3.11.2"));
        r.record_probe(Capability::MetadataRead, CapabilityStatus::Supported);
        assert_eq!(
            r.resolve(&d, Capability::MetadataRead),
            CapabilityStatus::Supported
        );
    }

    #[test]
    fn a_user_override_can_restrict() {
        let mut r = CapabilityResolver::bundled();
        let d = device(Some("3.11.2"));
        r.record_probe(Capability::MetadataRead, CapabilityStatus::Supported);

        assert!(r.set_user_override(&d, Capability::MetadataRead, CapabilityStatus::ReadOnly));
        assert_eq!(
            r.resolve(&d, Capability::MetadataRead),
            CapabilityStatus::ReadOnly
        );
    }

    #[test]
    fn a_user_override_can_never_expand() {
        let mut r = CapabilityResolver::bundled();
        let d = device(Some("3.11.2"));

        // The matrix says UNKNOWN for transfer. The user tries to force it on.
        let accepted = r.set_user_override(
            &d,
            Capability::SafeDocumentTransfer,
            CapabilityStatus::Supported,
        );
        assert!(!accepted, "there is no 'enable writes anyway' switch");
        assert!(!r
            .resolve(&d, Capability::SafeDocumentTransfer)
            .permits_write());
    }

    #[test]
    fn system_modification_is_unsupported_no_matter_what_anyone_says() {
        let mut r = CapabilityResolver::bundled();
        let d = device(Some("3.11.2"));
        r.record_probe(Capability::SystemModification, CapabilityStatus::Supported);
        assert_eq!(
            r.resolve(&d, Capability::SystemModification),
            CapabilityStatus::Unsupported,
            "not even a probe result can enable a system modification"
        );
    }
}
