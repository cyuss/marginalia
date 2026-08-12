//! Reading the highlights a reMarkable has already stored.
//!
//! # Why this crate is small
//!
//! For most of this project's life, extracting highlighted text looked like it
//! would need a geometry engine: take stroke paths out of the `.rm` format,
//! intersect them with a PDF's text layer, and infer which words a person meant
//! to mark. [`OPEN_QUESTIONS.md` U3] carried that risk, and the desk research
//! made it worse -- the best-known extractor does not support firmware 3.x, and
//! community tools had fallen back to rasterising pages and looking for
//! highlighter-coloured pixels.
//!
//! Hardware said otherwise. On a reMarkable 2 running 3.28.0.166, the text is
//! simply there, in its own file beside the document:
//!
//! ```text
//! <uuid>.highlights/<page-uuid>.json
//! ```
//!
//! So this crate reads JSON. That is the whole trick.
//!
//! # What it will not do
//!
//! It never opens a file for writing. It is given a path to the reMarkable's
//! own document store -- the most precious directory on the device -- and its
//! entire job is to look. There is no code path here that creates, moves,
//! truncates or deletes anything, and [`crate::extract`] takes `&Path` rather
//! than any handle that could be written through.
//!
//! # What it refuses to guess
//!
//! Page numbers come from the document's `.content` file, whose layout changed
//! between `formatVersion` 1 and 2. Both are handled. A third version, when it
//! arrives, will not be: the highlighted *text* is still returned, because that
//! part is unambiguous, but the page number becomes [`None`] with a recorded
//! reason rather than a plausible-looking lie. A quotation attributed to the
//! wrong page is worse than one attributed to no page.
//!
//! [`OPEN_QUESTIONS.md` U3]: ../../../docs/development/OPEN_QUESTIONS.md

mod content;
mod extract;
mod highlight;

pub use content::{PageOrder, PageOrderProblem};
pub use extract::{extract, extract_one, ExtractError, Library, DEFAULT_STORE};
pub use highlight::{DocumentHighlights, Highlight, PageHighlights, Rect};

/// Bumped whenever this crate would produce different output from the same
/// files on disk.
///
/// Stored alongside anything extracted so that a format correction can be
/// re-run against the device rather than silently disagreeing with rows
/// extracted by an older build. Extraction is a derived view; the device's
/// files remain the only source of truth.
pub const EXTRACTION_VERSION: u32 = 1;

/// The firmware this crate's understanding of the format was verified against.
///
/// Not a gate -- nothing here refuses to run on another release -- but a fact
/// worth carrying next to extracted data, because "which firmware wrote this"
/// is the first question to ask when an extraction looks wrong.
pub const VERIFIED_AGAINST_FIRMWARE: &str = "3.28.0.166";
