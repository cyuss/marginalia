//! The `DeviceProvider` port.
//!
//! # How the safety rule is expressed
//!
//! Read methods take `&self` and nothing else. Write methods take a
//! [`WriteGrant`], which cannot be constructed outside `marginalia-safety`.
//! A new transport implementing this trait therefore *cannot* offer a write
//! that skipped authorisation — there would be no way to call it.
//!
//! Note what the trait does not contain: there is no `delete_any_document`,
//! no `write_file`, no `run_command`, no `install`. The port is the whole
//! vocabulary available to the rest of the program, and it was chosen to make
//! dangerous sentences unsayable.

use marginalia_core::device::{Device, StorageInfo};
use marginalia_core::ids::RemarkableDocumentId;
use marginalia_core::Checksum;
use marginalia_safety::WriteGrant;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceProviderError {
    #[error("no device is connected")]
    NotConnected,

    #[error("the connection was lost during the operation")]
    ConnectionLost,

    #[error("the device refused the request: {0}")]
    Refused(String),

    #[error("the grant does not cover this operation")]
    GrantMismatch,

    #[error("the grant has expired")]
    GrantExpired,

    #[error("verification failed after the operation: {0}")]
    VerificationFailed(String),

    #[error("this device document was not transferred by Marginalia and will not be modified")]
    NotOwnedByMarginalia,

    #[error("transport error: {0}")]
    Transport(String),
}

pub type DeviceResult<T> = Result<T, DeviceProviderError>;

/// A document as the device reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDocument {
    pub uuid: RemarkableDocumentId,
    pub visible_name: String,
    pub parent: Option<String>,
    pub size_bytes: Option<u64>,
    pub has_annotations: bool,
    pub native_tags: Vec<String>,
}

/// A PDF that has been structurally validated and hashed.
///
/// WHY a distinct type: the upload method accepts only this, so "we validated
/// it" is a fact carried by the value rather than a step someone might skip.
/// It is constructed by the PDF layer after validation, never from a raw path.
#[derive(Debug, Clone)]
pub struct ValidatedPdf {
    working_copy_path: String,
    checksum: Checksum,
    size_bytes: u64,
    page_count: u32,
}

impl ValidatedPdf {
    /// Called by the PDF engine after a successful structural validation of a
    /// **working copy**. The path is never the user's original.
    pub fn new(
        working_copy_path: impl Into<String>,
        checksum: Checksum,
        size_bytes: u64,
        page_count: u32,
    ) -> Self {
        Self {
            working_copy_path: working_copy_path.into(),
            checksum,
            size_bytes,
            page_count,
        }
    }

    pub fn working_copy_path(&self) -> &str {
        &self.working_copy_path
    }
    pub fn checksum(&self) -> &Checksum {
        &self.checksum
    }
    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
    pub fn page_count(&self) -> u32 {
        self.page_count
    }
}

/// Everything Marginalia may ask of a physical device.
pub trait DeviceProvider {
    // ── GREEN: reads. No grant required. ─────────────────────────────────

    fn detect(&self) -> DeviceResult<Device>;
    fn read_storage(&self) -> DeviceResult<StorageInfo>;
    fn list_documents(&self) -> DeviceResult<Vec<RemoteDocument>>;
    fn read_native_tags(&self, uuid: &RemarkableDocumentId) -> DeviceResult<Vec<String>>;
    /// Copies annotation data to the host. Nothing is parsed on the device.
    fn read_annotations(&self, uuid: &RemarkableDocumentId) -> DeviceResult<Vec<u8>>;
    /// Re-read a transferred document's checksum, for post-transfer verification.
    fn checksum_of(&self, uuid: &RemarkableDocumentId) -> DeviceResult<Checksum>;

    // ── YELLOW: writes. A grant is a required parameter. ─────────────────

    /// Put exactly one validated PDF on the device.
    ///
    /// Implementations must verify `grant.covers(...)` and
    /// `grant.is_valid_at(now)` before touching anything.
    fn upload_document(
        &mut self,
        grant: &WriteGrant,
        pdf: &ValidatedPdf,
        visible_name: &str,
    ) -> DeviceResult<RemarkableDocumentId>;

    /// Remove exactly one document that Marginalia itself transferred.
    fn remove_document(
        &mut self,
        grant: &WriteGrant,
        uuid: &RemarkableDocumentId,
    ) -> DeviceResult<()>;

    /// Set native tags on a document Marginalia manages.
    fn write_native_tags(
        &mut self,
        grant: &WriteGrant,
        uuid: &RemarkableDocumentId,
        tags: &[String],
    ) -> DeviceResult<()>;

    /// Undo the effect of a failed write.
    ///
    /// Returns an error if the rollback itself could not be completed — the
    /// caller must then mark the device read-only and tell the user. We never
    /// guess at a second cleanup attempt.
    fn rollback_upload(
        &mut self,
        grant: &WriteGrant,
        uuid: &RemarkableDocumentId,
    ) -> DeviceResult<()>;
}
