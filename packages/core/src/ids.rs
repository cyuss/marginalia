//! Typed identifiers.
//!
//! WHY newtypes rather than a bare `String`: a `DocumentId` and a
//! `RemarkableDocumentId` are both strings, and confusing them is exactly the
//! kind of mistake that ends with writing to the wrong document on someone's
//! device. The compiler should refuse that, not a code reviewer.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Generate a fresh, sortable identifier.
            pub fn new() -> Self {
                Self(ulid::Ulid::new().to_string())
            }

            pub fn from_string(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

define_id!(
    DocumentId,
    "Identity of a document as Marginalia tracks it."
);
define_id!(MappingId, "Identity of a `DocumentMapping` row.");
define_id!(HighlightId, "Identity of a highlight.");
define_id!(SideNoteId, "Identity of a side note.");
define_id!(StickyNoteId, "Identity of a sticky note.");
define_id!(DeviceId, "Marginalia's identity for a physical device.");
define_id!(SnapshotId, "Identity of a `SafetySnapshot`.");
define_id!(SyncJobId, "Identity of a sync job.");
define_id!(SyncOperationId, "Identity of one operation inside a job.");
define_id!(TagId, "Identity of a tag.");
define_id!(TagMappingId, "Identity of a tag mapping.");
define_id!(
    ZoteroItemId,
    "Marginalia's local id for a mirrored Zotero item."
);
define_id!(
    ZoteroAttachmentId,
    "Marginalia's local id for a mirrored Zotero attachment."
);

/// A document's UUID **as reported by the device**.
///
/// Deliberately distinct from [`DocumentId`]. This value is authoritative for
/// device identity, and a device document whose UUID is absent from our
/// mappings is not ours to touch.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RemarkableDocumentId(String);

impl RemarkableDocumentId {
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemarkableDocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A Zotero item or attachment key (8-character Zotero identifier).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ZoteroKey(String);

impl ZoteroKey {
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ZoteroKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        assert_ne!(DocumentId::new(), DocumentId::new());
    }

    #[test]
    fn ids_sort_by_creation_time() {
        // ULIDs sort lexicographically by timestamp, which lets us page through
        // history without a separate ordering column.
        //
        // The guarantee is at *millisecond* granularity: two ULIDs minted
        // within the same millisecond differ only in their random suffix and
        // have no defined order between them. So the test crosses a millisecond
        // boundary rather than asserting something ULID does not promise.
        let a = DocumentId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = DocumentId::new();
        assert!(a.as_str() < b.as_str());
    }
}
