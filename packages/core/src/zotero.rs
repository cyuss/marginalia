//! Mirrored Zotero entities.
//!
//! Zotero is the bibliographic source of truth. Marginalia mirrors it
//! read-mostly and never tries to become a second Zotero.

use crate::ids::{ZoteroAttachmentId, ZoteroItemId, ZoteroKey};
use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// Whether the attachment's file is usable **on this machine**.
///
/// WHY this is a separate axis from `DocumentState`: knowing a PDF exists is a
/// fact about the library. Putting it on a device is an action. Conflating them
/// is exactly the mistake that produces "sync filled up my reMarkable".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttachmentAvailability {
    Unknown,
    /// Zotero knows of the file; it is not on this machine.
    NotPresent,
    /// Resolved and readable. Displayed as "PDF available".
    ///
    /// This state authorises **nothing**. It enables a button; it does not
    /// press it.
    AvailableLocal,
    /// The path exists but the file is invalid, corrupt, or unreadable.
    Unreadable,
}

impl AttachmentAvailability {
    /// Whether the `Send to reMarkable` action should be offered at all.
    pub fn can_be_offered_for_send(self) -> bool {
        self == AttachmentAvailability::AvailableLocal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkMode {
    ImportedFile,
    ImportedUrl,
    LinkedFile,
    LinkedUrl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Creator {
    pub creator_type: String,
    pub first_name: Option<String>,
    pub last_name: String,
}

impl Creator {
    /// "Vaswani et al." style rendering is a UI concern; this is the raw name.
    pub fn display(&self) -> String {
        match &self.first_name {
            Some(first) => format!("{first} {}", self.last_name),
            None => self.last_name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoteroItem {
    pub id: ZoteroItemId,
    pub zotero_key: ZoteroKey,
    /// Watermark for incremental sync.
    pub zotero_version: i64,
    pub library_id: String,
    pub item_type: String,
    pub title: Option<String>,
    pub creators: Vec<Creator>,
    pub publication: Option<String>,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub isbn: Option<String>,
    pub url: Option<String>,
    pub abstract_text: Option<String>,
    pub date_added: Option<Timestamp>,
    pub date_modified: Option<Timestamp>,
    /// The full upstream payload.
    ///
    /// WHY keep it: Zotero can add fields faster than we model them. Storing
    /// the raw payload means a schema addition is a display gap, not data loss.
    pub raw: serde_json::Value,
    /// A remote deletion marks the item; it never cascades into deleting the
    /// user's local annotations.
    pub deleted_remote: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoteroAttachment {
    pub id: ZoteroAttachmentId,
    pub zotero_item_id: ZoteroItemId,
    pub zotero_key: ZoteroKey,
    pub link_mode: LinkMode,
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub local_path: Option<String>,
    pub file_size_bytes: Option<u64>,
    pub availability: AttachmentAvailability,
    pub checksum_sha256: Option<crate::Checksum>,
}

impl ZoteroAttachment {
    pub fn is_pdf(&self) -> bool {
        self.content_type.as_deref() == Some("application/pdf")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoteroCollection {
    pub zotero_key: ZoteroKey,
    pub zotero_version: i64,
    pub name: String,
    pub parent_key: Option<ZoteroKey>,
    pub library_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_enables_a_button_and_nothing_more() {
        assert!(AttachmentAvailability::AvailableLocal.can_be_offered_for_send());
        for a in [
            AttachmentAvailability::Unknown,
            AttachmentAvailability::NotPresent,
            AttachmentAvailability::Unreadable,
        ] {
            assert!(!a.can_be_offered_for_send());
        }
    }
}
