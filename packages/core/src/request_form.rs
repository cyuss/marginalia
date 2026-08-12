//! The request form: how a user says "download this one" with a stylus.
//!
//! Implements the derivation described in
//! `docs/adr/ADR-006-on-device-interaction.md`.
//!
//! Marginalia generates an index document with a small empty box beside each
//! entry. The user ticks a box. On its next wake the agent reads that
//! document's annotation layer and turns marks into requests.
//!
//! # Why this module is pure
//!
//! Rendering the form and reading the annotation layer both need hardware.
//! Deciding *what a mark means* does not — and it is the part where a mistake
//! downloads the wrong paper. So it lives here, with no I/O, and is tested
//! exhaustively without a device.
//!
//! # The one thing to understand
//!
//! A mark is only an instruction when it lands, unambiguously, inside exactly
//! one box on the current generation of the form. Everything else — a stray
//! stroke, a mark on last week's copy, a line crossing two rows — produces a
//! reason for ignoring it, never a guess.

use crate::annotation::BoundingBox;
use crate::ids::DocumentId;
use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// Identifies one rendering of the form.
///
/// The device may still hold an older copy. Without this, regenerating the
/// index would re-fire every request the user ever made.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FormGeneration(String);

impl FormGeneration {
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

impl Default for FormGeneration {
    fn default() -> Self {
        Self::new()
    }
}

/// What ticking a box asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormAction {
    /// The Phase 3 path: fetch one attachment from Zotero onto the device.
    DownloadToDevice,
    /// Remove a document Marginalia itself put here.
    RemoveFromDevice,
    /// Push this document's annotations to Zotero.
    ExportAnnotations,
}

/// One row: a document, an action, and the box the user can tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormEntry {
    /// The document this row is *about* — not the form itself.
    pub target: DocumentId,
    pub action: FormAction,
    /// 1-based page of the form document.
    pub page: u32,
    /// The tick box, in PDF user space.
    pub tick_box: BoundingBox,
}

/// A generated index document, and the requests it can express.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestForm {
    pub generation: FormGeneration,
    /// The generated document itself, so a mark can be traced back to it.
    pub form_document: DocumentId,
    pub created_at: Timestamp,
    pub entries: Vec<FormEntry>,
}

/// A pen stroke's bounding box, as read from the annotation layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub page: u32,
    pub bounds: BoundingBox,
}

/// A request the user made by ticking a box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormRequest {
    pub target: DocumentId,
    pub action: FormAction,
    pub generation: FormGeneration,
    /// Index of the entry within the form. With the generation, this is what
    /// makes a request identifiable and therefore repeatable-safe.
    pub entry_index: usize,
}

impl FormRequest {
    /// Stable key for the sync journal's uniqueness constraint.
    ///
    /// Re-reading the same form yields the same key, so a second read cannot
    /// produce a second download.
    pub fn idempotency_key(&self) -> String {
        format!(
            "form:{}:{}:{:?}",
            self.generation.as_str(),
            self.entry_index,
            self.action
        )
    }
}

/// Why a mark was not treated as an instruction.
///
/// Every ignored mark has a reason, and the reasons are values rather than
/// silence — a user who ticked a box and saw nothing happen deserves to be
/// told which of these applied.
///
/// Not `Eq`: one variant carries a coverage fraction, and float equality is
/// not the comparison anyone wants here.
#[derive(Debug, Clone, PartialEq)]
pub enum IgnoredMark {
    /// The mark is on a page with no boxes, or misses every box.
    NoBoxAtThatPosition,
    /// It touched a box, but barely — a line passing through on its way
    /// elsewhere, not a tick.
    TooLittleCoverage { covered_fraction: f64 },
    /// It touched more than one box. Never guessed.
    Ambiguous { box_count: usize },
    /// The mark is on an older copy of the form.
    StaleGeneration,
}

/// The outcome of reading one mark.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkVerdict {
    Requested(FormRequest),
    Ignored(IgnoredMark),
}

/// Fraction of a tick box a mark must cover to count.
///
/// Chosen to accept a casual tick or scribble while rejecting a stroke that
/// merely crosses the box. It is deliberately not tiny: the cost of a false
/// positive is downloading a paper the user did not ask for, and the cost of a
/// false negative is ticking again.
pub const MIN_COVERAGE: f64 = 0.15;

impl RequestForm {
    pub fn new(form_document: DocumentId, created_at: Timestamp, entries: Vec<FormEntry>) -> Self {
        Self {
            generation: FormGeneration::new(),
            form_document,
            created_at,
            entries,
        }
    }

    /// Interpret one mark.
    ///
    /// `mark_generation` is the generation of the document the mark was found
    /// on. It is passed in rather than assumed, because the device may hold an
    /// older copy of the form and a mark on that copy must not act.
    pub fn interpret(&self, mark: &Mark, mark_generation: &FormGeneration) -> MarkVerdict {
        if mark_generation != &self.generation {
            return MarkVerdict::Ignored(IgnoredMark::StaleGeneration);
        }

        let touched: Vec<(usize, f64)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.page == mark.page)
            .filter_map(|(i, e)| {
                let covered = coverage(&e.tick_box, &mark.bounds);
                (covered > 0.0).then_some((i, covered))
            })
            .collect();

        match touched.len() {
            0 => MarkVerdict::Ignored(IgnoredMark::NoBoxAtThatPosition),
            1 => {
                let (index, covered) = touched[0];
                if covered < MIN_COVERAGE {
                    return MarkVerdict::Ignored(IgnoredMark::TooLittleCoverage {
                        covered_fraction: covered,
                    });
                }
                let entry = &self.entries[index];
                MarkVerdict::Requested(FormRequest {
                    target: entry.target.clone(),
                    action: entry.action,
                    generation: self.generation.clone(),
                    entry_index: index,
                })
            }
            // Two boxes touched. A stroke that spans rows is not a choice
            // between them, and picking the larger overlap would be inventing
            // an intention.
            n => MarkVerdict::Ignored(IgnoredMark::Ambiguous { box_count: n }),
        }
    }

    /// Interpret every mark found on the form, keeping only real requests and
    /// removing duplicates.
    ///
    /// Two marks in the same box — a tick and a second, heavier tick because
    /// nothing appeared to happen — are one request, not two.
    pub fn interpret_all(
        &self,
        marks: &[Mark],
        mark_generation: &FormGeneration,
    ) -> Vec<FormRequest> {
        let mut out: Vec<FormRequest> = Vec::new();
        for mark in marks {
            if let MarkVerdict::Requested(req) = self.interpret(mark, mark_generation) {
                if !out.iter().any(|r| r.entry_index == req.entry_index) {
                    out.push(req);
                }
            }
        }
        out
    }
}

/// Fraction of `target` covered by `mark`.
fn coverage(target: &BoundingBox, mark: &BoundingBox) -> f64 {
    let x_overlap = (target.x + target.width).min(mark.x + mark.width) - target.x.max(mark.x);
    let y_overlap = (target.y + target.height).min(mark.y + mark.height) - target.y.max(mark.y);

    if x_overlap <= 0.0 || y_overlap <= 0.0 {
        return 0.0;
    }
    let area = target.area();
    if area <= 0.0 {
        return 0.0;
    }
    ((x_overlap * y_overlap) / area).min(1.0)
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

    /// Two rows, boxes at y=700 and y=650, each 14pt square at x=60.
    fn two_row_form() -> (RequestForm, DocumentId, DocumentId) {
        let paper_a = DocumentId::new();
        let paper_b = DocumentId::new();
        let form = RequestForm::new(
            DocumentId::new(),
            Utc::now(),
            vec![
                FormEntry {
                    target: paper_a.clone(),
                    action: FormAction::DownloadToDevice,
                    page: 1,
                    tick_box: bbox(60.0, 700.0, 14.0, 14.0),
                },
                FormEntry {
                    target: paper_b.clone(),
                    action: FormAction::DownloadToDevice,
                    page: 1,
                    tick_box: bbox(60.0, 650.0, 14.0, 14.0),
                },
            ],
        );
        (form, paper_a, paper_b)
    }

    #[test]
    fn a_tick_in_a_box_is_a_request_for_that_paper() {
        let (form, paper_a, _) = two_row_form();
        let tick = Mark {
            page: 1,
            bounds: bbox(62.0, 702.0, 10.0, 10.0),
        };

        match form.interpret(&tick, &form.generation) {
            MarkVerdict::Requested(req) => {
                assert_eq!(req.target, paper_a);
                assert_eq!(req.action, FormAction::DownloadToDevice);
                assert_eq!(req.entry_index, 0);
            }
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn the_second_row_maps_to_the_second_paper() {
        // The failure this guards against is an off-by-one that downloads the
        // paper next to the one the user asked for.
        let (form, _, paper_b) = two_row_form();
        let tick = Mark {
            page: 1,
            bounds: bbox(62.0, 652.0, 10.0, 10.0),
        };

        match form.interpret(&tick, &form.generation) {
            MarkVerdict::Requested(req) => assert_eq!(req.target, paper_b),
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn a_stroke_crossing_two_boxes_is_never_guessed() {
        let (form, _, _) = two_row_form();
        // A vertical line down the margin, through both boxes.
        let stroke = Mark {
            page: 1,
            bounds: bbox(62.0, 640.0, 4.0, 90.0),
        };

        assert_eq!(
            form.interpret(&stroke, &form.generation),
            MarkVerdict::Ignored(IgnoredMark::Ambiguous { box_count: 2 }),
            "picking the larger overlap would be inventing an intention"
        );
    }

    #[test]
    fn a_line_merely_passing_through_is_not_a_tick() {
        let (form, _, _) = two_row_form();
        // A thin horizontal stroke clipping the bottom edge of the first box.
        let passing = Mark {
            page: 1,
            bounds: bbox(58.0, 700.0, 40.0, 1.0),
        };

        match form.interpret(&passing, &form.generation) {
            MarkVerdict::Ignored(IgnoredMark::TooLittleCoverage { covered_fraction }) => {
                assert!(covered_fraction < MIN_COVERAGE);
            }
            other => panic!("expected TooLittleCoverage, got {other:?}"),
        }
    }

    #[test]
    fn a_mark_elsewhere_on_the_page_is_ignored() {
        let (form, _, _) = two_row_form();
        let doodle = Mark {
            page: 1,
            bounds: bbox(300.0, 400.0, 30.0, 30.0),
        };
        assert_eq!(
            form.interpret(&doodle, &form.generation),
            MarkVerdict::Ignored(IgnoredMark::NoBoxAtThatPosition)
        );
    }

    #[test]
    fn a_mark_on_another_page_is_ignored() {
        let (form, _, _) = two_row_form();
        let same_position_wrong_page = Mark {
            page: 2,
            bounds: bbox(62.0, 702.0, 10.0, 10.0),
        };
        assert_eq!(
            form.interpret(&same_position_wrong_page, &form.generation),
            MarkVerdict::Ignored(IgnoredMark::NoBoxAtThatPosition)
        );
    }

    /// The rule that stops regeneration re-firing history.
    #[test]
    fn a_mark_on_an_old_copy_of_the_form_does_nothing() {
        let (form, _, _) = two_row_form();
        let tick = Mark {
            page: 1,
            bounds: bbox(62.0, 702.0, 10.0, 10.0),
        };
        let last_weeks_copy = FormGeneration::from_string("an-older-generation");

        assert_eq!(
            form.interpret(&tick, &last_weeks_copy),
            MarkVerdict::Ignored(IgnoredMark::StaleGeneration),
            "without this, regenerating the index would re-download everything \
             the user ever asked for"
        );
    }

    #[test]
    fn ticking_the_same_box_twice_is_one_request() {
        // The realistic case: nothing appears to happen, so the user ticks
        // again, harder.
        let (form, paper_a, _) = two_row_form();
        let marks = vec![
            Mark {
                page: 1,
                bounds: bbox(62.0, 702.0, 8.0, 8.0),
            },
            Mark {
                page: 1,
                bounds: bbox(61.0, 701.0, 12.0, 12.0),
            },
        ];

        let requests = form.interpret_all(&marks, &form.generation);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].target, paper_a);
    }

    #[test]
    fn two_different_boxes_are_two_requests() {
        let (form, paper_a, paper_b) = two_row_form();
        let marks = vec![
            Mark {
                page: 1,
                bounds: bbox(62.0, 702.0, 10.0, 10.0),
            },
            Mark {
                page: 1,
                bounds: bbox(62.0, 652.0, 10.0, 10.0),
            },
        ];

        let requests = form.interpret_all(&marks, &form.generation);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].target, paper_a);
        assert_eq!(requests[1].target, paper_b);
    }

    #[test]
    fn reading_the_form_twice_produces_the_same_idempotency_key() {
        // What makes a second read a no-op rather than a second download.
        let (form, _, _) = two_row_form();
        let tick = Mark {
            page: 1,
            bounds: bbox(62.0, 702.0, 10.0, 10.0),
        };

        let first = form.interpret_all(std::slice::from_ref(&tick), &form.generation);
        let second = form.interpret_all(std::slice::from_ref(&tick), &form.generation);

        assert_eq!(
            first[0].idempotency_key(),
            second[0].idempotency_key(),
            "the journal's uniqueness constraint depends on this"
        );
    }

    #[test]
    fn different_entries_have_different_keys() {
        let (form, _, _) = two_row_form();
        let marks = vec![
            Mark {
                page: 1,
                bounds: bbox(62.0, 702.0, 10.0, 10.0),
            },
            Mark {
                page: 1,
                bounds: bbox(62.0, 652.0, 10.0, 10.0),
            },
        ];
        let requests = form.interpret_all(&marks, &form.generation);
        assert_ne!(requests[0].idempotency_key(), requests[1].idempotency_key());
    }

    #[test]
    fn two_generations_of_the_same_row_have_different_keys() {
        // A user who asks again next week must get a second download, not a
        // deduplicated no-op.
        let (form_a, _, _) = two_row_form();
        let (form_b, _, _) = two_row_form();
        let tick = Mark {
            page: 1,
            bounds: bbox(62.0, 702.0, 10.0, 10.0),
        };

        let a = form_a.interpret_all(std::slice::from_ref(&tick), &form_a.generation);
        let b = form_b.interpret_all(std::slice::from_ref(&tick), &form_b.generation);
        assert_ne!(a[0].idempotency_key(), b[0].idempotency_key());
    }

    #[test]
    fn an_empty_form_can_express_nothing() {
        let form = RequestForm::new(DocumentId::new(), Utc::now(), vec![]);
        let tick = Mark {
            page: 1,
            bounds: bbox(62.0, 702.0, 10.0, 10.0),
        };
        assert_eq!(
            form.interpret(&tick, &form.generation),
            MarkVerdict::Ignored(IgnoredMark::NoBoxAtThatPosition)
        );
    }

    #[test]
    fn a_mark_completely_covering_a_box_counts() {
        // A scribble rather than a neat tick.
        let (form, paper_a, _) = two_row_form();
        let scribble = Mark {
            page: 1,
            bounds: bbox(55.0, 695.0, 26.0, 26.0),
        };
        match form.interpret(&scribble, &form.generation) {
            MarkVerdict::Requested(req) => assert_eq!(req.target, paper_a),
            other => panic!("a scribble over the box is a tick, got {other:?}"),
        }
    }

    #[test]
    fn coverage_is_bounded_and_sane() {
        let target = bbox(0.0, 0.0, 10.0, 10.0);
        assert_eq!(coverage(&target, &bbox(0.0, 0.0, 10.0, 10.0)), 1.0);
        assert_eq!(coverage(&target, &bbox(0.0, 0.0, 5.0, 10.0)), 0.5);
        assert_eq!(coverage(&target, &bbox(100.0, 100.0, 5.0, 5.0)), 0.0);
        // A mark far larger than the box still saturates at 1.0 rather than
        // reporting a nonsensical fraction.
        assert_eq!(coverage(&target, &bbox(-50.0, -50.0, 200.0, 200.0)), 1.0);
    }

    #[test]
    fn a_degenerate_box_cannot_be_ticked() {
        // Defensive: a zero-area box from a layout bug must not divide by zero
        // or accept every mark.
        let form = RequestForm::new(
            DocumentId::new(),
            Utc::now(),
            vec![FormEntry {
                target: DocumentId::new(),
                action: FormAction::DownloadToDevice,
                page: 1,
                tick_box: bbox(60.0, 700.0, 0.0, 0.0),
            }],
        );
        let tick = Mark {
            page: 1,
            bounds: bbox(55.0, 695.0, 20.0, 20.0),
        };
        assert!(matches!(
            form.interpret(&tick, &form.generation),
            MarkVerdict::Ignored(_)
        ));
    }

    #[test]
    fn nothing_but_a_mark_in_a_box_is_an_instruction() {
        // Reading, opening, or highlighting a paper must never be read as a
        // request. This asserts the only public entry point requires a Mark
        // that lands in a box: there is no other constructor for FormRequest.
        let (form, _, _) = two_row_form();
        let highlight_elsewhere = Mark {
            page: 1,
            bounds: bbox(120.0, 700.0, 200.0, 12.0),
        };
        assert!(matches!(
            form.interpret(&highlight_elsewhere, &form.generation),
            MarkVerdict::Ignored(_)
        ));
    }
}
