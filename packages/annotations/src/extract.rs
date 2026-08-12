//! Walking the reMarkable's document store and reading what is there.
//!
//! Every function in this module takes a path and returns data. None of them
//! creates, opens-for-writing, moves or removes anything. The directory this
//! reads is where a person's entire library lives; the only safe relationship
//! with it is to look.

use crate::content::{self, PageOrder, PageOrderProblem};
use crate::highlight::{self, DocumentHighlights, PageHighlights};
use std::path::{Path, PathBuf};

/// Where xochitl keeps documents on a reMarkable.
///
/// Public because the agent needs it and because naming it once, here, beats
/// each caller spelling the path out.
pub const DEFAULT_STORE: &str = "/home/root/.local/share/remarkable/xochitl"; // guard-allow: the path is read, never written; naming the directory is not touching the application that owns it

#[derive(Debug)]
pub enum ExtractError {
    /// The store directory is absent or cannot be listed.
    StoreUnreadable { path: PathBuf, cause: String },
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreUnreadable { path, cause } => {
                write!(f, "could not read {}: {cause}", path.display())
            }
        }
    }
}

impl std::error::Error for ExtractError {}

/// Everything with highlights, plus what could not be read.
#[derive(Debug, Default)]
pub struct Library {
    /// Documents carrying at least one highlight, newest reading first.
    pub documents: Vec<DocumentHighlights>,

    /// Files that looked like highlights but did not parse, with the reason.
    /// Surfaced rather than swallowed: a document silently missing from a
    /// review is worse than one listed as unreadable.
    pub unreadable: Vec<(String, String)>,
}

impl Library {
    pub fn total_highlights(&self) -> usize {
        self.documents.iter().map(DocumentHighlights::count).sum()
    }
}

/// Read every highlighted document in a store directory.
pub fn extract(store: &Path) -> Result<Library, ExtractError> {
    let entries = std::fs::read_dir(store).map_err(|e| ExtractError::StoreUnreadable {
        path: store.to_path_buf(),
        cause: e.to_string(),
    })?;

    let mut library = Library::default();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("highlights") {
            continue;
        }
        let Some(uuid) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        match extract_one(store, uuid) {
            Ok(Some(document)) => library.documents.push(document),
            // A document with an empty highlights directory, or one the user
            // has deleted. Neither is a problem to report.
            Ok(None) => {}
            Err(reason) => library.unreadable.push((uuid.to_string(), reason)),
        }
    }

    // Alphabetical by name. Reading order would be better, but `lastOpened` is
    // a millisecond string whose meaning across firmware versions has not been
    // verified, and a wrong order presented confidently is worse than a dull
    // one that is right.
    library.documents.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(library)
}

/// Read one document's highlights, by uuid.
///
/// `Ok(None)` means the document exists but has nothing to show -- no
/// highlights, or the user deleted it. `Err` carries a sentence explaining what
/// could not be read.
pub fn extract_one(store: &Path, uuid: &str) -> Result<Option<DocumentHighlights>, String> {
    let metadata = read_metadata(store, uuid)?;
    if metadata.deleted {
        return Ok(None);
    }

    let (order, problem) = read_page_order(store, uuid);

    let highlights_dir = store.join(format!("{uuid}.highlights"));
    let Ok(files) = std::fs::read_dir(&highlights_dir) else {
        return Ok(None);
    };

    let mut pages = Vec::new();
    for file in files.flatten() {
        let path = file.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(page_id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;

        let highlights = highlight::parse_page(&source)
            .map_err(|e| format!("{} is not valid highlight JSON: {e}", path.display()))?;

        if highlights.is_empty() {
            continue;
        }

        pages.push(PageHighlights {
            page_number: order.as_ref().and_then(|o| o.number_of(page_id)),
            page_id: page_id.to_string(),
            highlights,
        });
    }

    if pages.is_empty() {
        return Ok(None);
    }

    // Numbered pages in order, then anything unnumbered -- which only happens
    // when the page order could not be read at all.
    pages.sort_by_key(|p| (p.page_number.is_none(), p.page_number.unwrap_or(0)));

    Ok(Some(DocumentHighlights {
        uuid: uuid.to_string(),
        name: metadata.visible_name,
        file_type: order.map(|o| o.file_type).unwrap_or_default(),
        pages,
        page_order_problem: problem,
    }))
}

fn read_page_order(store: &Path, uuid: &str) -> (Option<PageOrder>, Option<PageOrderProblem>) {
    let path = store.join(format!("{uuid}.content"));
    let Ok(source) = std::fs::read_to_string(&path) else {
        return (None, Some(PageOrderProblem::ContentUnreadable));
    };
    match content::parse(&source) {
        Ok(order) => (Some(order), None),
        Err(problem) => (None, Some(problem)),
    }
}

struct Metadata {
    visible_name: String,
    deleted: bool,
}

fn read_metadata(store: &Path, uuid: &str) -> Result<Metadata, String> {
    let path = store.join(format!("{uuid}.metadata"));
    let source = std::fs::read_to_string(&path)
        .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&source)
        .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;

    Ok(Metadata {
        // An unnamed document is possible; showing the uuid is more use than
        // showing nothing, and far more use than refusing to list it.
        visible_name: value
            .get("visibleName")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(uuid)
            .to_string(),
        deleted: value
            .get("deleted")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a store in a temporary directory, shaped like the real one.
    struct Store {
        root: PathBuf,
    }

    impl Store {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("marginalia-annotations-{tag}"));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn document(&self, uuid: &str, name: &str, deleted: bool) -> &Self {
            std::fs::write(
                self.root.join(format!("{uuid}.metadata")),
                format!(
                    r#"{{"visibleName": "{name}", "deleted": {deleted}, "type": "DocumentType"}}"#
                ),
            )
            .unwrap();
            self
        }

        fn content(&self, uuid: &str, body: &str) -> &Self {
            std::fs::write(self.root.join(format!("{uuid}.content")), body).unwrap();
            self
        }

        fn highlight_page(&self, uuid: &str, page_id: &str, body: &str) -> &Self {
            let dir = self.root.join(format!("{uuid}.highlights"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{page_id}.json")), body).unwrap();
            self
        }
    }

    impl Drop for Store {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn page_with(text: &str) -> String {
        format!(r#"{{"highlights": [[{{"color": 1, "text": "{text}"}}]]}}"#)
    }

    const V1_TWO_PAGES: &str = r#"{"formatVersion": 1, "fileType": "pdf",
        "pages": ["page-one", "page-two"]}"#;

    #[test]
    fn a_highlighted_document_is_read_with_its_name_and_page_numbers() {
        let store = Store::new("read");
        store
            .document("doc-a", "On Certainty", false)
            .content("doc-a", V1_TWO_PAGES)
            .highlight_page("doc-a", "page-two", &page_with("second page passage"))
            .highlight_page("doc-a", "page-one", &page_with("first page passage"));

        let library = extract(&store.root).unwrap();
        assert_eq!(library.documents.len(), 1);

        let doc = &library.documents[0];
        assert_eq!(doc.name, "On Certainty");
        assert_eq!(doc.file_type, "pdf");
        assert_eq!(doc.count(), 2);

        // Sorted by page, not by the order the filesystem happened to list them.
        assert_eq!(doc.pages[0].page_number, Some(1));
        assert_eq!(doc.pages[0].highlights[0].text, "first page passage");
        assert_eq!(doc.pages[1].page_number, Some(2));
    }

    #[test]
    fn a_deleted_document_is_not_reported() {
        let store = Store::new("deleted");
        store
            .document("doc-b", "Thrown Away", true)
            .content("doc-b", V1_TWO_PAGES)
            .highlight_page("doc-b", "page-one", &page_with("gone"));

        assert!(extract(&store.root).unwrap().documents.is_empty());
    }

    #[test]
    fn a_document_with_an_empty_highlight_file_is_not_reported() {
        let store = Store::new("empty");
        store
            .document("doc-c", "Unmarked", false)
            .content("doc-c", V1_TWO_PAGES)
            .highlight_page("doc-c", "page-one", r#"{"highlights": []}"#);

        assert!(extract(&store.root).unwrap().documents.is_empty());
    }

    /// The behaviour the whole crate is arranged around: an unreadable page
    /// order costs page numbers, never the text.
    #[test]
    fn an_unsupported_content_version_still_yields_the_text() {
        let store = Store::new("future");
        store
            .document("doc-d", "Next Firmware", false)
            .content("doc-d", r#"{"formatVersion": 99, "pages": ["page-one"]}"#)
            .highlight_page("doc-d", "page-one", &page_with("still recoverable"));

        let library = extract(&store.root).unwrap();
        let doc = &library.documents[0];
        assert_eq!(doc.pages[0].highlights[0].text, "still recoverable");
        assert_eq!(doc.pages[0].page_number, None);
        assert_eq!(
            doc.page_order_problem,
            Some(PageOrderProblem::UnsupportedFormatVersion(99))
        );
    }

    #[test]
    fn a_missing_content_file_is_reported_as_such_and_loses_only_the_numbers() {
        let store = Store::new("nocontent");
        store
            .document("doc-e", "No Content File", false)
            .highlight_page("doc-e", "page-one", &page_with("text survives"));

        let doc = &extract(&store.root).unwrap().documents[0];
        assert_eq!(doc.pages[0].highlights[0].text, "text survives");
        assert_eq!(
            doc.page_order_problem,
            Some(PageOrderProblem::ContentUnreadable)
        );
    }

    /// One corrupt document must not hide the rest of the library.
    #[test]
    fn a_broken_document_is_listed_as_unreadable_and_the_others_still_appear() {
        let store = Store::new("broken");
        store
            .document("doc-good", "Fine", false)
            .content("doc-good", V1_TWO_PAGES)
            .highlight_page("doc-good", "page-one", &page_with("intact"));
        store
            .document("doc-bad", "Corrupt", false)
            .content("doc-bad", V1_TWO_PAGES)
            .highlight_page("doc-bad", "page-one", "{ truncated");

        let library = extract(&store.root).unwrap();
        assert_eq!(library.documents.len(), 1);
        assert_eq!(library.documents[0].name, "Fine");
        assert_eq!(library.unreadable.len(), 1);
        assert_eq!(library.unreadable[0].0, "doc-bad");
    }

    #[test]
    fn an_unnamed_document_falls_back_to_its_uuid_rather_than_vanishing() {
        let store = Store::new("unnamed");
        store
            .document("doc-f", "", false)
            .content("doc-f", V1_TWO_PAGES)
            .highlight_page("doc-f", "page-one", &page_with("kept"));

        assert_eq!(extract(&store.root).unwrap().documents[0].name, "doc-f");
    }

    #[test]
    fn documents_are_listed_in_a_stable_order() {
        let store = Store::new("order");
        for (uuid, name) in [("d1", "Zettel"), ("d2", "Anarchy"), ("d3", "Method")] {
            store
                .document(uuid, name, false)
                .content(uuid, V1_TWO_PAGES)
                .highlight_page(uuid, "page-one", &page_with("x"));
        }

        let library = extract(&store.root).unwrap();
        let names: Vec<_> = library.documents.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Anarchy", "Method", "Zettel"]);
        assert_eq!(library.total_highlights(), 3);
    }

    #[test]
    fn a_store_that_does_not_exist_is_an_error_rather_than_an_empty_library() {
        let missing = std::env::temp_dir().join("marginalia-annotations-definitely-absent");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(extract(&missing).is_err());
    }

    /// Reading must leave the store exactly as it was found. The crate has no
    /// write path by construction; this checks the construction.
    #[test]
    fn extraction_does_not_disturb_the_store() {
        let store = Store::new("readonly");
        store
            .document("doc-g", "Untouched", false)
            .content("doc-g", V1_TWO_PAGES)
            .highlight_page("doc-g", "page-one", &page_with("x"));

        let before = listing(&store.root);
        extract(&store.root).unwrap();
        assert_eq!(before, listing(&store.root));
    }

    /// Every path under a directory, with its length, sorted.
    fn listing(root: &Path) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let meta = entry.metadata().unwrap();
                if meta.is_dir() {
                    stack.push(path.clone());
                }
                out.push((path.display().to_string(), meta.len()));
            }
        }
        out.sort();
        out
    }
}
