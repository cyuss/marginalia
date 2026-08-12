//! A folder of documents, as a library source.
//!
//! The second [`LibraryProvider`], and the one that proves the port is real
//! rather than a Zotero-shaped hole with a trait around it.
//!
//! It is also the source a lot of people actually have. Not everyone keeps a
//! reference manager; plenty of reMarkable users have a directory of PDFs and
//! the same problem — no metadata, no way back from a highlight to a source.
//!
//! Two properties make it the right second implementation:
//!
//! - **it needs no network**, so the workflow demonstrably works on a device
//!   with Wi-Fi off;
//! - **it reads nothing but names and sizes**, so it cannot become a way to
//!   move a file. Like every provider, it has no method returning bytes.

use std::fs;
use std::path::{Path, PathBuf};

use marginalia_core::library::{
    Author, Identifiers, LibraryError, LibraryItem, LibraryPage, LibraryProvider, LibrarySource,
    SourceInfo, SourceRef,
};
use marginalia_core::zotero::AttachmentAvailability;

/// Extensions we consider readable material.
const READABLE: &[&str] = &["pdf", "epub"];

pub struct FolderLibrary {
    root: PathBuf,
    label: String,
}

impl FolderLibrary {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let label = root.to_string_lossy().to_string();
        Self { root, label }
    }

    pub fn with_label(root: impl Into<PathBuf>, label: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            label: label.into(),
        }
    }

    fn scan(&self) -> Result<Vec<PathBuf>, LibraryError> {
        if !self.root.exists() {
            return Err(LibraryError::NotConfigured);
        }
        let mut out = Vec::new();
        let mut stack = vec![self.root.clone()];

        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir).map_err(|e| match e.kind() {
                std::io::ErrorKind::PermissionDenied => LibraryError::Unauthorized,
                _ => LibraryError::Unreachable(e.kind().to_string()),
            })?;

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if is_readable(&path) {
                    out.push(path);
                }
            }
        }
        out.sort();
        Ok(out)
    }
}

fn is_readable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| READABLE.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Turn a filename into the best metadata a filename can honestly give.
///
/// Deliberately conservative. A filename is evidence, not a citation: it is
/// worth reading `Vaswani et al. - 2017 - Attention Is All You Need.pdf`
/// because that is how reference managers export, but anything more
/// speculative would put invented authorship in front of the user.
pub fn parse_filename(stem: &str) -> (String, Vec<Author>, Option<i32>) {
    let parts: Vec<&str> = stem.split(" - ").map(str::trim).collect();

    // "Authors - Year - Title" — the shape Zotero and Calibre both export.
    if parts.len() >= 3 {
        if let Ok(year) = parts[1].parse::<i32>() {
            if (1400..=2200).contains(&year) {
                return (parts[2..].join(" - "), parse_authors(parts[0]), Some(year));
            }
        }
    }

    // "Title (2017)" — trailing year in brackets.
    if let Some(open) = stem.rfind('(') {
        if stem.ends_with(')') {
            let inner = &stem[open + 1..stem.len() - 1];
            if let Ok(year) = inner.parse::<i32>() {
                if (1400..=2200).contains(&year) {
                    return (stem[..open].trim().to_string(), Vec::new(), Some(year));
                }
            }
        }
    }

    // Otherwise the filename is the title and nothing else is claimed.
    (stem.to_string(), Vec::new(), None)
}

fn parse_authors(field: &str) -> Vec<Author> {
    // "Vaswani et al." and "Gu, Dao" are the two forms worth handling. Anything
    // else becomes a single name rather than a guess at how to split it.
    let field = field.trim();
    if field.is_empty() {
        return Vec::new();
    }
    if let Some(first) = field.strip_suffix(" et al.") {
        return vec![Author::new(first.trim(), None)];
    }
    field
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Author::new(s, None))
        .collect()
}

impl LibraryProvider for FolderLibrary {
    fn source(&self) -> LibrarySource {
        LibrarySource::Folder
    }

    fn info(&self) -> Result<SourceInfo, LibraryError> {
        let files = self.scan()?;
        Ok(SourceInfo {
            source: LibrarySource::Folder,
            label: self.label.clone(),
            item_count: Some(files.len() as u32),
            last_refreshed: None,
        })
    }

    fn list(&self, _cursor: Option<&str>) -> Result<LibraryPage, LibraryError> {
        let files = self.scan()?;

        let items = files
            .iter()
            .map(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Untitled");
                let (title, authors, year) = parse_filename(stem);

                // A folder can be read, so its documents are available. This
                // records a fact; it does not copy anything.
                let metadata = fs::metadata(path).ok();

                LibraryItem {
                    source: LibrarySource::Folder,
                    source_ref: SourceRef::new(path.to_string_lossy()),
                    title,
                    authors,
                    year,
                    container: None,
                    identifiers: Identifiers::default(),
                    tags: Vec::new(),
                    // The directory a file sits in is the closest thing a
                    // folder has to a collection, and people do organise that
                    // way.
                    collections: relative_folders(&self.root, path),
                    availability: if metadata.is_some() {
                        AttachmentAvailability::AvailableLocal
                    } else {
                        AttachmentAvailability::Unreadable
                    },
                    size_bytes: metadata.map(|m| m.len()),
                    added_at: None,
                }
            })
            .collect();

        // A folder is read in one pass; there is nothing to paginate.
        Ok(LibraryPage::last(items))
    }
}

/// The directories between the root and the file, as a collection path.
fn relative_folders(root: &Path, file: &Path) -> Vec<String> {
    file.strip_prefix(root)
        .ok()
        .and_then(|rel| rel.parent())
        .map(|p| {
            p.components()
                .filter_map(|c| c.as_os_str().to_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalia_core::ids::DocumentId;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("marginalia-folder-{}", DocumentId::new()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn file(&self, rel: &str, bytes: usize) -> &Self {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, vec![b'x'; bytes]).unwrap();
            self
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // ── filenames ───────────────────────────────────────────────────────

    #[test]
    fn the_reference_manager_export_shape_is_understood() {
        let (title, authors, year) =
            parse_filename("Vaswani et al. - 2017 - Attention Is All You Need");
        assert_eq!(title, "Attention Is All You Need");
        assert_eq!(authors.len(), 1);
        assert_eq!(authors[0].family, "Vaswani");
        assert_eq!(year, Some(2017));
    }

    #[test]
    fn a_two_author_export_keeps_both() {
        let (_, authors, _) = parse_filename("Gu, Dao - 2023 - Mamba");
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[1].family, "Dao");
    }

    #[test]
    fn a_trailing_year_in_brackets_is_read() {
        let (title, authors, year) =
            parse_filename("The Structure of Scientific Revolutions (1962)");
        assert_eq!(title, "The Structure of Scientific Revolutions");
        assert!(authors.is_empty());
        assert_eq!(year, Some(1962));
    }

    #[test]
    fn an_ordinary_filename_claims_only_a_title() {
        // The alternative is inventing authorship, which is worse than a gap.
        let (title, authors, year) = parse_filename("notes-on-attention");
        assert_eq!(title, "notes-on-attention");
        assert!(authors.is_empty());
        assert_eq!(year, None);
    }

    #[test]
    fn a_number_that_is_not_a_year_is_not_treated_as_one() {
        let (title, _, year) = parse_filename("Chapter - 42 - Something");
        assert_eq!(year, None);
        assert_eq!(title, "Chapter - 42 - Something");
    }

    #[test]
    fn a_hyphenated_title_survives_the_split() {
        let (title, _, year) =
            parse_filename("Devlin - 2018 - BERT - Pre-training of Transformers");
        assert_eq!(title, "BERT - Pre-training of Transformers");
        assert_eq!(year, Some(2018));
    }

    // ── scanning ────────────────────────────────────────────────────────

    #[test]
    fn a_folder_becomes_a_library() {
        let s = Scratch::new();
        s.file("Vaswani et al. - 2017 - Attention Is All You Need.pdf", 128)
            .file("notes.txt", 10)
            .file("Hofstadter - 1979 - Godel Escher Bach.epub", 64);

        let page = FolderLibrary::new(&s.0).list(None).unwrap();

        assert_eq!(page.items.len(), 2, "only readable material counts");
        assert!(page.is_last(), "a folder is read in one pass");

        let attention = page.items.iter().find(|i| i.year == Some(2017)).unwrap();
        assert_eq!(attention.title, "Attention Is All You Need");
        assert_eq!(attention.byline(), "Vaswani");
        assert_eq!(attention.size_bytes, Some(128));
        assert!(attention.can_be_requested());
    }

    #[test]
    fn subdirectories_become_collections() {
        // People organise folders the way they organise collections, and
        // throwing that away would lose real structure.
        let s = Scratch::new();
        s.file("AI/Transformers/Vaswani - 2017 - Attention.pdf", 16);

        let page = FolderLibrary::new(&s.0).list(None).unwrap();
        assert_eq!(page.items[0].collections, vec!["AI", "Transformers"]);
    }

    #[test]
    fn a_file_at_the_root_has_no_collection() {
        let s = Scratch::new();
        s.file("loose.pdf", 8);
        let page = FolderLibrary::new(&s.0).list(None).unwrap();
        assert!(page.items[0].collections.is_empty());
    }

    #[test]
    fn a_missing_folder_is_not_configured_rather_than_an_error() {
        // A path that does not exist yet is a setup state, not a failure.
        let provider = FolderLibrary::new("/nonexistent-marginalia-folder");
        assert_eq!(provider.list(None), Err(LibraryError::NotConfigured));
        assert_eq!(provider.info(), Err(LibraryError::NotConfigured));
    }

    #[test]
    fn an_empty_folder_is_an_empty_library_not_an_error() {
        let s = Scratch::new();
        let page = FolderLibrary::new(&s.0).list(None).unwrap();
        assert!(page.items.is_empty());
        assert_eq!(FolderLibrary::new(&s.0).info().unwrap().item_count, Some(0));
    }

    #[test]
    fn extensions_are_matched_case_insensitively() {
        let s = Scratch::new();
        s.file("Upper.PDF", 4).file("Mixed.Epub", 4);
        assert_eq!(FolderLibrary::new(&s.0).list(None).unwrap().items.len(), 2);
    }

    #[test]
    fn the_source_is_reported_and_needs_no_network() {
        // The property that makes this the right second provider: it proves the
        // workflow runs with Wi-Fi off.
        let s = Scratch::new();
        let provider = FolderLibrary::new(&s.0);
        assert_eq!(provider.source(), LibrarySource::Folder);
        assert!(!provider.source().needs_network());
    }

    #[test]
    fn a_label_can_be_friendlier_than_a_path() {
        let s = Scratch::new();
        let provider = FolderLibrary::with_label(&s.0, "Papers on the device");
        assert_eq!(provider.info().unwrap().label, "Papers on the device");
    }

    #[test]
    fn the_provider_can_be_used_behind_a_trait_object() {
        // The whole point of the port: the workflow above never learns which
        // kind of source it is talking to.
        let s = Scratch::new();
        s.file("A Paper (2020).pdf", 32);

        let sources: Vec<Box<dyn LibraryProvider>> = vec![Box::new(FolderLibrary::new(&s.0))];
        let items: Vec<LibraryItem> = sources
            .iter()
            .filter_map(|p| p.list(None).ok())
            .flat_map(|page| page.items)
            .collect();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].year, Some(2020));
    }
}
