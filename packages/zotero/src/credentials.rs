//! What a Zotero credential is.
//!
//! Three things the setup flow must keep distinct, because users conflate them
//! and the consequences differ:
//!
//! - the **library ID** identifies a library. Not a secret.
//! - the **library kind** says whether that ID is a user or a group. The API
//!   paths differ, and a right ID with the wrong kind gives a confusing 404.
//! - the **API key** is a revocable secret. Never displayed, never logged.

use marginalia_core::secret::Redacted;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryKind {
    User,
    Group,
}

impl LibraryKind {
    /// The path segment Zotero uses.
    pub const fn path_segment(self) -> &'static str {
        match self {
            LibraryKind::User => "users",
            LibraryKind::Group => "groups",
        }
    }
}

impl fmt::Display for LibraryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LibraryKind::User => "personal library",
            LibraryKind::Group => "group library",
        })
    }
}

/// Which library to read. Not secret — safe to display and to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRef {
    pub kind: LibraryKind,
    pub id: String,
}

impl LibraryRef {
    pub fn user(id: impl Into<String>) -> Self {
        Self {
            kind: LibraryKind::User,
            id: id.into(),
        }
    }

    pub fn group(id: impl Into<String>) -> Self {
        Self {
            kind: LibraryKind::Group,
            id: id.into(),
        }
    }

    /// Zotero library IDs are numeric. Catching a paste error here gives a far
    /// better message than a 404 three screens later.
    pub fn is_well_formed(&self) -> bool {
        !self.id.is_empty() && self.id.chars().all(|c| c.is_ascii_digit())
    }

    pub fn base_path(&self) -> String {
        format!("/{}/{}", self.kind.path_segment(), self.id)
    }
}

impl fmt::Display for LibraryRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind, self.id)
    }
}

/// An API key plus the library it is for.
///
/// Not `Clone`: a credential should be constructed where it is used rather than
/// copied around. Its `Debug` cannot leak the key, because the key is a
/// `Redacted`.
#[derive(Debug)]
pub struct ZoteroCredentials {
    api_key: Redacted<String>,
    library: LibraryRef,
}

impl ZoteroCredentials {
    pub fn new(api_key: Redacted<String>, library: LibraryRef) -> Self {
        Self { api_key, library }
    }

    pub fn library(&self) -> &LibraryRef {
        &self.library
    }

    /// Reaching for the key. Called by the HTTP layer when building a header,
    /// and nowhere else.
    pub fn api_key(&self) -> &Redacted<String> {
        &self.api_key
    }

    /// Whether the key looks like a Zotero key at all.
    ///
    /// A shape check, not an authorisation check — only Zotero can say whether
    /// a key is valid. This exists to turn "pasted the wrong thing" into an
    /// immediate, clear message instead of a failed network round trip.
    pub fn key_is_plausible(&self) -> bool {
        let k = self.api_key.expose_secret();
        k.len() >= 20 && k.chars().all(|c| c.is_ascii_alphanumeric())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(key: &str, library: LibraryRef) -> ZoteroCredentials {
        ZoteroCredentials::new(Redacted::new(key.to_string()), library)
    }

    #[test]
    fn a_credential_never_prints_its_key() {
        // The whole struct, via Debug -- the realistic leak.
        let c = creds("aaaaaaaaaaaaaaaaaaaaaaaa", LibraryRef::user("12345"));
        let rendered = format!("{c:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("aaaaaaaa"));
        // The library, by contrast, is not secret and should be visible.
        assert!(rendered.contains("12345"));
    }

    #[test]
    fn library_ids_are_numeric() {
        assert!(LibraryRef::user("12345").is_well_formed());
        assert!(LibraryRef::group("987").is_well_formed());

        // The common paste errors.
        assert!(!LibraryRef::user("").is_well_formed());
        assert!(!LibraryRef::user("my-library").is_well_formed());
        assert!(
            !LibraryRef::user("https://www.zotero.org/user/12345").is_well_formed(),
            "a pasted URL must be rejected with a clear message, not sent"
        );
    }

    #[test]
    fn user_and_group_libraries_use_different_paths() {
        // A right ID with the wrong kind produces a confusing 404, so the kind
        // is part of the credential rather than inferred.
        assert_eq!(LibraryRef::user("42").base_path(), "/users/42");
        assert_eq!(LibraryRef::group("42").base_path(), "/groups/42");
    }

    #[test]
    fn a_key_shape_check_catches_the_obvious_paste_errors() {
        let library = LibraryRef::user("12345");

        assert!(creds("aaaaaaaaaaaaaaaaaaaaaaaa", library.clone()).key_is_plausible());

        assert!(!creds("too-short", library.clone()).key_is_plausible());
        assert!(
            !creds("Bearer aaaaaaaaaaaaaaaaaaaaaa", library.clone()).key_is_plausible(),
            "a pasted Authorization header is not a key"
        );
        assert!(!creds("", library).key_is_plausible());
    }

    #[test]
    fn the_shape_check_does_not_pretend_to_be_an_authorisation_check() {
        // A well-shaped key that Zotero has revoked still looks plausible.
        // Only the network can answer that, which is why verify() exists.
        let c = creds("bbbbbbbbbbbbbbbbbbbbbbbb", LibraryRef::user("1"));
        assert!(c.key_is_plausible());
    }
}
