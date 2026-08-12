//! Turning a document's `.content` file into page numbers.
//!
//! A highlight file is named after a page's uuid, which is meaningless to a
//! reader. `.content` holds the order those uuids appear in, so it is what
//! turns `5088a025-…` into "page 2".
//!
//! Two layouts exist on a single device, and both are in daily use -- 450
//! documents at `formatVersion` 1 and 85 at 2 on the machine this was written
//! against:
//!
//! ```text
//! v1:  "pages": ["uuid", "uuid", …]
//! v2:  "cPages": { "pages": [ {"id": "uuid", "idx": …, "redir": …}, … ] }
//! ```
//!
//! Anything else yields [`PageOrderProblem::UnsupportedFormatVersion`] and no
//! page numbers at all. That is the honest outcome: highlights still carry
//! their text, and nothing claims a page it cannot justify.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Why a document's pages could not be numbered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageOrderProblem {
    /// The `.content` file was absent or unreadable.
    ContentUnreadable,
    /// The file did not parse as JSON.
    ContentMalformed,
    /// A `formatVersion` this crate has not been verified against.
    UnsupportedFormatVersion(i64),
    /// A recognised version whose page list was missing or the wrong shape.
    PageListMissing,
}

impl std::fmt::Display for PageOrderProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContentUnreadable => write!(f, "its .content file could not be read"),
            Self::ContentMalformed => write!(f, "its .content file is not valid JSON"),
            Self::UnsupportedFormatVersion(v) => {
                write!(f, "it uses .content formatVersion {v}, which Marginalia has not been verified against")
            }
            Self::PageListMissing => write!(f, "its .content file lists no pages"),
        }
    }
}

/// Page uuid to 1-based position.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageOrder {
    positions: HashMap<String, u32>,
    /// The document's own `fileType`, carried along because the caller wants it
    /// and this is the file that has it.
    pub file_type: String,
}

impl PageOrder {
    /// The 1-based page number for a page uuid, if the document lists it.
    pub fn number_of(&self, page_id: &str) -> Option<u32> {
        self.positions.get(page_id).copied()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

#[derive(Deserialize)]
struct ContentFile {
    #[serde(default)]
    format_version: Option<i64>,
    #[serde(default)]
    file_type: Option<String>,
    /// formatVersion 1.
    #[serde(default)]
    pages: Option<Vec<String>>,
    /// formatVersion 2.
    #[serde(default)]
    c_pages: Option<CPages>,
}

#[derive(Deserialize)]
struct CPages {
    #[serde(default)]
    pages: Vec<CPage>,
}

#[derive(Deserialize)]
struct CPage {
    id: String,
}

/// Read page order out of a `.content` file's contents.
pub(crate) fn parse(source: &str) -> Result<PageOrder, PageOrderProblem> {
    // serde's rename_all would be the obvious tool, but `.content` mixes
    // camelCase keys with a nested object whose own keys are short and unrelated
    // ("id", "idx", "redir"). Naming the fields explicitly keeps the mapping
    // visible at the point where a future format change will break it.
    let raw: serde_json::Value =
        serde_json::from_str(source).map_err(|_| PageOrderProblem::ContentMalformed)?;

    let content = ContentFile {
        format_version: raw.get("formatVersion").and_then(|v| v.as_i64()),
        file_type: raw
            .get("fileType")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        pages: raw.get("pages").and_then(|v| {
            v.as_array()?
                .iter()
                .map(|p| p.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        }),
        c_pages: raw
            .get("cPages")
            .and_then(|v| serde_json::from_value::<CPages>(v.clone()).ok()),
    };

    let file_type = content.file_type.unwrap_or_default();

    // A missing formatVersion is treated as 1: the oldest documents predate the
    // field, and every one observed without it used the v1 layout.
    let ids: Vec<String> = match content.format_version.unwrap_or(1) {
        1 => content.pages.ok_or(PageOrderProblem::PageListMissing)?,
        2 => content
            .c_pages
            .map(|c| c.pages.into_iter().map(|p| p.id).collect())
            .ok_or(PageOrderProblem::PageListMissing)?,
        other => return Err(PageOrderProblem::UnsupportedFormatVersion(other)),
    };

    if ids.is_empty() {
        return Err(PageOrderProblem::PageListMissing);
    }

    let positions = ids
        .into_iter()
        .enumerate()
        .map(|(index, id)| (id, index as u32 + 1))
        .collect();

    Ok(PageOrder {
        positions,
        file_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The v1 layout, as observed: a flat ordered array of uuids.
    const V1: &str = r#"{
        "formatVersion": 1,
        "fileType": "pdf",
        "pageCount": 19,
        "pages": [
            "f414ce0d-e519-4afa-811c-8d32e2ce4cb7",
            "5088a025-3a80-45e4-8e0e-6285759990f9",
            "c467e272-1747-4f5e-99c6-2ef47f0cc6b4"
        ]
    }"#;

    /// The v2 layout, as observed: objects under cPages, carrying the ordering
    /// index and redirection fields this crate does not need.
    const V2: &str = r#"{
        "formatVersion": 2,
        "fileType": "epub",
        "cPages": {
            "lastOpened": {"timestamp": "1:19", "value": "4fd611e3-4ebd-4c6d-84d7-3c9ccca28ec4"},
            "original": {"timestamp": "1:1", "value": 466},
            "pages": [
                {"id": "bd45c4e4-50e9-461c-8a13-6ac2c87f43d5",
                 "idx": {"timestamp": "1:1", "value": "ba"},
                 "redir": {"timestamp": "1:1", "value": 0}},
                {"id": "aa11c4e4-50e9-461c-8a13-6ac2c87f43d5",
                 "idx": {"timestamp": "1:2", "value": "bb"},
                 "redir": {"timestamp": "1:2", "value": 1}}
            ]
        }
    }"#;

    #[test]
    fn version_one_numbers_pages_from_the_flat_array() {
        let order = parse(V1).unwrap();
        assert_eq!(
            order.number_of("f414ce0d-e519-4afa-811c-8d32e2ce4cb7"),
            Some(1)
        );
        assert_eq!(
            order.number_of("5088a025-3a80-45e4-8e0e-6285759990f9"),
            Some(2)
        );
        assert_eq!(order.file_type, "pdf");
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn version_two_numbers_pages_from_the_objects_under_cpages() {
        let order = parse(V2).unwrap();
        assert_eq!(
            order.number_of("bd45c4e4-50e9-461c-8a13-6ac2c87f43d5"),
            Some(1)
        );
        assert_eq!(
            order.number_of("aa11c4e4-50e9-461c-8a13-6ac2c87f43d5"),
            Some(2)
        );
        assert_eq!(order.file_type, "epub");
    }

    #[test]
    fn a_page_the_document_does_not_list_has_no_number() {
        assert_eq!(
            parse(V1).unwrap().number_of("not-a-page-of-this-document"),
            None
        );
    }

    /// The refusal that matters. A future layout must not be numbered by
    /// falling back to whichever key happens to still be present -- that is how
    /// a quotation gets attributed to the wrong page.
    #[test]
    fn an_unverified_format_version_is_refused_rather_than_guessed() {
        let v3 = r#"{"formatVersion": 3, "fileType": "pdf", "pages": ["a", "b"]}"#;
        assert_eq!(
            parse(v3),
            Err(PageOrderProblem::UnsupportedFormatVersion(3))
        );
    }

    /// Documents predating the field used the v1 layout, so absence means 1.
    #[test]
    fn a_missing_format_version_reads_as_the_oldest_layout() {
        let order = parse(r#"{"fileType": "pdf", "pages": ["a", "b"]}"#).unwrap();
        assert_eq!(order.number_of("b"), Some(2));
    }

    #[test]
    fn a_recognised_version_with_no_pages_says_so() {
        assert_eq!(
            parse(r#"{"formatVersion": 1, "fileType": "pdf"}"#),
            Err(PageOrderProblem::PageListMissing)
        );
        assert_eq!(
            parse(r#"{"formatVersion": 2, "cPages": {"pages": []}}"#),
            Err(PageOrderProblem::PageListMissing)
        );
    }

    #[test]
    fn malformed_content_is_reported_as_malformed() {
        assert_eq!(parse("{ not json"), Err(PageOrderProblem::ContentMalformed));
    }

    /// The message reaches a person, so it has to read like one wrote it.
    #[test]
    fn every_problem_explains_itself_in_a_sentence() {
        assert!(PageOrderProblem::UnsupportedFormatVersion(3)
            .to_string()
            .contains("formatVersion 3"));
        assert!(PageOrderProblem::ContentUnreadable
            .to_string()
            .contains("could not be read"));
    }
}
