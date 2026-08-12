//! Credential store implementations.
//!
//! Two, for two different jobs:
//!
//! - [`FileCredentialStore`] — what the device uses, and what the desktop uses
//!   until the Tauri wiring lands. One file per secret, mode `0600`, inside the
//!   application's own data directory.
//! - [`EnvCredentialStore`] — read-only, for tests and CI. Reading a secret
//!   from the environment is appropriate for a test runner and inappropriate
//!   for a product, so this one refuses to write.
//!
//! See `docs/adr/ADR-004-device-credentials.md`, including its honest note that
//! a `0600` file is not a hardware-backed keychain.

use std::fs;
use std::path::{Path, PathBuf};

use marginalia_core::credentials::{CredentialError, CredentialKey, CredentialStore};
use marginalia_core::secret::Redacted;

/// File-backed secrets in a directory Marginalia owns.
///
/// One file per secret rather than one file with all of them: deleting a single
/// credential then cannot rewrite — and therefore cannot corrupt or lose — the
/// others.
#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    dir: PathBuf,
}

impl FileCredentialStore {
    /// `dir` must be inside the application's own data area. This type never
    /// chooses the path itself, so a caller cannot accidentally get secrets
    /// written next to a user's documents.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path_for(&self, key: CredentialKey) -> PathBuf {
        self.dir.join(format!("{}.secret", key.storage_id()))
    }

    fn ensure_dir(&self) -> Result<(), CredentialError> {
        fs::create_dir_all(&self.dir).map_err(|e| CredentialError::Backend(e.to_string()))?;
        restrict_permissions(&self.dir, 0o700)?;
        Ok(())
    }
}

/// Tighten a path to owner-only access.
///
/// On Unix this is real. On other platforms it is a no-op, and that is a
/// limitation rather than a solved problem — recorded here rather than hidden,
/// because a caller on Windows should know that the file's protection is
/// whatever the directory ACL says.
#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) -> Result<(), CredentialError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, perms).map_err(|e| CredentialError::Backend(e.to_string()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path, _mode: u32) -> Result<(), CredentialError> {
    // Windows relies on the parent directory's ACL. The desktop build uses the
    // OS credential manager there instead of this store.
    Ok(())
}

impl CredentialStore for FileCredentialStore {
    fn store(&self, key: CredentialKey, secret: Redacted<String>) -> Result<(), CredentialError> {
        self.ensure_dir()?;
        let path = self.path_for(key);

        // Write, then tighten. The window between the two is why the directory
        // is already 0700: even briefly, the file is not reachable by another
        // user.
        fs::write(&path, secret.expose_secret().as_bytes())
            .map_err(|e| CredentialError::Backend(e.to_string()))?;
        restrict_permissions(&path, 0o600)?;
        Ok(())
    }

    fn load(&self, key: CredentialKey) -> Result<Option<Redacted<String>>, CredentialError> {
        let path = self.path_for(key);
        match fs::read_to_string(&path) {
            Ok(contents) => Ok(Some(Redacted::new(contents.trim().to_string()))),
            // Absent is not an error: a first run has no credentials.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(CredentialError::Denied)
            }
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }

    fn delete(&self, key: CredentialKey) -> Result<(), CredentialError> {
        match fs::remove_file(self.path_for(key)) {
            Ok(()) => Ok(()),
            // Deleting something already absent is a success: the caller's
            // goal was "this must not exist", and it does not.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CredentialError::Backend(e.to_string())),
        }
    }
}

/// Secrets from environment variables. Read-only.
///
/// For the integration tests and CI: a test runner may legitimately be handed a
/// key through its environment. A product may not persist one there, so
/// [`CredentialStore::store`] and [`CredentialStore::delete`] deliberately fail
/// rather than silently doing nothing.
#[derive(Debug, Clone, Default)]
pub struct EnvCredentialStore;

impl EnvCredentialStore {
    pub fn new() -> Self {
        Self
    }

    /// The variable consulted for a given secret.
    pub fn var_name(key: CredentialKey) -> String {
        format!("MARGINALIA_{}", key.storage_id().to_uppercase())
    }
}

impl CredentialStore for EnvCredentialStore {
    fn store(&self, _key: CredentialKey, _secret: Redacted<String>) -> Result<(), CredentialError> {
        Err(CredentialError::Backend(
            "the environment store is read-only; secrets are not persisted there".into(),
        ))
    }

    fn load(&self, key: CredentialKey) -> Result<Option<Redacted<String>>, CredentialError> {
        match std::env::var(Self::var_name(key)) {
            Ok(v) if v.trim().is_empty() => Ok(None),
            Ok(v) => Ok(Some(Redacted::new(v.trim().to_string()))),
            Err(_) => Ok(None),
        }
    }

    fn delete(&self, _key: CredentialKey) -> Result<(), CredentialError> {
        Err(CredentialError::Backend(
            "the environment store is read-only".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marginalia_core::ids::DocumentId;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("marginalia-cred-{}", DocumentId::new()));
            Self(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_secret_round_trips_through_a_file() {
        let dir = TempDir::new();
        let store = FileCredentialStore::new(&dir.0);

        store
            .store(
                CredentialKey::ZoteroApiKey,
                Redacted::new("a-zotero-key".into()),
            )
            .unwrap();

        let loaded = store.load(CredentialKey::ZoteroApiKey).unwrap().unwrap();
        assert_eq!(loaded.expose_secret(), "a-zotero-key");
        assert_eq!(format!("{loaded:?}"), "<redacted>");
    }

    #[test]
    fn a_missing_credential_is_none_not_an_error() {
        let dir = TempDir::new();
        let store = FileCredentialStore::new(&dir.0);
        assert_eq!(store.load(CredentialKey::ZoteroApiKey).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_secret_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new();
        let store = FileCredentialStore::new(&dir.0);
        store
            .store(CredentialKey::ZoteroApiKey, Redacted::new("k".into()))
            .unwrap();

        let path = dir.0.join("zotero_api_key.secret");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "the secret file must not be readable by others"
        );

        let dir_mode = fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            dir_mode, 0o700,
            "the containing directory must be owner-only"
        );
    }

    #[test]
    fn each_secret_lives_in_its_own_file() {
        // So that deleting one cannot rewrite, corrupt, or lose another.
        let dir = TempDir::new();
        let store = FileCredentialStore::new(&dir.0);

        store
            .store(CredentialKey::ZoteroApiKey, Redacted::new("zotero".into()))
            .unwrap();
        store
            .store(
                CredentialKey::DeviceAccessPassword,
                Redacted::new("device".into()),
            )
            .unwrap();

        store.delete(CredentialKey::ZoteroApiKey).unwrap();

        assert_eq!(store.load(CredentialKey::ZoteroApiKey).unwrap(), None);
        assert_eq!(
            store
                .load(CredentialKey::DeviceAccessPassword)
                .unwrap()
                .unwrap()
                .expose_secret(),
            "device"
        );
    }

    #[test]
    fn clear_all_leaves_no_file_behind() {
        let dir = TempDir::new();
        let store = FileCredentialStore::new(&dir.0);
        for key in CredentialKey::ALL {
            store.store(key, Redacted::new("v".into())).unwrap();
        }

        store.clear_all().unwrap();

        for key in CredentialKey::ALL {
            assert_eq!(store.load(key).unwrap(), None, "{key:?} survived a reset");
        }
        let remaining: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "secret"))
            .collect();
        assert!(remaining.is_empty(), "a reset must leave no secret files");
    }

    #[test]
    fn deleting_an_absent_credential_succeeds() {
        // The caller's goal is "this must not exist". It does not.
        let dir = TempDir::new();
        let store = FileCredentialStore::new(&dir.0);
        assert!(store.delete(CredentialKey::ZoteroApiKey).is_ok());
    }

    #[test]
    fn a_trailing_newline_is_not_part_of_the_secret() {
        // Someone will eventually create this file with `echo key > file`.
        let dir = TempDir::new();
        fs::create_dir_all(&dir.0).unwrap();
        fs::write(dir.0.join("zotero_api_key.secret"), "a-key\n").unwrap();

        let store = FileCredentialStore::new(&dir.0);
        let loaded = store.load(CredentialKey::ZoteroApiKey).unwrap().unwrap();
        assert_eq!(loaded.expose_secret(), "a-key");
    }

    #[test]
    fn the_environment_store_refuses_to_persist() {
        // A test runner may hand us a key. A product may not leave one there.
        let store = EnvCredentialStore::new();
        assert!(store
            .store(CredentialKey::ZoteroApiKey, Redacted::new("k".into()))
            .is_err());
        assert!(store.delete(CredentialKey::ZoteroApiKey).is_err());
    }

    #[test]
    fn the_environment_variable_names_are_stable() {
        assert_eq!(
            EnvCredentialStore::var_name(CredentialKey::ZoteroApiKey),
            "MARGINALIA_ZOTERO_API_KEY"
        );
        assert_eq!(
            EnvCredentialStore::var_name(CredentialKey::DeviceAccessPassword),
            "MARGINALIA_DEVICE_ACCESS_PASSWORD"
        );
    }
}
