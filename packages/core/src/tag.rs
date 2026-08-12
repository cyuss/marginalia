//! The tag bridge.
//!
//! reMarkable has native tags. Zotero has tags. Marginalia does not add a
//! third system — it maps between the two, and only where the user has said
//! the mapping is correct.
//!
//! WHY confirmation is mandatory: `machine-learning` and `Machine Learning`
//! *look* like the same tag, and usually are. "Usually" is not good enough
//! when the consequence is silently renaming tags across someone's research
//! library.

use crate::ids::{TagId, TagMappingId};
use crate::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TagNamespace {
    Zotero,
    Remarkable,
    Marginalia,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub namespace: TagNamespace,
    pub name: String,
    pub normalized_name: String,
}

impl Tag {
    /// Case-folded, whitespace/underscore/hyphen-collapsed form used only to
    /// *suggest* a mapping. Never used to merge tags automatically.
    pub fn normalize(name: &str) -> String {
        name.trim()
            .to_lowercase()
            .chars()
            .map(|c| if c == '_' || c == '-' { ' ' } else { c })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn new(namespace: TagNamespace, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: TagId::new(),
            namespace,
            normalized_name: Self::normalize(&name),
            name,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TagDirection {
    ZoteroToRm,
    RmToZotero,
    Bidirectional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagMapping {
    pub id: TagMappingId,
    pub zotero_tag: String,
    pub remarkable_tag: String,
    pub direction: TagDirection,
    /// Until this is true, the mapping is a suggestion and nothing else.
    pub confirmed_by_user: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl TagMapping {
    /// The only question the executor asks before acting on a mapping.
    pub fn may_be_applied(&self) -> bool {
        self.confirmed_by_user
    }
}

/// A proposed equivalence, awaiting a human.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagConflict {
    pub zotero_tag: String,
    pub remarkable_tag: String,
}

impl TagConflict {
    /// Detect tags that normalise identically but are written differently.
    ///
    /// Returns suggestions, not decisions.
    pub fn detect(zotero_tags: &[String], remarkable_tags: &[String]) -> Vec<TagConflict> {
        let mut out = Vec::new();
        for z in zotero_tags {
            for r in remarkable_tags {
                if z != r && Tag::normalize(z) == Tag::normalize(r) {
                    out.push(TagConflict {
                        zotero_tag: z.clone(),
                        remarkable_tag: r.clone(),
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn normalisation_folds_case_and_separators() {
        assert_eq!(Tag::normalize("Machine Learning"), "machine learning");
        assert_eq!(Tag::normalize("machine-learning"), "machine learning");
        assert_eq!(Tag::normalize("  machine_learning  "), "machine learning");
    }

    #[test]
    fn normalisation_does_not_conflate_genuinely_different_tags() {
        assert_ne!(Tag::normalize("RAG"), Tag::normalize("RAGE"));
        assert_ne!(
            Tag::normalize("must-read"),
            Tag::normalize("must read later")
        );
    }

    #[test]
    fn conflicts_are_detected_but_not_resolved() {
        let conflicts = TagConflict::detect(
            &["machine-learning".into(), "RAG".into()],
            &["Machine Learning".into(), "RAG".into()],
        );
        // The identical "RAG" pair is not a conflict; the differing pair is.
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].zotero_tag, "machine-learning");
        assert_eq!(conflicts[0].remarkable_tag, "Machine Learning");
    }

    #[test]
    fn an_unconfirmed_mapping_is_never_applied() {
        let mut m = TagMapping {
            id: TagMappingId::new(),
            zotero_tag: "machine-learning".into(),
            remarkable_tag: "Machine Learning".into(),
            direction: TagDirection::Bidirectional,
            confirmed_by_user: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(!m.may_be_applied());

        m.confirmed_by_user = true;
        assert!(m.may_be_applied());
    }
}
