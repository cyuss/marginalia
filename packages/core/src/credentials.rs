//! Credential storage, as a port.
//!
//! Phase 0 put the Zotero API key in the desktop OS secure store. The
//! reMarkable has no Keychain, no Credential Manager and no Secret Service, and
//! `keyring` is now forbidden in the portable crates by an architecture test.
//!
//! This is a real seam rather than a speculative one: Phase 2 needs the device
//! to hold a Zotero key, and the two targets cannot share an implementation.
//! The **implementations** deliberately do not exist yet — they arrive with the
//! setup flow that needs them. See `docs/adr/ADR-004-device-credentials.md`.

use crate::secret::Redacted;
use std::collections::BTreeMap;
use std::sync::Mutex;
use thiserror::Error;

/// The secrets Marginalia may hold.
///
/// An enum rather than a free-form string so that a typo cannot silently
/// create a second, orphaned credential — and so the complete set of secrets
/// the product stores is greppable in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CredentialKey {
    /// Zotero Web API key. Revocable by the user from their Zotero account.
    ZoteroApiKey,
    /// Password for user-enabled developer access to a device, held by the
    /// desktop companion only. The device never stores its own.
    DeviceAccessPassword,
}

impl CredentialKey {
    /// A stable identifier for use as a storage key. Never derived from
    /// `Debug`, which is free to change.
    pub const fn storage_id(self) -> &'static str {
        match self {
            CredentialKey::ZoteroApiKey => "zotero_api_key",
            CredentialKey::DeviceAccessPassword => "device_access_password",
        }
    }

    pub const ALL: [CredentialKey; 2] = [
        CredentialKey::ZoteroApiKey,
        CredentialKey::DeviceAccessPassword,
    ];
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialError {
    #[error("no credential store is available on this platform")]
    Unavailable,

    #[error("the credential store refused access")]
    Denied,

    #[error("credential store failure: {0}")]
    Backend(String),
}

/// Somewhere to keep a secret.
///
/// Note the shape: secrets go in and come out wrapped in [`Redacted`], so the
/// only way to see one is an explicit `expose_secret()` call that stands out in
/// review.
pub trait CredentialStore: Send + Sync {
    fn store(&self, key: CredentialKey, secret: Redacted<String>) -> Result<(), CredentialError>;

    fn load(&self, key: CredentialKey) -> Result<Option<Redacted<String>>, CredentialError>;

    fn delete(&self, key: CredentialKey) -> Result<(), CredentialError>;

    /// Remove every secret. Backs the "revoke and reset" action, which must
    /// leave nothing behind — a reset that forgets one key is worse than no
    /// reset, because the user believes they are clean.
    fn clear_all(&self) -> Result<(), CredentialError> {
        for key in CredentialKey::ALL {
            self.delete(key)?;
        }
        Ok(())
    }
}

/// An in-process store, for tests and for a first-run state with nothing
/// persisted yet.
///
/// Not a production implementation: it holds secrets in memory and forgets
/// them on exit, which is the correct behaviour for a test double and the
/// wrong behaviour for a product.
#[derive(Debug, Default)]
pub struct InMemoryCredentialStore {
    entries: Mutex<BTreeMap<CredentialKey, String>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many secrets are held. For tests that assert a reset was complete.
    pub fn len(&self) -> usize {
        self.entries.lock().expect("credential lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn store(&self, key: CredentialKey, secret: Redacted<String>) -> Result<(), CredentialError> {
        self.entries
            .lock()
            .expect("credential lock")
            .insert(key, secret.into_secret());
        Ok(())
    }

    fn load(&self, key: CredentialKey) -> Result<Option<Redacted<String>>, CredentialError> {
        Ok(self
            .entries
            .lock()
            .expect("credential lock")
            .get(&key)
            .cloned()
            .map(Redacted::new))
    }

    fn delete(&self, key: CredentialKey) -> Result<(), CredentialError> {
        self.entries.lock().expect("credential lock").remove(&key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips_without_ever_rendering() {
        let store = InMemoryCredentialStore::new();
        store
            .store(
                CredentialKey::ZoteroApiKey,
                Redacted::new("secret-key-value".into()),
            )
            .unwrap();

        let loaded = store.load(CredentialKey::ZoteroApiKey).unwrap().unwrap();

        // The value survived...
        assert_eq!(loaded.expose_secret(), "secret-key-value");
        // ...but printing it does not leak it.
        assert_eq!(format!("{loaded:?}"), "<redacted>");
        assert!(!format!("{loaded}").contains("secret"));
    }

    #[test]
    fn an_absent_credential_is_none_not_an_error() {
        // First run is not a failure state.
        let store = InMemoryCredentialStore::new();
        assert_eq!(store.load(CredentialKey::ZoteroApiKey).unwrap(), None);
    }

    #[test]
    fn clear_all_leaves_nothing_behind() {
        // A reset that forgets one key is worse than no reset: the user
        // believes they are clean.
        let store = InMemoryCredentialStore::new();
        for key in CredentialKey::ALL {
            store.store(key, Redacted::new("value".into())).unwrap();
        }
        assert_eq!(store.len(), CredentialKey::ALL.len());

        store.clear_all().unwrap();

        assert!(store.is_empty());
        for key in CredentialKey::ALL {
            assert_eq!(store.load(key).unwrap(), None, "{key:?} survived a reset");
        }
    }

    #[test]
    fn deleting_one_credential_leaves_the_others() {
        let store = InMemoryCredentialStore::new();
        store
            .store(CredentialKey::ZoteroApiKey, Redacted::new("a".into()))
            .unwrap();
        store
            .store(
                CredentialKey::DeviceAccessPassword,
                Redacted::new("b".into()),
            )
            .unwrap();

        store.delete(CredentialKey::ZoteroApiKey).unwrap();

        assert_eq!(store.load(CredentialKey::ZoteroApiKey).unwrap(), None);
        assert!(store
            .load(CredentialKey::DeviceAccessPassword)
            .unwrap()
            .is_some());
    }

    #[test]
    fn storage_ids_are_stable_and_distinct() {
        // These become filenames and keychain entries. A collision would make
        // one secret silently overwrite another.
        let ids: Vec<&str> = CredentialKey::ALL.iter().map(|k| k.storage_id()).collect();
        assert_eq!(ids, vec!["zotero_api_key", "device_access_password"]);

        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "storage ids must be distinct");
    }

    #[test]
    fn the_port_is_usable_behind_a_trait_object() {
        // Production will pass a keyring-backed or file-backed store; nothing
        // above this line needs to know which.
        let store: Box<dyn CredentialStore> = Box::new(InMemoryCredentialStore::new());
        store
            .store(CredentialKey::ZoteroApiKey, Redacted::new("k".into()))
            .unwrap();
        assert!(store.load(CredentialKey::ZoteroApiKey).unwrap().is_some());
    }
}
