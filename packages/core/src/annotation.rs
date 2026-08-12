//! Highlights, side notes and sticky notes.
//!
//! WHY provenance is non-optional: an annotation you cannot trace back to a
//! page in a document is a fortune cookie. The entire value of the Annotation
//! Inbox rests on every item being able to answer "where did this come from?".

use crate::ids::{DocumentId, HighlightId, SideNoteId, StickyNoteId, ZoteroKey};
use crate::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AnnotationSource {
    Remarkable,
    Desktop,
    Zotero,
    Imported,
}

/// Where an annotation came from, precisely enough to navigate back to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub document_id: DocumentId,
    /// 1-based, matching the document's own page index.
    pub page_number: u32,
    pub source: AnnotationSource,
    /// Device page uuid, `.rm` layer id, or Zotero key.
    pub source_ref: Option<String>,
    /// Which extractor version produced this.
    ///
    /// WHY version it: when we improve highlight↔text mapping, we must be able
    /// to re-run extraction on old documents without silently mixing results
    /// from two different algorithms.
    pub extraction_version: u32,
    /// For text-mapped highlights, how confident the mapping was.
    pub confidence: Option<f32>,
}

/// A rectangle in **PDF user space** — origin bottom-left, units of 1/72 inch.
///
/// WHY state the coordinate system here: reMarkable canvas coordinates and PDF
/// user space differ in origin and scale, and a silent mix-up puts a highlight
/// on the wrong part of the page. Every value in this struct has already been
/// converted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl BoundingBox {
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }

    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}

/// A user-assigned meaning.
///
/// **Never inferred.** Marginalia does not guess that a passage is "important".
/// The user assigns these, or configures an explicit mapping (e.g. a highlight
/// colour to a type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HighlightType {
    Plain,
    Important,
    Citation,
    Question,
    Idea,
    Reference,
}

#[allow(clippy::derivable_impls)]
impl Default for HighlightType {
    /// Written out rather than derived so the rule is visible at the
    /// definition: an extracted highlight is `Plain` until a human says
    /// otherwise. Marginalia never infers that a passage is "important".
    fn default() -> Self {
        HighlightType::Plain
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Highlight {
    pub id: HighlightId,
    pub bounding_boxes: Vec<BoundingBox>,
    /// `None` when geometry could not be mapped to text.
    ///
    /// WHY nullable rather than a best guess: an approximate quotation in a
    /// research library is worse than an honest gap. We show the region and say
    /// we could not read it.
    pub selected_text: Option<String>,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub color: Option<String>,
    pub highlight_type: HighlightType,
    pub zotero_annotation_key: Option<ZoteroKey>,
    pub provenance: Provenance,
    pub created_at: Timestamp,
    pub modified_at: Timestamp,
}

impl Highlight {
    /// Whether we managed to turn this highlight into quotable text.
    pub fn has_text(&self) -> bool {
        self.selected_text
            .as_ref()
            .is_some_and(|t| !t.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentType {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SideNote {
    pub id: SideNoteId,
    /// A note may be page-level rather than anchored to a point.
    pub anchor_x: Option<f64>,
    pub anchor_y: Option<f64>,
    pub highlight_id: Option<HighlightId>,
    pub content: String,
    pub content_type: ContentType,
    pub zotero_note_key: Option<ZoteroKey>,
    pub provenance: Provenance,
    pub created_at: Timestamp,
    pub modified_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StickyNote {
    pub id: StickyNoteId,
    pub x: f64,
    pub y: f64,
    pub anchor_width: Option<f64>,
    pub anchor_height: Option<f64>,
    pub content: String,
    pub zotero_annotation_key: Option<ZoteroKey>,
    pub provenance: Provenance,
    pub created_at: Timestamp,
    pub modified_at: Timestamp,
}

/// The unified read model behind the Annotation Inbox and search.
///
/// WHY a projection rather than a trait object: the Inbox needs to sort, filter
/// and page across all three kinds at once. A flat value type keeps that a
/// query concern, and adding a fourth kind later does not fork the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationView {
    pub kind: AnnotationKind,
    pub id: String,
    pub document_id: DocumentId,
    pub page_number: u32,
    pub text: String,
    pub highlight_type: Option<HighlightType>,
    pub source: AnnotationSource,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AnnotationKind {
    Highlight,
    SideNote,
    StickyNote,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn bbox(x: f64, y: f64, w: f64, h: f64) -> BoundingBox {
        BoundingBox {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn overlapping_boxes_intersect() {
        assert!(bbox(0.0, 0.0, 10.0, 10.0).intersects(&bbox(5.0, 5.0, 10.0, 10.0)));
    }

    #[test]
    fn touching_boxes_do_not_intersect() {
        // Adjacency is not overlap; a highlight ending exactly where a glyph
        // begins should not claim that glyph.
        assert!(!bbox(0.0, 0.0, 10.0, 10.0).intersects(&bbox(10.0, 0.0, 10.0, 10.0)));
    }

    #[test]
    fn disjoint_boxes_do_not_intersect() {
        assert!(!bbox(0.0, 0.0, 5.0, 5.0).intersects(&bbox(100.0, 100.0, 5.0, 5.0)));
    }

    #[test]
    fn a_highlight_without_mapped_text_is_honest_about_it() {
        let h = Highlight {
            id: HighlightId::new(),
            bounding_boxes: vec![bbox(0.0, 0.0, 100.0, 12.0)],
            selected_text: None,
            context_before: None,
            context_after: None,
            color: None,
            highlight_type: HighlightType::default(),
            zotero_annotation_key: None,
            provenance: Provenance {
                document_id: DocumentId::new(),
                page_number: 7,
                source: AnnotationSource::Remarkable,
                source_ref: None,
                extraction_version: 1,
                confidence: None,
            },
            created_at: Utc::now(),
            modified_at: Utc::now(),
        };
        assert!(!h.has_text());
        assert_eq!(h.highlight_type, HighlightType::Plain);
    }

    #[test]
    fn whitespace_only_text_does_not_count_as_text() {
        let mut h = sample_highlight();
        h.selected_text = Some("   \n ".to_string());
        assert!(!h.has_text());
    }

    fn sample_highlight() -> Highlight {
        Highlight {
            id: HighlightId::new(),
            bounding_boxes: vec![],
            selected_text: None,
            context_before: None,
            context_after: None,
            color: None,
            highlight_type: HighlightType::default(),
            zotero_annotation_key: None,
            provenance: Provenance {
                document_id: DocumentId::new(),
                page_number: 1,
                source: AnnotationSource::Remarkable,
                source_ref: None,
                extraction_version: 1,
                confidence: None,
            },
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }
}
