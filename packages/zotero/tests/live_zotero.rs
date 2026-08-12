//! # Live Zotero integration tests
//!
//! These talk to the real Zotero API. They are **skipped unless you provide a
//! key through the environment**, so `cargo test` stays offline, deterministic
//! and safe by default.
//!
//! ```bash
//! export MARGINALIA_ZOTERO_API_KEY=your-new-key
//! export MARGINALIA_ZOTERO_LIBRARY_ID=1234567
//! cargo test -p marginalia-zotero --features http -- --ignored --nocapture
//! ```
//!
//! # Why the key is not in this file
//!
//! `SECURITY.md` and `CONTRIBUTING.md` both say no key may be embedded in
//! builds, fixtures, screenshots, crash reports, or source control. A key in a
//! test file is in source control the moment the test is committed, and stays
//! in the history after it is removed. The environment is the right place: it
//! is per-machine, per-session, and never serialised into the repository.
//!
//! A key that has been pasted into a chat, an issue, or a pull request should
//! be treated as compromised and revoked at zotero.org/settings/keys.

#![cfg(feature = "http")]

use marginalia_core::credentials::{CredentialKey, CredentialStore, InMemoryCredentialStore};
use marginalia_platform::EnvCredentialStore;
use marginalia_zotero::http::HttpZoteroClient;
use marginalia_zotero::{LibraryRef, SetupOutcome, SetupService, ZoteroClient, ZoteroError};

/// The credentials for a live run, or `None` if the environment has none.
fn live_credentials() -> Option<(String, LibraryRef)> {
    let store = EnvCredentialStore::new();
    let key = store.load(CredentialKey::ZoteroApiKey).ok()??;
    let library_id = std::env::var("MARGINALIA_ZOTERO_LIBRARY_ID").ok()?;
    Some((
        key.expose_secret().clone(),
        match std::env::var("MARGINALIA_ZOTERO_LIBRARY_KIND").as_deref() {
            Ok("group") => LibraryRef::group(library_id),
            _ => LibraryRef::user(library_id),
        },
    ))
}

macro_rules! require_credentials {
    () => {
        match live_credentials() {
            Some(c) => c,
            None => {
                eprintln!(
                    "skipping: set MARGINALIA_ZOTERO_API_KEY and \
                     MARGINALIA_ZOTERO_LIBRARY_ID to run live tests"
                );
                return;
            }
        }
    };
}

#[test]
#[ignore = "live: needs MARGINALIA_ZOTERO_API_KEY and MARGINALIA_ZOTERO_LIBRARY_ID"]
fn a_real_key_verifies_against_the_real_api() {
    let (key, library) = require_credentials!();

    let client = HttpZoteroClient::new();
    let creds = marginalia_zotero::http::credentials_from(
        marginalia_core::secret::Redacted::new(key),
        library.clone(),
    );

    match client.verify(&creds) {
        Ok(v) => {
            assert!(v.grants_read, "the key should be able to read {library}");
            println!("verified against {library}");
        }
        Err(e) => panic!("verification failed: {e}"),
    }
}

#[test]
#[ignore = "live: needs MARGINALIA_ZOTERO_API_KEY and MARGINALIA_ZOTERO_LIBRARY_ID"]
fn the_setup_flow_connects_end_to_end() {
    let (key, library) = require_credentials!();

    let client = HttpZoteroClient::new();
    let store = InMemoryCredentialStore::new();
    let outcome = SetupService::new(&client, &store).connect(key, library);

    match outcome {
        SetupOutcome::Connected {
            library,
            can_export,
            ..
        } => {
            println!("connected to {library}; export available: {can_export}");
            assert!(
                store.load(CredentialKey::ZoteroApiKey).unwrap().is_some(),
                "a verified key must be stored"
            );
        }
        other => panic!("setup did not connect: {other:?}"),
    }
}

#[test]
#[ignore = "live: needs MARGINALIA_ZOTERO_LIBRARY_ID"]
fn a_wrong_key_is_rejected_and_not_stored() {
    // The failure path against the real API, which is the one that matters:
    // a rejected key must never reach storage.
    let library_id = match std::env::var("MARGINALIA_ZOTERO_LIBRARY_ID") {
        Ok(id) => id,
        Err(_) => {
            eprintln!("skipping: set MARGINALIA_ZOTERO_LIBRARY_ID");
            return;
        }
    };

    let client = HttpZoteroClient::new();
    let store = InMemoryCredentialStore::new();

    // Well-formed but not a real key, so it passes the shape check and is
    // genuinely sent.
    let outcome = SetupService::new(&client, &store).connect(
        "ZZZZZZZZZZZZZZZZZZZZZZZZ".into(),
        LibraryRef::user(library_id),
    );

    match outcome {
        SetupOutcome::Rejected { error } => {
            assert!(
                matches!(
                    error,
                    ZoteroError::Unauthorized
                        | ZoteroError::Forbidden
                        | ZoteroError::LibraryNotFound
                ),
                "expected a rejection, got {error:?}"
            );
        }
        other => panic!("a bogus key should be rejected, got {other:?}"),
    }

    assert!(store.is_empty(), "a rejected key must never be stored");
}
