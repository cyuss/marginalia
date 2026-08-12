//! The firmware compatibility matrix.
//!
//! Data, not code. WHY the loader is strict about `tested_at`: a matrix entry
//! is a claim that someone verified a behaviour on real hardware. An entry
//! that says `SUPPORTED` with no test date is a wish. Loading it as `UNKNOWN`
//! means an optimistic edit to a data file — or a well-meaning pull request —
//! cannot hand out write permissions.

use marginalia_core::device::{Capability, CapabilityStatus, DeviceKind, FirmwareVersion};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixEntry {
    pub model: String,
    /// A range expression such as `"3.11"` or `"3.x"`.
    pub firmware: String,
    pub capability: String,
    pub status: String,
    /// Empty or absent means "never verified" — the entry loads as UNKNOWN.
    #[serde(default)]
    pub tested_at: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CompatibilityMatrix {
    #[serde(default, rename = "entry")]
    pub entries: Vec<MatrixEntry>,
}

impl CompatibilityMatrix {
    pub fn from_toml(source: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(source)
    }

    /// The matrix Marginalia ships with.
    pub fn bundled() -> Self {
        Self::from_toml(include_str!("../compatibility/matrix.toml"))
            .expect("the bundled matrix must parse; it is checked in CI")
    }

    /// Resolve one capability for one device.
    ///
    /// Returns [`CapabilityStatus::Unknown`] when there is no matching entry,
    /// when the entry was never tested, or when the status string is not one we
    /// recognise. All three are the same thing: we do not know, so we do not
    /// write.
    pub fn resolve(
        &self,
        model: DeviceKind,
        firmware: &FirmwareVersion,
        capability: Capability,
    ) -> CapabilityStatus {
        let model_key = model_key(model);
        let capability_key = capability_key(capability);

        for entry in &self.entries {
            if entry.model != model_key || entry.capability != capability_key {
                continue;
            }
            if !firmware.matches_range(&entry.firmware) {
                continue;
            }

            // An untested entry is a wish, not a verification.
            if entry.tested_at.trim().is_empty() {
                return CapabilityStatus::Unknown;
            }

            return parse_status(&entry.status);
        }

        CapabilityStatus::Unknown
    }
}

fn parse_status(s: &str) -> CapabilityStatus {
    match s {
        "SUPPORTED" => CapabilityStatus::Supported,
        "READ_ONLY" => CapabilityStatus::ReadOnly,
        "EXPERIMENTAL" => CapabilityStatus::Experimental,
        "UNSUPPORTED" => CapabilityStatus::Unsupported,
        // Including "UNKNOWN" and anything we fail to recognise.
        _ => CapabilityStatus::Unknown,
    }
}

fn model_key(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Rm2 => "RM2",
        DeviceKind::Rm1 => "RM1",
        DeviceKind::RmPaperPro => "RMPP",
        DeviceKind::Unknown => "UNKNOWN",
    }
}

fn capability_key(c: Capability) -> &'static str {
    match c {
        Capability::DeviceInfoRead => "DeviceInfoRead",
        Capability::MetadataRead => "MetadataRead",
        Capability::StorageRead => "StorageRead",
        Capability::AnnotationRead => "AnnotationRead",
        Capability::NativeTagsRead => "NativeTagsRead",
        Capability::SafeDocumentTransfer => "SafeDocumentTransfer",
        Capability::DocumentRemoval => "DocumentRemoval",
        Capability::NativeTagsWrite => "NativeTagsWrite",
        Capability::PdfAnnotationExport => "PdfAnnotationExport",
        Capability::CompanionApp => "CompanionApp",
        Capability::ExperimentalRmUi => "ExperimentalRmUi",
        Capability::SystemModification => "SystemModification",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fw(s: &str) -> FirmwareVersion {
        FirmwareVersion::parse(s).unwrap()
    }

    #[test]
    fn the_bundled_matrix_parses() {
        let m = CompatibilityMatrix::bundled();
        assert!(!m.entries.is_empty());
    }

    /// The shipped matrix must not claim anything is supported until a real
    /// device has been tested. This test is what keeps Phase 0 honest.
    #[test]
    fn the_bundled_matrix_grants_no_writes_yet() {
        let m = CompatibilityMatrix::bundled();
        let firmware = fw("3.11.2");
        for capability in [
            Capability::SafeDocumentTransfer,
            Capability::DocumentRemoval,
            Capability::NativeTagsWrite,
            Capability::PdfAnnotationExport,
        ] {
            let status = m.resolve(DeviceKind::Rm2, &firmware, capability);
            assert!(
                !status.permits_write(),
                "{capability:?} must not permit writes until validated on hardware"
            );
        }
    }

    #[test]
    fn an_untested_entry_loads_as_unknown_however_optimistic_it_is() {
        let toml = r#"
[[entry]]
model = "RM2"
firmware = "3.x"
capability = "SafeDocumentTransfer"
status = "SUPPORTED"
tested_at = ""
"#;
        let m = CompatibilityMatrix::from_toml(toml).unwrap();
        let status = m.resolve(
            DeviceKind::Rm2,
            &fw("3.11.2"),
            Capability::SafeDocumentTransfer,
        );
        assert_eq!(
            status,
            CapabilityStatus::Unknown,
            "a SUPPORTED claim with no test date must not grant anything"
        );
    }

    #[test]
    fn a_tested_entry_resolves_properly() {
        let toml = r#"
[[entry]]
model = "RM2"
firmware = "3.11"
capability = "MetadataRead"
status = "SUPPORTED"
tested_at = "2026-01-15"
method = "usb_web_interface"
"#;
        let m = CompatibilityMatrix::from_toml(toml).unwrap();
        assert_eq!(
            m.resolve(DeviceKind::Rm2, &fw("3.11.2"), Capability::MetadataRead),
            CapabilityStatus::Supported
        );
    }

    #[test]
    fn a_missing_entry_is_unknown() {
        let m = CompatibilityMatrix::default();
        assert_eq!(
            m.resolve(DeviceKind::Rm2, &fw("3.11.2"), Capability::MetadataRead),
            CapabilityStatus::Unknown
        );
    }

    #[test]
    fn a_newer_major_firmware_does_not_inherit_permissions() {
        let toml = r#"
[[entry]]
model = "RM2"
firmware = "3.x"
capability = "SafeDocumentTransfer"
status = "SUPPORTED"
tested_at = "2026-01-15"
"#;
        let m = CompatibilityMatrix::from_toml(toml).unwrap();
        assert_eq!(
            m.resolve(
                DeviceKind::Rm2,
                &fw("3.11.2"),
                Capability::SafeDocumentTransfer
            ),
            CapabilityStatus::Supported
        );
        assert_eq!(
            m.resolve(
                DeviceKind::Rm2,
                &fw("4.0.0"),
                Capability::SafeDocumentTransfer
            ),
            CapabilityStatus::Unknown,
            "a firmware update must drop us back to read-only, not carry permissions forward"
        );
    }

    #[test]
    fn an_unrecognised_status_string_is_unknown() {
        let toml = r#"
[[entry]]
model = "RM2"
firmware = "3.x"
capability = "SafeDocumentTransfer"
status = "PROBABLY_FINE"
tested_at = "2026-01-15"
"#;
        let m = CompatibilityMatrix::from_toml(toml).unwrap();
        assert_eq!(
            m.resolve(
                DeviceKind::Rm2,
                &fw("3.11.2"),
                Capability::SafeDocumentTransfer
            ),
            CapabilityStatus::Unknown
        );
    }
}
