//! Where reading material comes from.
//!
//! Marginalia is a reading and annotation workflow for the reMarkable. Zotero
//! is **one** way to tell it what you are reading — a good one, and the first
//! implemented, but not the product.
//!
//! This module is the seam that keeps that true. Everything above it works in
//! terms of [`LibraryItem`], which knows nothing about Zotero; everything below
//! it is an adapter for one particular source.
//!
//! ```text
//!   Zotero API ─┐
//!   a folder ───┼─► LibraryProvider ─► LibraryItem ─► the workflow
//!   Calibre ────┤                                     (inbox · search ·
//!   a DOI ──────┘                                      notes · tags · reading)
//! ```
//!
//! # The firewall, restated for sources
//!
//! [`LibraryProvider`] has **no method that returns file bytes**. A source can
//! say a document exists and is reachable; fetching it is a separate operation
//! behind an explicit request. Adding a `fetch` here would be the moment
//! someone has to think about invariants 8 and 9 — which is exactly the point
//! of it not being here.

use crate::ids::DocumentId;
use crate::zotero::AttachmentAvailability;
use crate::Timestamp;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Which source an item came from.
///
/// Kept on every item because provenance matters as much for "where did this
/// paper come from" as it does for "where did this highlight come from".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LibrarySource {
    /// A Zotero library, personal or group.
    Zotero,
    /// A folder of documents — on the device, or on a machine you sync from.
    Folder,
    /// Documents already on the reMarkable, discovered rather than imported.
    Device,
    /// Added by hand, with metadata you typed.
    Manual,
}

impl LibrarySource {
    /// Whether this source needs the network to consult.
    ///
    /// The workflow must stay usable offline, so anything that answers `true`
    /// has to degrade to cached data rather than to an error.
    pub const fn needs_network(self) -> bool {
        matches!(self, LibrarySource::Zotero)
    }
}

impl fmt::Display for LibrarySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LibrarySource::Zotero => "Zotero",
            LibrarySource::Folder => "folder",
            LibrarySource::Device => "on the device",
            LibrarySource::Manual => "added by hand",
        })
    }
}

/// A stable identifier for one item within its source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceRef(String);

impl SourceRef {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A person credited on a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub family: String,
    pub given: Option<String>,
}

impl Author {
    pub fn new(family: impl Into<String>, given: Option<String>) -> Self {
        Self {
            family: family.into(),
            given,
        }
    }

    /// "Vaswani" / "Vaswani et al." is a list concern; this is one name.
    pub fn short(&self) -> &str {
        &self.family
    }
}

/// Stable external identifiers, when a source knows any.
///
/// Kept separate from the free-text fields because these are what make it
/// possible to notice that a paper from a folder and a paper from Zotero are
/// the same paper.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identifiers {
    pub doi: Option<String>,
    pub isbn: Option<String>,
    pub arxiv: Option<String>,
    pub url: Option<String>,
}

impl Identifiers {
    pub fn is_empty(&self) -> bool {
        self.doi.is_none() && self.isbn.is_none() && self.arxiv.is_none() && self.url.is_none()
    }

    /// The strongest identifier available, for matching across sources.
    ///
    /// DOI first, then arXiv, then ISBN. A URL is deliberately last and never
    /// used alone for matching — two people can save the same landing page for
    /// different things.
    pub fn strongest(&self) -> Option<&str> {
        self.doi
            .as_deref()
            .or(self.arxiv.as_deref())
            .or(self.isbn.as_deref())
    }
}

/// One thing you might read, described in terms no source owns.
///
/// This is what the whole workflow — inbox, search, notes, tags, reading state
/// — actually operates on. Nothing above this type knows what Zotero is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub source: LibrarySource,
    pub source_ref: SourceRef,

    pub title: String,
    pub authors: Vec<Author>,
    pub year: Option<i32>,
    /// Journal, book, conference — whatever contains it.
    pub container: Option<String>,
    pub identifiers: Identifiers,

    pub tags: Vec<String>,
    /// Path from the source's own organisation: `["AI", "Transformers"]`.
    pub collections: Vec<String>,

    /// Whether the document itself can be obtained. Says nothing about whether
    /// it *has* been — that is device state, not library state.
    pub availability: AttachmentAvailability,
    pub size_bytes: Option<u64>,

    pub added_at: Option<Timestamp>,
}

impl LibraryItem {
    /// The byline a list shows: "Vaswani et al." or "Hofstadter".
    pub fn byline(&self) -> String {
        match self.authors.len() {
            0 => "Unknown".to_string(),
            1 => self.authors[0].short().to_string(),
            2 => format!("{} & {}", self.authors[0].short(), self.authors[1].short()),
            _ => format!("{} et al.", self.authors[0].short()),
        }
    }

    /// Whether this item could be brought onto the device if asked.
    pub fn can_be_requested(&self) -> bool {
        self.availability == AttachmentAvailability::AvailableLocal
    }

    /// Whether two records from different sources describe the same work.
    ///
    /// Only a shared strong identifier counts. Matching on title would merge
    /// two different papers with the same name, and a research library is
    /// exactly where that happens.
    pub fn is_same_work_as(&self, other: &LibraryItem) -> bool {
        match (self.identifiers.strongest(), other.identifiers.strongest()) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

/// What a configured source is, for the settings screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub source: LibrarySource,
    /// "youcef's Zotero library", "/home/root/papers".
    pub label: String,
    pub item_count: Option<u32>,
    pub last_refreshed: Option<Timestamp>,
}

/// A page of items, however the source paginates.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryPage {
    pub items: Vec<LibraryItem>,
    /// Opaque to the caller: only the provider knows what it means.
    pub next: Option<String>,
}

impl LibraryPage {
    pub fn last(items: Vec<LibraryItem>) -> Self {
        Self { items, next: None }
    }

    pub fn is_last(&self) -> bool {
        self.next.is_none()
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LibraryError {
    #[error("this source is not configured yet")]
    NotConfigured,

    #[error("the source refused access")]
    Unauthorized,

    #[error("the source could not be reached: {0}")]
    Unreachable(String),

    #[error("the source is temporarily unavailable; retry after {retry_after_secs}s")]
    Busy { retry_after_secs: u64 },

    #[error("the source returned something unexpected: {0}")]
    Malformed(String),
}

impl LibraryError {
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            LibraryError::Unreachable(_) | LibraryError::Busy { .. }
        )
    }
}

/// Somewhere reading material comes from.
///
/// Note what is missing: **no method returns file bytes**. A provider can say a
/// document exists and is reachable. Fetching it is a separate operation,
/// behind an explicit request, through the safety layer.
pub trait LibraryProvider {
    fn source(&self) -> LibrarySource;

    /// What to show in settings.
    fn info(&self) -> Result<SourceInfo, LibraryError>;

    /// One page of items. `cursor` is whatever the previous page returned.
    fn list(&self, cursor: Option<&str>) -> Result<LibraryPage, LibraryError>;
}

/// The reading workflow's own state, which is not a library concern at all.
///
/// Kept here to make the separation obvious: a source says what exists, and
/// this says what *you* are doing with it. A folder cannot tell you that you
/// are halfway through something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReadingStatus {
    Unread,
    Reading,
    Completed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadingProgress {
    pub document_id: DocumentId,
    pub status: ReadingStatus,
    pub current_page: Option<u32>,
    pub total_pages: Option<u32>,
    pub last_opened_at: Option<Timestamp>,
}

impl ReadingProgress {
    pub fn percent(&self) -> Option<u8> {
        match (self.current_page, self.total_pages) {
            (Some(c), Some(t)) if t > 0 => Some(((c as f64 / t as f64) * 100.0).round() as u8),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: LibrarySource, title: &str, authors: Vec<Author>) -> LibraryItem {
        LibraryItem {
            source,
            source_ref: SourceRef::new("ref"),
            title: title.into(),
            authors,
            year: Some(2017),
            container: None,
            identifiers: Identifiers::default(),
            tags: vec![],
            collections: vec![],
            availability: AttachmentAvailability::Unknown,
            size_bytes: None,
            added_at: None,
        }
    }

    fn author(family: &str) -> Author {
        Author::new(family, None)
    }

    #[test]
    fn a_byline_reads_the_way_a_person_would_write_it() {
        assert_eq!(item(LibrarySource::Folder, "x", vec![]).byline(), "Unknown");
        assert_eq!(
            item(LibrarySource::Folder, "x", vec![author("Hofstadter")]).byline(),
            "Hofstadter"
        );
        assert_eq!(
            item(
                LibrarySource::Folder,
                "x",
                vec![author("Gu"), author("Dao")]
            )
            .byline(),
            "Gu & Dao"
        );
        assert_eq!(
            item(
                LibrarySource::Zotero,
                "x",
                vec![author("Vaswani"), author("Shazeer"), author("Parmar")]
            )
            .byline(),
            "Vaswani et al."
        );
    }

    #[test]
    fn only_a_shared_strong_identifier_means_the_same_work() {
        // Matching on title would merge two different papers with the same
        // name, and a research library is exactly where that happens.
        let mut a = item(LibrarySource::Zotero, "Attention Is All You Need", vec![]);
        let mut b = item(LibrarySource::Folder, "Attention Is All You Need", vec![]);
        assert!(
            !a.is_same_work_as(&b),
            "identical titles are not evidence of identity"
        );

        a.identifiers.doi = Some("10.5555/3295222".into());
        b.identifiers.doi = Some("10.5555/3295222".into());
        assert!(a.is_same_work_as(&b));
    }

    #[test]
    fn identifier_matching_is_case_insensitive_but_not_loose() {
        let mut a = item(LibrarySource::Zotero, "A", vec![]);
        let mut b = item(LibrarySource::Folder, "B", vec![]);
        a.identifiers.doi = Some("10.5555/ABC".into());
        b.identifiers.doi = Some("10.5555/abc".into());
        assert!(a.is_same_work_as(&b));

        b.identifiers.doi = Some("10.5555/abd".into());
        assert!(!a.is_same_work_as(&b));
    }

    #[test]
    fn a_url_alone_never_establishes_identity() {
        // Two people can save the same landing page for different things.
        let mut a = item(LibrarySource::Zotero, "A", vec![]);
        let mut b = item(LibrarySource::Folder, "B", vec![]);
        a.identifiers.url = Some("https://arxiv.org/abs/1706.03762".into());
        b.identifiers.url = Some("https://arxiv.org/abs/1706.03762".into());

        assert_eq!(a.identifiers.strongest(), None);
        assert!(!a.is_same_work_as(&b));
    }

    #[test]
    fn identifier_strength_has_an_order() {
        let ids = Identifiers {
            doi: Some("10.1/x".into()),
            arxiv: Some("1706.03762".into()),
            isbn: Some("9780465026562".into()),
            url: None,
        };
        assert_eq!(ids.strongest(), Some("10.1/x"));

        let ids = Identifiers {
            arxiv: Some("1706.03762".into()),
            ..Default::default()
        };
        assert_eq!(ids.strongest(), Some("1706.03762"));
        assert!(!ids.is_empty());
        assert!(Identifiers::default().is_empty());
    }

    #[test]
    fn only_an_available_document_can_be_requested() {
        let mut i = item(LibrarySource::Folder, "x", vec![]);
        assert!(!i.can_be_requested());

        i.availability = AttachmentAvailability::AvailableLocal;
        assert!(i.can_be_requested());

        i.availability = AttachmentAvailability::Unreadable;
        assert!(!i.can_be_requested());
    }

    #[test]
    fn a_folder_source_works_offline_and_zotero_does_not() {
        // Anything that needs the network must degrade to cached data rather
        // than to an error, so the workflow stays usable on a train.
        assert!(!LibrarySource::Folder.needs_network());
        assert!(!LibrarySource::Device.needs_network());
        assert!(!LibrarySource::Manual.needs_network());
        assert!(LibrarySource::Zotero.needs_network());
    }

    #[test]
    fn reading_progress_is_the_workflows_own_state() {
        // A folder cannot tell you that you are halfway through something.
        let p = ReadingProgress {
            document_id: DocumentId::new(),
            status: ReadingStatus::Reading,
            current_page: Some(7),
            total_pages: Some(15),
            last_opened_at: None,
        };
        assert_eq!(p.percent(), Some(47));
    }

    #[test]
    fn progress_without_a_page_count_reports_nothing_rather_than_zero() {
        let p = ReadingProgress {
            document_id: DocumentId::new(),
            status: ReadingStatus::Reading,
            current_page: Some(7),
            total_pages: None,
            last_opened_at: None,
        };
        assert_eq!(p.percent(), None);
    }

    #[test]
    fn a_page_knows_whether_it_is_the_last() {
        assert!(LibraryPage::last(vec![]).is_last());
        assert!(!LibraryPage {
            items: vec![],
            next: Some("cursor".into())
        }
        .is_last());
    }

    #[test]
    fn transient_source_errors_are_distinguished() {
        assert!(LibraryError::Unreachable("dns".into()).is_transient());
        assert!(LibraryError::Busy {
            retry_after_secs: 30
        }
        .is_transient());
        assert!(!LibraryError::Unauthorized.is_transient());
        assert!(!LibraryError::NotConfigured.is_transient());
    }
}
