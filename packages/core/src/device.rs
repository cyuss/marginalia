//! Device identity, firmware, and capabilities.
//!
//! Implements `docs/architecture/DEVICE_CAPABILITY_MODEL.md`.
//!
//! WHY no feature module may parse a firmware string: reMarkable firmware
//! evolves, and `if firmware.starts_with("3.")` is a bet that the future
//! resembles the past. Feature code asks what the device *can do*; only this
//! module and the compatibility matrix decide what the answer is.

use crate::error::CoreError;
use crate::ids::DeviceId;
use crate::Timestamp;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceKind {
    /// The V1 target.
    Rm2,
    /// Reserved so the design does not have to change later. Not supported.
    Rm1,
    /// Reserved. Not supported.
    RmPaperPro,
    Unknown,
}

/// A parsed firmware version, keeping the raw string alongside.
///
/// The raw string is retained because our parse may be wrong for a future
/// format, and an unparseable firmware must still be reportable to the user
/// and storable in a bug report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirmwareVersion {
    pub raw: String,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl FirmwareVersion {
    pub fn parse(raw: &str) -> Result<Self, CoreError> {
        let mut parts = raw.split('.');
        let mut next = |field: &str| -> Result<u32, CoreError> {
            parts
                .next()
                .and_then(|p| p.parse::<u32>().ok())
                .ok_or_else(|| {
                    let _ = field;
                    CoreError::InvalidFirmware(raw.to_string())
                })
        };
        let major = next("major")?;
        let minor = next("minor")?;
        let patch = next("patch").unwrap_or(0);
        Ok(Self {
            raw: raw.to_string(),
            major,
            minor,
            patch,
        })
    }

    /// Whether this version is covered by a matrix range such as `"3.x"`.
    ///
    /// A range never absorbs a future major version: `3.x` does not match
    /// `4.0.0`. A firmware we have not considered is UNKNOWN, and UNKNOWN means
    /// read-only.
    pub fn matches_range(&self, range: &str) -> bool {
        let mut parts = range.split('.');
        let major = parts.next();
        let minor = parts.next();

        let major_ok = match major {
            Some("x") | Some("*") => false, // a wildcard major is never accepted
            Some(m) => m.parse::<u32>().map(|v| v == self.major).unwrap_or(false),
            None => false,
        };
        if !major_ok {
            return false;
        }

        match minor {
            None | Some("x") | Some("*") => true,
            Some(m) => m.parse::<u32>().map(|v| v == self.minor).unwrap_or(false),
        }
    }
}

impl fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

/// The things a device might be able to do, from Marginalia's point of view.
///
/// `Ord` is derived so capabilities can key a `BTreeMap` — the ordering carries
/// no meaning and must not be used to compare how dangerous two capabilities
/// are. That is what [`SafetyClass`](../../safety/classification) is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Capability {
    DeviceInfoRead,
    MetadataRead,
    StorageRead,
    AnnotationRead,
    NativeTagsRead,

    SafeDocumentTransfer,
    DocumentRemoval,
    NativeTagsWrite,
    PdfAnnotationExport,

    CompanionApp,
    ExperimentalRmUi,

    /// Exists **only** so the safety layer can name and refuse it. There is no
    /// implementation behind this variant and there never will be.
    SystemModification,
}

impl Capability {
    /// Whether exercising this capability changes device state.
    pub fn is_write(self) -> bool {
        matches!(
            self,
            Capability::SafeDocumentTransfer
                | Capability::DocumentRemoval
                | Capability::NativeTagsWrite
                | Capability::PdfAnnotationExport
                | Capability::CompanionApp
                | Capability::ExperimentalRmUi
                | Capability::SystemModification
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityStatus {
    Supported,
    ReadOnly,
    Experimental,
    Unsupported,
    /// The default for anything we have not verified. Never grants a write.
    Unknown,
}

impl Default for CapabilityStatus {
    /// Fail closed. An uninitialised capability is UNKNOWN, not Supported.
    fn default() -> Self {
        CapabilityStatus::Unknown
    }
}

impl CapabilityStatus {
    /// The single question the safety layer asks.
    pub fn permits_write(self) -> bool {
        matches!(self, CapabilityStatus::Supported)
    }

    pub fn permits_read(self) -> bool {
        matches!(
            self,
            CapabilityStatus::Supported
                | CapabilityStatus::ReadOnly
                | CapabilityStatus::Experimental
                | CapabilityStatus::Unknown
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilitySource {
    /// Lowest precedence.
    Matrix,
    Probed,
    /// Highest precedence, and may only *downgrade*. There is no user override
    /// that enables writes on untested firmware.
    UserOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCapability {
    pub capability: Capability,
    pub status: CapabilityStatus,
    pub source: CapabilitySource,
    pub tested_at: Option<Timestamp>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectionKind {
    Usb,
    Wifi,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub kind: DeviceKind,
    /// A hash. We never store the device serial itself.
    pub serial_hash: Option<String>,
    pub display_name: String,
    pub firmware: Option<FirmwareVersion>,
    pub connection: ConnectionKind,
    pub last_seen_at: Option<Timestamp>,
    pub storage: Option<StorageInfo>,
    /// ON by default, for every device, always.
    pub safe_mode: bool,
}

impl Device {
    /// A device whose firmware we could not parse is, by definition, unknown —
    /// and unknown means read-only.
    pub fn firmware_is_known(&self) -> bool {
        self.firmware.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageInfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// How close to full is too close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageVerdict {
    Ok,
    /// Allowed, but the user is warned.
    Low,
    /// Refused. The reserve exists so a device is never driven to full by us.
    Critical,
}

impl StorageInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    /// Free space after a hypothetical write, or `None` if it would not fit.
    pub fn free_after(&self, incoming_bytes: u64) -> Option<u64> {
        self.free_bytes.checked_sub(incoming_bytes)
    }

    /// Whether `incoming_bytes` may be written while preserving `reserve_bytes`.
    ///
    /// The reserve is never spendable. A device that cannot keep its reserve
    /// after a transfer does not get the transfer.
    pub fn can_accept(&self, incoming_bytes: u64, reserve_bytes: u64) -> bool {
        match self.free_after(incoming_bytes) {
            Some(remaining) => remaining >= reserve_bytes,
            None => false,
        }
    }

    pub fn verdict(&self, incoming_bytes: u64, reserve_bytes: u64) -> StorageVerdict {
        if !self.can_accept(incoming_bytes, reserve_bytes) {
            return StorageVerdict::Critical;
        }
        // Warn while there is still room to act, not once it is too late.
        let remaining = self.free_after(incoming_bytes).unwrap_or(0);
        if remaining < reserve_bytes.saturating_mul(2) {
            StorageVerdict::Low
        } else {
            StorageVerdict::Ok
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;
    const RESERVE: u64 = 500 * MB;

    #[test]
    fn firmware_parses_and_keeps_the_raw_string() {
        let fw = FirmwareVersion::parse("3.11.2").unwrap();
        assert_eq!((fw.major, fw.minor, fw.patch), (3, 11, 2));
        assert_eq!(fw.raw, "3.11.2");
    }

    #[test]
    fn unparseable_firmware_is_an_error_not_a_guess() {
        assert!(FirmwareVersion::parse("beta-unknown").is_err());
        assert!(FirmwareVersion::parse("").is_err());
    }

    #[test]
    fn a_range_never_absorbs_a_future_major_version() {
        let v3 = FirmwareVersion::parse("3.11.2").unwrap();
        let v4 = FirmwareVersion::parse("4.0.0").unwrap();

        assert!(v3.matches_range("3.x"));
        assert!(
            !v4.matches_range("3.x"),
            "a v4 device must be UNKNOWN, not assumed compatible"
        );
        assert!(!v4.matches_range("x"), "a wildcard major must never match");
    }

    #[test]
    fn unknown_is_the_default_status() {
        assert_eq!(CapabilityStatus::default(), CapabilityStatus::Unknown);
        assert!(!CapabilityStatus::default().permits_write());
    }

    #[test]
    fn only_supported_permits_a_write() {
        for status in [
            CapabilityStatus::Unknown,
            CapabilityStatus::ReadOnly,
            CapabilityStatus::Unsupported,
            CapabilityStatus::Experimental,
        ] {
            assert!(!status.permits_write(), "{status:?} must not permit writes");
        }
        assert!(CapabilityStatus::Supported.permits_write());
    }

    #[test]
    fn the_reserve_is_not_spendable() {
        let s = StorageInfo {
            total_bytes: 6 * GB,
            free_bytes: 600 * MB,
        };
        // Fits in raw free space, but would eat into the reserve.
        assert!(!s.can_accept(200 * MB, RESERVE));
        assert_eq!(s.verdict(200 * MB, RESERVE), StorageVerdict::Critical);
    }

    #[test]
    fn a_comfortable_transfer_is_ok() {
        let s = StorageInfo {
            total_bytes: 6 * GB,
            free_bytes: 3 * GB,
        };
        assert!(s.can_accept(84 * MB, RESERVE));
        assert_eq!(s.verdict(84 * MB, RESERVE), StorageVerdict::Ok);
        assert_eq!(s.free_after(84 * MB), Some(3 * GB - 84 * MB));
    }

    #[test]
    fn a_document_larger_than_free_space_does_not_underflow() {
        let s = StorageInfo {
            total_bytes: 6 * GB,
            free_bytes: 100 * MB,
        };
        assert_eq!(s.free_after(300 * MB), None);
        assert!(!s.can_accept(300 * MB, 0));
    }

    #[test]
    fn system_modification_is_classified_as_a_write() {
        // It must never be reachable, but if it is ever asked for, it must at
        // least be recognised as the most dangerous thing in the enum.
        assert!(Capability::SystemModification.is_write());
    }
}
