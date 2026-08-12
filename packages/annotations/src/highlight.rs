//! The shape of a `.highlights/<page-uuid>.json` file.
//!
//! Modelled from files on a reMarkable 2 running 3.28.0.166. Every field here
//! was observed; none was inferred from documentation, because there is none.

use serde::{Deserialize, Serialize};

/// One highlighted passage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Highlight {
    /// The highlighted text, as the device recorded it.
    pub text: String,

    /// The highlighter colour, as the raw integer the device stores, when it
    /// stored one at all.
    ///
    /// Optional because a device says so. Of 26 highlighted documents on the
    /// reMarkable this was verified against, 10 had files with no `color` key
    /// whatsoever -- older highlights, made before the colour highlighters
    /// existed. Requiring the field cost those 10 documents entirely, which is
    /// how this was found: the extractor reported them as unreadable instead of
    /// dropping them silently.
    ///
    /// Deliberately not mapped to a colour name. Only `1` has been observed,
    /// and inventing a palette from one sample is how a yellow highlight ends
    /// up labelled green in someone's notes.
    #[serde(default)]
    pub color: Option<i64>,

    /// Offset of the passage in the page's extracted text.
    #[serde(default)]
    pub start: i64,

    /// Length of the passage in the page's extracted text.
    #[serde(default)]
    pub length: i64,

    /// Where the highlight sits on the page. Usually one box; more when the
    /// passage wraps across lines.
    #[serde(default)]
    pub rects: Vec<Rect>,
}

/// A box on the page, in the device's own coordinate space.
///
/// Not converted to PDF points: the transform has not been verified on
/// hardware, and an unverified conversion is a wrong number wearing a
/// convincing unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The file's top level: highlights grouped into layers.
///
/// The device writes an array of arrays. What separates one inner array from
/// the next has not been established, so they are flattened in order rather
/// than given a meaning they may not have.
#[derive(Debug, Clone, Deserialize)]
struct HighlightFile {
    #[serde(default)]
    highlights: Vec<Vec<Highlight>>,
}

/// Every highlight on one page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageHighlights {
    /// The page's uuid, which is also the JSON file's stem.
    pub page_id: String,

    /// 1-based page number, or [`None`] when the `.content` layout could not
    /// be read. See [`crate::PageOrderProblem`].
    pub page_number: Option<u32>,

    pub highlights: Vec<Highlight>,
}

/// Every highlight in one document.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentHighlights {
    pub uuid: String,

    /// The name as it appears on the device.
    pub name: String,

    /// `"pdf"`, `"epub"`, or whatever else the device recorded.
    pub file_type: String,

    /// Pages carrying at least one highlight, in reading order. Pages whose
    /// number is unknown sort last, since there is nothing to order them by.
    pub pages: Vec<PageHighlights>,

    /// Why page numbers are missing, when they are.
    pub page_order_problem: Option<crate::PageOrderProblem>,
}

impl DocumentHighlights {
    /// Total highlights across every page.
    pub fn count(&self) -> usize {
        self.pages.iter().map(|p| p.highlights.len()).sum()
    }
}

/// Parse one `.highlights/<page-uuid>.json` file's contents.
pub(crate) fn parse_page(source: &str) -> Result<Vec<Highlight>, serde_json::Error> {
    let file: HighlightFile = serde_json::from_str(source)?;
    Ok(file.highlights.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped exactly like a file read off a device, down to the array of
    /// arrays and the float precision.
    const REAL_SHAPE: &str = r#"{
        "highlights": [
            [
                {
                    "color": 1,
                    "length": 52,
                    "rects": [
                        {
                            "height": 24.165608694943558,
                            "width": 529.15594405788,
                            "x": 758.5598480273736,
                            "y": 1121.9896698174584
                        }
                    ],
                    "start": 4672,
                    "text": "the passage someone marked"
                },
                {
                    "color": 1,
                    "length": 35,
                    "rects": [
                        {"height": 24.1, "width": 344.4, "x": 758.8, "y": 1151.3}
                    ],
                    "start": 4726,
                    "text": "and the next one"
                }
            ]
        ]
    }"#;

    #[test]
    fn a_file_in_the_shape_a_device_writes_yields_its_text() {
        let highlights = parse_page(REAL_SHAPE).unwrap();
        assert_eq!(highlights.len(), 2);
        assert_eq!(highlights[0].text, "the passage someone marked");
        assert_eq!(highlights[0].color, Some(1));
        assert_eq!(highlights[0].start, 4672);
        assert_eq!(highlights[0].length, 52);
        assert_eq!(highlights[1].text, "and the next one");
    }

    /// The device writes coordinates to ~17 significant digits. Rounding them
    /// to something tidier would make every box subtly wrong, so the check is
    /// that the fraction survives -- not that two f64 literals are bit-equal,
    /// which compares JSON's float parser against rustc's and tests neither.
    #[test]
    fn the_boxes_survive_with_their_precision() {
        let highlights = parse_page(REAL_SHAPE).unwrap();
        let rect = highlights[0].rects[0];
        assert!((rect.x - 758.5598480273736).abs() < 1e-9);
        assert!((rect.height - 24.165608694943558).abs() < 1e-9);
        assert!((rect.width - 529.15594405788).abs() < 1e-9);

        // Not silently truncated to whole units or two decimals.
        assert_ne!(rect.x, rect.x.trunc());
        assert!((rect.x * 1e6).fract() != 0.0);
    }

    /// Layers are flattened in order. If they later turn out to mean something,
    /// this test is where the change announces itself.
    #[test]
    fn several_layers_flatten_in_order() {
        let source = r#"{"highlights": [
            [{"color": 1, "text": "first"}],
            [{"color": 2, "text": "second"}]
        ]}"#;
        let highlights = parse_page(source).unwrap();
        assert_eq!(
            highlights
                .iter()
                .map(|h| h.text.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn a_page_with_no_highlights_is_empty_rather_than_an_error() {
        assert!(parse_page(r#"{"highlights": []}"#).unwrap().is_empty());
        assert!(parse_page("{}").unwrap().is_empty());
    }

    /// A highlight missing its optional geometry still yields its text. The
    /// text is the point; the boxes are decoration.
    #[test]
    fn missing_geometry_does_not_lose_the_quotation() {
        let highlights = parse_page(r#"{"highlights": [[{"color": 1, "text": "kept"}]]}"#).unwrap();
        assert_eq!(highlights[0].text, "kept");
        assert!(highlights[0].rects.is_empty());
        assert_eq!(highlights[0].start, 0);
    }

    /// Shaped like the files that broke the first version of this parser: real
    /// highlights, on a real device, with no `color` key at all.
    #[test]
    fn a_highlight_the_device_stored_without_a_colour_is_still_read() {
        let source = r#"{
            "highlights": [
                [
                    {
                        "length": 33,
                        "rects": [
                            {
                                "height": 43.31959802964167,
                                "width": 447.66536824843456,
                                "x": 121.50176575604607,
                                "y": 469.8175910500918
                            }
                        ],
                        "start": 87,
                        "text": "an older highlight"
                    }
                ]
            ]
        }"#;

        let highlights = parse_page(source).unwrap();
        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].text, "an older highlight");
        assert_eq!(highlights[0].color, None);
        assert_eq!(highlights[0].start, 87);
    }

    /// A colourless highlight and a colour-1 highlight can sit in one file.
    #[test]
    fn colourless_and_coloured_highlights_coexist() {
        let source = r#"{"highlights": [[
            {"text": "old"},
            {"color": 1, "text": "new"}
        ]]}"#;
        let highlights = parse_page(source).unwrap();
        assert_eq!(highlights[0].color, None);
        assert_eq!(highlights[1].color, Some(1));
    }

    #[test]
    fn malformed_json_is_an_error_not_an_empty_page() {
        assert!(parse_page("{ not json").is_err());
    }

    /// An empty page must be distinguishable from a page that failed to parse,
    /// or a corrupt file reads as "you highlighted nothing".
    #[test]
    fn a_broken_file_never_masquerades_as_a_page_without_highlights() {
        let broken = parse_page(r#"{"highlights": "not an array"}"#);
        assert!(broken.is_err());
    }
}
