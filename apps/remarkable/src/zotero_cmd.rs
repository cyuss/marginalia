//! The `zotero` and `sync` commands. Behind the `network` feature.
//!
//! These are the two things the agent does that reach the internet, and they
//! are the two the user cares about: connect a library, then keep it current.
//!
//! Neither can move a PDF. `SyncRunner` hands the database `MetadataOperation`s
//! and that type has no variant capable of expressing a transfer.

use std::path::Path;
use std::process::ExitCode;

use marginalia_core::credentials::{CredentialKey, CredentialStore};
use marginalia_core::secret::Redacted;
use marginalia_core::sync::JobTrigger;
use marginalia_database::StorageProfile;
use marginalia_platform::FileCredentialStore;
use marginalia_sync::SyncRunner;
use marginalia_zotero::credentials::{LibraryRef, ZoteroCredentials};
use marginalia_zotero::http::HttpZoteroClient;
use marginalia_zotero::{SetupOutcome, SetupService};

/// Where the chosen library is remembered. Not a secret, so it sits beside the
/// database rather than in the credential store.
const LIBRARY_FILE: &str = "library";

pub fn connect(home: &Path, api_key: String) -> ExitCode {
    if let Err(e) = std::fs::create_dir_all(home) {
        eprintln!("could not create {}: {e}", home.display());
        return ExitCode::FAILURE;
    }

    let client = HttpZoteroClient::new();
    let store = FileCredentialStore::new(home);

    println!("Asking Zotero about that key…");
    match SetupService::new(&client, &store).connect_with_key(api_key) {
        SetupOutcome::Connected {
            library,
            username,
            can_export,
        } => {
            if let Err(e) = std::fs::write(home.join(LIBRARY_FILE), library.base_path()) {
                eprintln!("connected, but could not remember which library: {e}");
                return ExitCode::FAILURE;
            }
            println!("Connected.");
            println!("  library  {library}");
            if let Some(name) = username {
                println!("  account  {name}");
            }
            println!(
                "  export   {}",
                if can_export {
                    "allowed"
                } else {
                    "not granted — reading only"
                }
            );
            println!("\nNext: marginalia sync");
            ExitCode::SUCCESS
        }

        SetupOutcome::ChooseLibrary {
            options,
            may_have_more_groups,
            ..
        } => {
            // Nothing has been stored: configuring a library the user did not
            // choose would be worse than asking.
            println!("That key reaches more than one library. Pick one:\n");
            for option in &options {
                println!("  marginalia zotero use {}", option.id);
                println!("      {option}");
            }
            if may_have_more_groups {
                println!("\n  (Zotero also granted access to groups it did not name.)");
            }
            ExitCode::from(3)
        }

        SetupOutcome::NoLibraryAccess { .. } => {
            eprintln!("That key is valid but cannot read any library.");
            eprintln!("In Zotero, edit the key and tick 'Allow library access'.");
            ExitCode::FAILURE
        }

        SetupOutcome::Malformed { reason } => {
            eprintln!("{}", reason.user_message());
            ExitCode::from(64)
        }

        SetupOutcome::Rejected { error } => {
            eprintln!("{}", error.user_message());
            eprintln!("Nothing was saved.");
            ExitCode::FAILURE
        }
    }
}

pub fn disconnect(home: &Path) -> ExitCode {
    let store = FileCredentialStore::new(home);
    if let Err(e) = store.delete(CredentialKey::ZoteroApiKey) {
        eprintln!("could not remove the key: {e}");
        return ExitCode::FAILURE;
    }
    let _ = std::fs::remove_file(home.join(LIBRARY_FILE));

    println!("Disconnected. Marginalia's copy of the key is gone.");
    println!();
    println!("This did NOT revoke the key at Zotero — only you can do that:");
    println!("  https://www.zotero.org/settings/keys");
    ExitCode::SUCCESS
}

pub fn sync(home: &Path, trigger: JobTrigger) -> ExitCode {
    let store = FileCredentialStore::new(home);
    let key = match store.load(CredentialKey::ZoteroApiKey) {
        Ok(Some(k)) => k,
        Ok(None) => {
            eprintln!("No Zotero library connected yet.");
            eprintln!("Run: marginalia zotero connect <your-api-key>");
            return ExitCode::from(3);
        }
        Err(e) => {
            eprintln!("could not read the stored key: {e}");
            return ExitCode::FAILURE;
        }
    };

    let library = match read_library(home) {
        Some(l) => l,
        None => {
            eprintln!("Connected, but no library is chosen. Run `zotero connect` again.");
            return ExitCode::from(3);
        }
    };

    let db = home.join("marginalia.sqlite");
    let conn =
        match marginalia_database::open_with_profile(&db.to_string_lossy(), StorageProfile::Device)
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("could not open the database: {e}");
                return ExitCode::FAILURE;
            }
        };

    let credentials = ZoteroCredentials::new(Redacted::new(key.expose_secret().clone()), library);
    let client = HttpZoteroClient::new();

    println!("Syncing metadata from Zotero…");
    match SyncRunner::new(&conn, &client).run(&credentials, trigger) {
        Ok(report) => {
            println!("{}", report.summary());
            if report.stopped_at_page_limit {
                println!("\nStopped early to stay within this run's budget.");
                println!("Nothing was lost. Run sync again to continue.");
            } else if report.completed {
                println!("\nUp to date.");
            }
            // Stated every time, including now.
            println!("\nNo PDFs were transferred. Syncing never moves a file.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.user_message());
            if e.is_transient() {
                eprintln!("Nothing was lost — running sync again will continue where it stopped.");
            }
            ExitCode::FAILURE
        }
    }
}

fn read_library(home: &Path) -> Option<LibraryRef> {
    let raw = std::fs::read_to_string(home.join(LIBRARY_FILE)).ok()?;
    let trimmed = raw.trim().trim_start_matches('/');
    let (kind, id) = trimmed.split_once('/')?;
    match kind {
        "users" => Some(LibraryRef::user(id)),
        "groups" => Some(LibraryRef::group(id)),
        _ => None,
    }
}

/// Remember a specific library, for the multi-library case.
pub fn use_library(home: &Path, id: &str, group: bool) -> ExitCode {
    let library = if group {
        LibraryRef::group(id)
    } else {
        LibraryRef::user(id)
    };
    if !library.is_well_formed() {
        eprintln!("A Zotero library ID is a number. Got: {id}");
        return ExitCode::from(64);
    }
    match std::fs::write(home.join(LIBRARY_FILE), library.base_path()) {
        Ok(()) => {
            println!("Using {library}.");
            println!("Next: marginalia sync");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not remember that choice: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `marginalia zotero tree` — the folders, as stored on this device.
///
/// Reads the database and nothing else: no network, no library. If it is empty
/// it says so, rather than printing a hopeful blank.
pub fn tree(home: &Path) -> ExitCode {
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

    let applier = marginalia_database::sync_apply::MetadataApplier::new(&conn);
    let folders = match applier.collection_tree() {
        Ok(folders) => folders,
        Err(e) => {
            eprintln!("could not read the folders: {e}");
            return ExitCode::FAILURE;
        }
    };

    if folders.is_empty() {
        println!("No folders stored yet.");
        println!();
        println!("  marginalia sync    fetch them from Zotero");
        return ExitCode::SUCCESS;
    }

    println!("{} folder(s)\n", folders.len());
    for folder in &folders {
        let indent = "   ".repeat(folder.depth);
        let branch = if folder.depth > 0 { "└─ " } else { "" };
        let subfolders = if folder.children > 0 {
            format!("   [{} subfolders]", folder.children)
        } else {
            String::new()
        };
        println!("{indent}{branch}{}{subfolders}", folder.name);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remembered_library_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "marginalia-lib-{}",
            marginalia_core::ids::DocumentId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        use_library(&dir, "12345", false);
        assert_eq!(read_library(&dir), Some(LibraryRef::user("12345")));

        use_library(&dir, "98765", true);
        assert_eq!(read_library(&dir), Some(LibraryRef::group("98765")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_numeric_library_id_is_refused() {
        let dir = std::env::temp_dir().join("marginalia-lib-refuse");
        assert_eq!(use_library(&dir, "not-a-number", false), ExitCode::from(64));
    }

    #[test]
    fn an_absent_library_file_is_none_not_a_panic() {
        assert_eq!(read_library(Path::new("/nonexistent-marginalia")), None);
    }
}
