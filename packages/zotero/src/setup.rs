//! The setup flow.
//!
//! This is what runs when the user adds their Zotero key during installation.
//!
//! # The rule
//!
//! **A key is verified before it is stored, never after.** Storing first and
//! validating later produces the worst outcome available: a persisted secret
//! that does not work, and a user who believes setup succeeded. On failure
//! nothing is written, and any previously working key is left alone.

use marginalia_core::credentials::{CredentialKey, CredentialStore};
use marginalia_core::secret::Redacted;

use crate::credentials::{LibraryRef, ZoteroCredentials};
use crate::{KeyVerification, ZoteroClient, ZoteroError};

/// What the user is told after pressing Connect.
#[derive(Debug, PartialEq, Eq)]
pub enum SetupOutcome {
    /// Verified and stored.
    Connected {
        library: LibraryRef,
        /// For "Connected to <account>'s personal library" — never the key.
        username: Option<String>,
        /// Whether export to Zotero will be available, or only reading.
        can_export: bool,
    },
    /// Rejected before any network call, because the input could not be right.
    Malformed { reason: MalformedReason },
    /// Zotero was asked and said no. Nothing was stored.
    Rejected { error: ZoteroError },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MalformedReason {
    LibraryIdNotNumeric,
    ApiKeyImplausible,
}

impl MalformedReason {
    pub const fn user_message(self) -> &'static str {
        match self {
            MalformedReason::LibraryIdNotNumeric => {
                "A Zotero library ID is a number. You can find yours at \
                 zotero.org/settings/keys, under 'Your userID for use in API calls'."
            }
            MalformedReason::ApiKeyImplausible => {
                "That does not look like a Zotero API key. Create one at \
                 zotero.org/settings/keys — it is a long string of letters and digits."
            }
        }
    }
}

pub struct SetupService<'a> {
    client: &'a dyn ZoteroClient,
    store: &'a dyn CredentialStore,
}

impl<'a> SetupService<'a> {
    pub fn new(client: &'a dyn ZoteroClient, store: &'a dyn CredentialStore) -> Self {
        Self { client, store }
    }

    /// Verify a key against Zotero and, only if that succeeds, store it.
    ///
    /// The key is taken by value so the caller's copy is consumed: the plain
    /// string should not outlive this call.
    pub fn connect(&self, api_key: String, library: LibraryRef) -> SetupOutcome {
        // Cheap local checks first. A round trip to say "that's a URL, not an
        // ID" is a worse experience and a pointless request.
        if !library.is_well_formed() {
            return SetupOutcome::Malformed {
                reason: MalformedReason::LibraryIdNotNumeric,
            };
        }

        let credentials = ZoteroCredentials::new(Redacted::new(api_key), library);

        if !credentials.key_is_plausible() {
            return SetupOutcome::Malformed {
                reason: MalformedReason::ApiKeyImplausible,
            };
        }

        match self.client.verify(&credentials) {
            Ok(KeyVerification {
                username,
                grants_write,
                ..
            }) => {
                // Only now does the secret touch disk.
                if let Err(e) = self.store.store(
                    CredentialKey::ZoteroApiKey,
                    Redacted::new(credentials.api_key().expose_secret().clone()),
                ) {
                    return SetupOutcome::Rejected {
                        error: ZoteroError::Protocol(format!("could not store the key: {e}")),
                    };
                }
                SetupOutcome::Connected {
                    library: credentials.library().clone(),
                    username,
                    can_export: grants_write,
                }
            }
            Err(error) => SetupOutcome::Rejected { error },
        }
    }

    /// Forget the stored key. Backs "Revoke and reset".
    ///
    /// This removes Marginalia's copy. It does **not** revoke the key at
    /// Zotero, which only the user can do, and the UI must say so — otherwise
    /// someone believes a compromised key is dead when it is still live.
    pub fn disconnect(&self) -> Result<(), marginalia_core::credentials::CredentialError> {
        self.store.delete(CredentialKey::ZoteroApiKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalia_core::credentials::InMemoryCredentialStore;
    use std::cell::RefCell;

    /// A client that answers however the test needs, and counts calls so a test
    /// can assert the network was not touched.
    struct StubClient {
        response: RefCell<Option<Result<KeyVerification, ZoteroError>>>,
        calls: RefCell<usize>,
    }

    impl StubClient {
        fn ok(grants_write: bool) -> Self {
            Self {
                response: RefCell::new(Some(Ok(KeyVerification {
                    username: Some("youcef".into()),
                    user_id: Some("12345".into()),
                    grants_read: true,
                    grants_write,
                }))),
                calls: RefCell::new(0),
            }
        }

        fn failing(error: ZoteroError) -> Self {
            Self {
                response: RefCell::new(Some(Err(error))),
                calls: RefCell::new(0),
            }
        }

        fn call_count(&self) -> usize {
            *self.calls.borrow()
        }
    }

    impl ZoteroClient for StubClient {
        fn verify(&self, _c: &ZoteroCredentials) -> Result<KeyVerification, ZoteroError> {
            *self.calls.borrow_mut() += 1;
            self.response
                .borrow_mut()
                .take()
                .unwrap_or(Err(ZoteroError::Unauthorized))
        }
    }

    const GOOD_KEY: &str = "P9NiFoyLeZu2bZNvvuQPDWsd";

    #[test]
    fn a_verified_key_is_stored() {
        let client = StubClient::ok(false);
        let store = InMemoryCredentialStore::new();
        let outcome =
            SetupService::new(&client, &store).connect(GOOD_KEY.into(), LibraryRef::user("12345"));

        match outcome {
            SetupOutcome::Connected {
                library,
                username,
                can_export,
            } => {
                assert_eq!(library, LibraryRef::user("12345"));
                assert_eq!(username.as_deref(), Some("youcef"));
                assert!(!can_export, "a read-only key must not promise export");
            }
            other => panic!("expected Connected, got {other:?}"),
        }

        assert!(store.load(CredentialKey::ZoteroApiKey).unwrap().is_some());
    }

    /// The rule this module exists for.
    #[test]
    fn a_rejected_key_is_never_stored() {
        let client = StubClient::failing(ZoteroError::Unauthorized);
        let store = InMemoryCredentialStore::new();

        let outcome =
            SetupService::new(&client, &store).connect(GOOD_KEY.into(), LibraryRef::user("12345"));

        assert_eq!(
            outcome,
            SetupOutcome::Rejected {
                error: ZoteroError::Unauthorized
            }
        );
        assert!(
            store.is_empty(),
            "a key Zotero rejected must not reach disk; setup would appear to \
             have succeeded while nothing works"
        );
    }

    #[test]
    fn a_failed_reconnect_leaves_the_working_key_alone() {
        // The user re-runs setup and mistypes. Their existing, working key must
        // survive that.
        let store = InMemoryCredentialStore::new();
        store
            .store(
                CredentialKey::ZoteroApiKey,
                Redacted::new("the-working-key".into()),
            )
            .unwrap();

        let client = StubClient::failing(ZoteroError::Unauthorized);
        let _ = SetupService::new(&client, &store)
            .connect("QQQQQQQQQQQQQQQQQQQQQQQQ".into(), LibraryRef::user("12345"));

        assert_eq!(
            store
                .load(CredentialKey::ZoteroApiKey)
                .unwrap()
                .unwrap()
                .expose_secret(),
            "the-working-key"
        );
    }

    #[test]
    fn a_malformed_library_id_never_reaches_the_network() {
        let client = StubClient::ok(true);
        let store = InMemoryCredentialStore::new();

        let outcome = SetupService::new(&client, &store).connect(
            GOOD_KEY.into(),
            LibraryRef::user("https://www.zotero.org/user/12345"),
        );

        assert_eq!(
            outcome,
            SetupOutcome::Malformed {
                reason: MalformedReason::LibraryIdNotNumeric
            }
        );
        assert_eq!(client.call_count(), 0, "no request should have been made");
        assert!(store.is_empty());
    }

    #[test]
    fn a_malformed_key_never_reaches_the_network() {
        let client = StubClient::ok(true);
        let store = InMemoryCredentialStore::new();

        let outcome = SetupService::new(&client, &store)
            .connect("Bearer abc".into(), LibraryRef::user("12345"));

        assert_eq!(
            outcome,
            SetupOutcome::Malformed {
                reason: MalformedReason::ApiKeyImplausible
            }
        );
        assert_eq!(client.call_count(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn a_write_capable_key_enables_export() {
        let client = StubClient::ok(true);
        let store = InMemoryCredentialStore::new();
        let outcome =
            SetupService::new(&client, &store).connect(GOOD_KEY.into(), LibraryRef::user("12345"));

        assert!(matches!(
            outcome,
            SetupOutcome::Connected {
                can_export: true,
                ..
            }
        ));
    }

    #[test]
    fn disconnect_removes_our_copy() {
        let client = StubClient::ok(false);
        let store = InMemoryCredentialStore::new();
        let service = SetupService::new(&client, &store);

        service.connect(GOOD_KEY.into(), LibraryRef::user("12345"));
        assert!(!store.is_empty());

        service.disconnect().unwrap();
        assert!(store.is_empty());
    }

    #[test]
    fn a_group_library_is_supported() {
        let client = StubClient::ok(false);
        let store = InMemoryCredentialStore::new();
        let outcome =
            SetupService::new(&client, &store).connect(GOOD_KEY.into(), LibraryRef::group("98765"));

        assert!(matches!(outcome, SetupOutcome::Connected { .. }));
    }

    #[test]
    fn every_malformed_reason_tells_the_user_where_to_look() {
        for reason in [
            MalformedReason::LibraryIdNotNumeric,
            MalformedReason::ApiKeyImplausible,
        ] {
            assert!(reason.user_message().contains("zotero.org"));
        }
    }
}
