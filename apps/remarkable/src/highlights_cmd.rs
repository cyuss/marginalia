//! `marginalia highlights` — read back what you marked while reading.
//!
//! This is the first command that does something with the reMarkable's own
//! library rather than with Marginalia's. It reads; it never writes there. The
//! only thing it may write is a review document, and that goes inside the
//! agent's own home directory.

use marginalia_annotations::{extract, extract_one, DocumentHighlights, DEFAULT_STORE};
use marginalia_database::highlights::{HighlightRecord, HighlightRepository};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Overridable so the command can be run against a copied store on a
/// workstation, which is how it is exercised without a device present.
fn store_path() -> PathBuf {
    std::env::var("MARGINALIA_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_STORE))
}

/// `marginalia highlights` — one line per document.
pub fn list() -> ExitCode {
    let store = store_path();
    let library = match extract(&store) {
        Ok(library) => library,
        Err(e) => return unreadable_store(&store, &e.to_string()),
    };

    if library.documents.is_empty() {
        println!("No highlights yet.");
        println!();
        println!("  Highlight some text in the reMarkable's own reader —");
        println!("  select with the stylus, then choose the highlighter.");
        println!("  Marginalia reads what the device already stores.");
        return ExitCode::SUCCESS;
    }

    println!(
        "{} document(s), {} highlight(s)",
        library.documents.len(),
        library.total_highlights()
    );
    println!();

    let width = library
        .documents
        .iter()
        .map(|d| d.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(52);

    for document in &library.documents {
        let name = truncate(&document.name, width);
        println!(
            "  {name:<width$}  {:>4}  {}",
            document.count(),
            document.file_type,
        );
        if let Some(problem) = &document.page_order_problem {
            println!(
                "  {:<width$}        page numbers unavailable: {problem}",
                ""
            );
        }
    }

    println!();
    println!("  marginalia highlights <part of a title>   to read them");
    println!("  marginalia highlights --export            to write them to a file");

    report_unreadable(&library.unreadable);
    ExitCode::SUCCESS
}

/// `marginalia highlights <query>` — the passages themselves.
pub fn show(query: &str) -> ExitCode {
    let store = store_path();
    let library = match extract(&store) {
        Ok(library) => library,
        Err(e) => return unreadable_store(&store, &e.to_string()),
    };

    let needle = query.to_lowercase();
    let matches: Vec<&DocumentHighlights> = library
        .documents
        .iter()
        .filter(|d| d.name.to_lowercase().contains(&needle) || d.uuid == query)
        .collect();

    if matches.is_empty() {
        eprintln!("Nothing matching \"{query}\".");
        eprintln!();
        eprintln!("  marginalia highlights   lists everything with highlights");
        return ExitCode::from(1);
    }

    for document in matches {
        println!("{}", document.name);
        println!("{}", "─".repeat(document.name.chars().count().min(60)));
        if let Some(problem) = &document.page_order_problem {
            println!("Page numbers unavailable: {problem}");
        }
        println!();

        for page in &document.pages {
            let label = match page.page_number {
                Some(n) => format!("page {n}"),
                None => "page unknown".to_string(),
            };
            for highlight in &page.highlights {
                println!("  {}", highlight.text.trim());
                println!("      — {label}");
                println!();
            }
        }
    }

    ExitCode::SUCCESS
}

/// `marginalia highlights --export` — a Markdown file per document.
///
/// Written into the agent's own home. Nothing is placed in the reMarkable's
/// library: putting a generated file there would make Marginalia the author of
/// a document in someone else's collection, which is exactly the boundary the
/// project refuses to cross without being asked.
pub fn export(home: &Path) -> ExitCode {
    let store = store_path();
    let library = match extract(&store) {
        Ok(library) => library,
        Err(e) => return unreadable_store(&store, &e.to_string()),
    };

    if library.documents.is_empty() {
        println!("Nothing to export yet.");
        return ExitCode::SUCCESS;
    }

    let out = home.join("highlights");
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("could not create {}: {e}", out.display());
        return ExitCode::FAILURE;
    }

    let mut written = 0usize;
    for document in &library.documents {
        let path = out.join(format!("{}.md", safe_filename(&document.name)));
        match std::fs::write(&path, markdown(document)) {
            Ok(()) => written += 1,
            Err(e) => eprintln!("could not write {}: {e}", path.display()),
        }
    }

    println!("Wrote {written} file(s) to {}", out.display());
    println!();
    println!("  Copy them to your computer with:");
    println!("      scp -r root@<device-ip>:{}/ .", out.display());

    report_unreadable(&library.unreadable);
    ExitCode::SUCCESS
}

/// One document as Markdown.
fn markdown(document: &DocumentHighlights) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", document.name));

    if let Some(problem) = &document.page_order_problem {
        out.push_str(&format!("> Page numbers unavailable: {problem}\n\n"));
    }

    for page in &document.pages {
        for highlight in &page.highlights {
            // Blockquote, because it is someone else's sentence. Every line
            // gets its own marker so a passage spanning lines stays quoted.
            for line in highlight.text.trim().lines() {
                out.push_str(&format!("> {line}\n"));
            }
            match page.page_number {
                Some(n) => out.push_str(&format!(">\n> — page {n}\n\n")),
                None => out.push_str(">\n> — page unknown\n\n"),
            }
        }
    }

    out.push_str(&format!(
        "---\n\nExtracted by Marginalia (extraction v{}, format verified against firmware {}).\n\
         The reMarkable's own files remain the source of truth.\n",
        marginalia_annotations::EXTRACTION_VERSION,
        marginalia_annotations::VERIFIED_AGAINST_FIRMWARE,
    ));
    out
}

/// `marginalia highlights --document <uuid>` for scripting.
pub fn one(uuid: &str) -> ExitCode {
    let store = store_path();
    match extract_one(&store, uuid) {
        Ok(Some(document)) => {
            match serde_json::to_string_pretty(&document) {
                Ok(json) => println!("{json}"),
                Err(e) => {
                    eprintln!("could not render: {e}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("{uuid} has no highlights, or has been deleted.");
            ExitCode::from(1)
        }
        Err(reason) => {
            eprintln!("{reason}");
            ExitCode::FAILURE
        }
    }
}

fn unreadable_store(store: &Path, cause: &str) -> ExitCode {
    eprintln!("Could not read the reMarkable's document store.");
    eprintln!("  {cause}");
    eprintln!();
    if store.to_string_lossy() == DEFAULT_STORE {
        eprintln!("  This command reads the device's own library, so it runs on");
        eprintln!("  the reMarkable itself:");
        eprintln!("      ssh root@<device-ip> '/home/root/.marginalia/bin/marginalia highlights'");
        eprintln!();
        eprintln!("  To try it against a copy instead, set MARGINALIA_STORE.");
    }
    ExitCode::FAILURE
}

fn report_unreadable(unreadable: &[(String, String)]) {
    if unreadable.is_empty() {
        return;
    }
    eprintln!();
    eprintln!("{} document(s) could not be read:", unreadable.len());
    for (uuid, reason) in unreadable {
        eprintln!("  {uuid}: {reason}");
    }
    eprintln!("Nothing was changed. These are listed rather than skipped silently.");
}

fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    // `width` is at least 1 here because it is the max of at least one name.
    let keep = width.saturating_sub(1);
    format!("{}…", s.chars().take(keep).collect::<String>())
}

/// A document title is a person's words; a filename is a filesystem's. This
/// keeps the first readable inside the second without inventing a new title.
fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

/// `marginalia highlights --save` — extract, and keep what was found.
///
/// Writes to the agent's own database and nothing else. The device's files are
/// read; your library is not touched.
pub fn save(home: &Path) -> ExitCode {
    let store = store_path();
    let library = match extract(&store) {
        Ok(library) => library,
        Err(e) => return unreadable_store(&store, &e.to_string()),
    };

    let records = to_records(&library);

    let db_path = home.join("marginalia.sqlite");
    let conn = match marginalia_database::open_with_profile(
        &db_path.to_string_lossy(),
        marginalia_database::StorageProfile::Device,
    ) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("could not open the database: {e}");
            eprintln!("nothing was changed. Your documents are unaffected.");
            return ExitCode::FAILURE;
        }
    };

    let repo = HighlightRepository::new(&conn);
    let summary = match repo.record_extraction(
        &records,
        marginalia_annotations::EXTRACTION_VERSION,
        library.unreadable.len(),
    ) {
        Ok(summary) => summary,
        Err(e) => {
            eprintln!("could not store the highlights: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Only safe because `extract` read the whole store. A single-document run
    // must never call this: everything else would look as though it had gone.
    let gone = repo.mark_gone(&records).unwrap_or(0);

    println!(
        "{} highlight(s) across {} document(s)",
        summary.highlights_seen, summary.documents_seen
    );
    if summary.highlights_new > 0 {
        println!("  {} new since the last run", summary.highlights_new);
    } else {
        println!("  nothing new since the last run");
    }
    if gone > 0 {
        println!("  {gone} no longer on the device — kept here anyway");
    }
    println!("  {} kept in total", repo.total().unwrap_or(0));
    println!();
    println!("  marginalia highlights --new    what arrived since the run before");

    report_unreadable(&library.unreadable);
    ExitCode::SUCCESS
}

/// `marginalia highlights --new` — what appeared since the previous run.
pub fn whats_new(home: &Path) -> ExitCode {
    let db_path = home.join("marginalia.sqlite");
    let conn = match marginalia_database::open_with_profile(
        &db_path.to_string_lossy(),
        marginalia_database::StorageProfile::Device,
    ) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("could not open the database: {e}");
            return ExitCode::FAILURE;
        }
    };

    let repo = HighlightRepository::new(&conn);
    let Some(previous) = repo.previous_run_at().unwrap_or(None) else {
        println!("There has only been one run so far, so there is nothing to compare with.");
        println!();
        println!("  marginalia highlights --save    run it again later, then ask");
        return ExitCode::SUCCESS;
    };

    match repo.since(&previous) {
        Ok(rows) if rows.is_empty() => {
            println!("Nothing new since {previous}.");
            ExitCode::SUCCESS
        }
        Ok(rows) => {
            println!("{} new highlight(s) since {previous}", rows.len());
            println!();
            let mut current = String::new();
            for row in rows {
                if row.document_name != current {
                    println!("{}", row.document_name);
                    current = row.document_name.clone();
                }
                let page = match row.page_number {
                    Some(n) => format!("page {n}"),
                    None => "page unknown".to_string(),
                };
                println!("  {}", row.text.trim());
                println!("      — {page}");
                println!();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not read them back: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The extractor's types, flattened into rows the database understands.
///
/// This mapping lives here rather than in either crate because the agent is the
/// only thing that knows both — which is what keeps `marginalia-database` from
/// having to learn the reMarkable's file formats.
fn to_records(library: &marginalia_annotations::Library) -> Vec<HighlightRecord> {
    library
        .documents
        .iter()
        .flat_map(|document| {
            document.pages.iter().flat_map(move |page| {
                page.highlights
                    .iter()
                    .map(move |highlight| HighlightRecord {
                        document_uuid: document.uuid.clone(),
                        document_name: document.name.clone(),
                        file_type: document.file_type.clone(),
                        page_id: page.page_id.clone(),
                        page_number: page.page_number,
                        start_offset: highlight.start,
                        length: highlight.length,
                        text: highlight.text.clone(),
                        color: highlight.color,
                    })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document title comes from the user and lands in a path, so the only
    /// property that matters is that it cannot leave the directory it is
    /// written to. The exact substitution is cosmetic; the absence of a
    /// separator is not.
    #[test]
    fn a_title_becomes_a_filename_without_escaping_its_directory() {
        assert_eq!(safe_filename("Notes/On Certainty"), "Notes-On Certainty");
        assert_eq!(safe_filename("   "), "untitled");
        assert_eq!(safe_filename(""), "untitled");

        for hostile in [
            "../../etc/passwd",
            "..\\..\\windows\\system32",
            "/absolute/path",
            "with\0null",
        ] {
            let safe = safe_filename(hostile);
            let path = Path::new(&safe);
            assert_eq!(
                path.components().count(),
                1,
                "{hostile:?} became {safe:?}, which is more than one path component"
            );
            assert!(!safe.contains('/'), "{safe:?} still contains a separator");
            assert!(!safe.contains('\\'), "{safe:?} still contains a separator");
        }
    }

    /// A title that is only dots would otherwise produce "." or "..".
    #[test]
    fn a_title_of_dots_does_not_become_a_directory_reference() {
        assert_eq!(safe_filename("."), "untitled");
        assert_eq!(safe_filename(".."), "untitled");
    }

    #[test]
    fn a_long_title_is_shortened_rather_than_refused() {
        let long = "a".repeat(300);
        assert_eq!(safe_filename(&long).chars().count(), 80);
    }

    #[test]
    fn truncation_keeps_the_column_width_it_promises() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(
            truncate("a very long document title", 10).chars().count(),
            10
        );
    }

    /// Multi-byte titles must not be cut mid-character.
    #[test]
    fn truncation_counts_characters_not_bytes() {
        let title = "Généalogie de la morale";
        let cut = truncate(title, 8);
        assert_eq!(cut.chars().count(), 8);
    }
}
