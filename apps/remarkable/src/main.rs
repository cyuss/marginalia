//! # The Marginalia agent
//!
//! Runs **on** a reMarkable. Headless: it never draws to the screen, never
//! talks to `xochitl`, and never touches a path outside the one directory it
//! owns.
//!
//! This is the option D shape from `docs/adr/ADR-002-remarkable-ui-and-runtime.md`
//! — everything the user sees is a document the native reader already renders,
//! and everything the user asks for arrives as a stylus mark on a generated
//! form (`docs/adr/ADR-006-on-device-interaction.md`).
//!
//! ## What this build actually does
//!
//! Honestly: not much yet, and deliberately so. It proves the runtime works on
//! the device — it starts, owns a directory, opens and migrates its database
//! under the durable storage profile, reports what it is permitted to do, and
//! exits cleanly. Zotero sync and document generation are wired in later
//! phases, once this has been shown to survive on real hardware.
//!
//! What it will never do is grow a code path that writes outside its home.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use marginalia_core::credentials::{CredentialKey, CredentialStore};
use marginalia_database::StorageProfile;
use marginalia_platform::FileCredentialStore;
use marginalia_safety::{FeatureFlag, FeatureFlagManager};

#[cfg(feature = "network")]
mod zotero_cmd;

/// Everything the agent owns lives under here. One directory, removable in one
/// command, listed in the install manifest.
const DEFAULT_HOME: &str = "/home/root/.marginalia";

/// Paths the agent must never write to, checked at startup rather than trusted.
///
/// This is belt-and-braces: the code has no reason to touch these, and the
/// check exists so that a future mistake fails loudly at launch instead of
/// quietly on someone's device.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "/usr", "/etc", "/lib", "/bin", "/sbin", "/boot", "/opt", "/var", "/proc", "/sys",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("status");

    let home = std::env::var("MARGINALIA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_HOME));

    if let Err(message) = check_home_is_ours(&home) {
        eprintln!("refusing to run: {message}");
        return ExitCode::from(2);
    }

    match command {
        "status" => status(&home),
        "init" => init(&home),
        "doctor" => doctor(&home),

        #[cfg(feature = "network")]
        "sync" => zotero_cmd::sync(&home, marginalia_core::sync::JobTrigger::User),

        #[cfg(feature = "network")]
        "zotero" => match args.get(1).map(String::as_str) {
            Some("connect") => match args.get(2) {
                Some(key) => zotero_cmd::connect(&home, key.clone()),
                None => {
                    eprintln!("usage: marginalia zotero connect <api-key>");
                    eprintln!("Create one at https://www.zotero.org/settings/keys");
                    ExitCode::from(64)
                }
            },
            Some("use") => match args.get(2) {
                Some(id) => zotero_cmd::use_library(
                    &home,
                    id,
                    args.get(3).map(String::as_str) == Some("--group"),
                ),
                None => {
                    eprintln!("usage: marginalia zotero use <library-id> [--group]");
                    ExitCode::from(64)
                }
            },
            Some("disconnect") => zotero_cmd::disconnect(&home),
            _ => {
                eprintln!("usage: marginalia zotero <connect|use|disconnect>");
                ExitCode::from(64)
            }
        },

        #[cfg(not(feature = "network"))]
        "sync" | "zotero" => {
            eprintln!(
                "This build has no network support, so it cannot reach Zotero.\n\
                 Rebuild with:  cargo build -p marginalia-agent --features network"
            );
            ExitCode::from(69)
        }
        "version" | "--version" | "-V" => {
            println!("marginalia {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command: {other}\n");
            print_help();
            ExitCode::from(64)
        }
    }
}

fn print_help() {
    println!(
        "marginalia — the on-device agent

USAGE
    marginalia <command>

COMMANDS
    status     what the agent knows and is permitted to do (default)
    init       create the agent's home and database
    doctor     check this installation, changing nothing
    version    print the version
    help       this text

  Zotero (builds with --features network)
    zotero connect <api-key>    connect a library; the ID is discovered for you
    zotero use <id> [--group]   pick a library, when the key reaches several
    zotero disconnect           forget the key here (does NOT revoke it at Zotero)
    sync                        bring the metadata up to date

  Syncing brings titles, authors, collections, tags and which attachments
  exist. It never moves a PDF: that is a separate, explicit request.

ENVIRONMENT
    MARGINALIA_HOME    where the agent keeps everything (default {DEFAULT_HOME})

The agent writes only inside its home directory. It never modifies the
reMarkable's own software, and removing that one directory removes it entirely."
    );
}

/// Refuse to run if home points anywhere that is not ours to write to.
fn check_home_is_ours(home: &Path) -> Result<(), String> {
    let path = home.to_string_lossy();

    if !home.is_absolute() {
        return Err(format!(
            "MARGINALIA_HOME must be an absolute path, got {path}"
        ));
    }
    for prefix in FORBIDDEN_PREFIXES {
        if path.starts_with(prefix) {
            return Err(format!(
                "{path} is inside {prefix}, which belongs to the device. \
                 The agent only writes to its own directory."
            ));
        }
    }
    if path == "/" || path == "/home" || path == "/home/root" {
        return Err(format!(
            "{path} is too broad. The agent needs its own directory so that \
             removing it removes only the agent."
        ));
    }
    Ok(())
}

fn init(home: &Path) -> ExitCode {
    if let Err(e) = std::fs::create_dir_all(home) {
        eprintln!("could not create {}: {e}", home.display());
        return ExitCode::FAILURE;
    }

    let db_path = home.join("marginalia.sqlite");
    // The device profile: rollback journal and synchronous=FULL. A reMarkable
    // can lose power without warning while holding the only copy of someone's
    // annotation history.
    match marginalia_database::open_with_profile(&db_path.to_string_lossy(), StorageProfile::Device)
    {
        Ok(conn) => {
            let version = marginalia_database::migrations::current_version(&conn).unwrap_or(0);
            println!("ready");
            println!("  home     {}", home.display());
            println!("  database {} (schema v{version})", db_path.display());
            println!(
                "  journal  {}",
                marginalia_database::journal_mode(&conn).unwrap_or_else(|_| "unknown".into())
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("could not open the database: {e}");
            eprintln!("nothing was changed. Your documents are unaffected.");
            ExitCode::FAILURE
        }
    }
}

fn status(home: &Path) -> ExitCode {
    println!("Marginalia {}", env!("CARGO_PKG_VERSION"));
    println!();

    let db_path = home.join("marginalia.sqlite");
    let installed = home.exists();
    let has_db = db_path.exists();

    println!("  home        {}", home.display());
    println!("  initialised {}", yes_no(installed && has_db));

    if has_db {
        match marginalia_database::open_with_profile(
            &db_path.to_string_lossy(),
            StorageProfile::Device,
        ) {
            Ok(conn) => {
                let version = marginalia_database::migrations::current_version(&conn).unwrap_or(0);
                println!("  schema      v{version}");
            }
            Err(e) => println!("  schema      unreadable ({e})"),
        }
    }

    let credentials = FileCredentialStore::new(home);
    let zotero = credentials
        .load(CredentialKey::ZoteroApiKey)
        .ok()
        .flatten()
        .is_some();
    println!(
        "  zotero      {}",
        if zotero { "connected" } else { "not connected" }
    );

    println!();
    println!("Permitted right now");
    let flags = FeatureFlagManager::new();
    for (label, flag) in [
        (
            "send documents to this device",
            FeatureFlag::SafeDocumentTransfer,
        ),
        (
            "write annotations into PDFs",
            FeatureFlag::NativePdfAnnotations,
        ),
        ("two-way tag sync", FeatureFlag::BidirectionalTagSync),
    ] {
        println!(
            "  {} {label}",
            if flags.is_enabled(flag) { "yes" } else { " no" }
        );
    }

    println!();
    println!("Never, under any setting");
    for line in [
        "modify the reMarkable's own software",
        "touch a document Marginalia did not put here",
        "delete anything to free space",
    ] {
        println!("  no  {line}");
    }

    ExitCode::SUCCESS
}

fn doctor(home: &Path) -> ExitCode {
    println!("Checking this installation. Nothing will be changed.");
    println!();

    let mut problems = 0;

    let mut check = |label: &str, ok: bool, remedy: &str| {
        println!("  {}  {label}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            println!("        {remedy}");
            problems += 1;
        }
    };

    check(
        "home directory exists",
        home.exists(),
        "run `marginalia init`",
    );
    check(
        "home is writable",
        home.exists()
            && !home
                .metadata()
                .map(|m| m.permissions().readonly())
                .unwrap_or(true),
        "check the directory's permissions",
    );

    let db = home.join("marginalia.sqlite");
    let db_ok = db.exists()
        && marginalia_database::open_with_profile(&db.to_string_lossy(), StorageProfile::Device)
            .is_ok();
    check(
        "database opens under the device profile",
        db_ok,
        "the database may be corrupt; move it aside and run `marginalia init`",
    );

    let secret = home.join("zotero_api_key.secret");
    if secret.exists() {
        check(
            "the Zotero key is readable only by you",
            secret_is_private(&secret),
            "run `chmod 600` on it",
        );
    }

    println!();
    if problems == 0 {
        println!("No problems found.");
        ExitCode::SUCCESS
    } else {
        println!("{problems} problem(s) found. Nothing was changed.");
        ExitCode::FAILURE
    }
}

#[cfg(unix)]
fn secret_is_private(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o077 == 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn secret_is_private(_path: &Path) -> bool {
    true
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_paths_are_refused_as_a_home() {
        // Nothing in the agent has a reason to write here. The check exists so
        // that a future mistake fails at launch rather than on a device.
        for path in [
            "/usr/lib/marginalia",
            "/etc/marginalia",
            "/boot/marginalia",
            "/var/lib/marginalia",
        ] {
            assert!(
                check_home_is_ours(Path::new(path)).is_err(),
                "{path} should be refused"
            );
        }
    }

    #[test]
    fn overly_broad_homes_are_refused() {
        // Removing the agent must remove only the agent.
        for path in ["/", "/home", "/home/root"] {
            assert!(
                check_home_is_ours(Path::new(path)).is_err(),
                "{path} should be refused"
            );
        }
    }

    #[test]
    fn a_relative_home_is_refused() {
        assert!(check_home_is_ours(Path::new(".marginalia")).is_err());
    }

    #[test]
    fn the_default_home_is_accepted() {
        assert!(check_home_is_ours(Path::new(DEFAULT_HOME)).is_ok());
        assert!(check_home_is_ours(Path::new("/home/root/.marginalia")).is_ok());
    }

    #[test]
    fn a_refusal_explains_itself() {
        let message = check_home_is_ours(Path::new("/usr/share/marginalia")).unwrap_err();
        assert!(message.contains("/usr"));
        assert!(message.contains("own directory"));
    }
}
