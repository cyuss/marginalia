//! The Zotero adapter.
//!
//! Phase 2 of the standalone roadmap needs the device to talk to the Zotero Web
//! API directly. This crate holds the parts of that which are portable and
//! testable without a network: what a credential *is*, what verifying one
//! means, and the setup flow that refuses to store an unverified key.
//!
//! The HTTP client is behind the `http` feature, off by default. Whether a TLS
//! stack builds for `armv7-unknown-linux-gnueabihf` is an open question (U16),
//! and the rest of this crate must not be blocked on the answer.
//!
//! # The rule this crate exists to keep
//!
//! Nothing here can download a PDF. [`ZoteroClient`] has no method that returns
//! file bytes. Attachment *availability* is metadata; attachment *content* is a
//! separate operation that arrives in Phase 3, behind an explicit user action.

pub mod credentials;
pub mod setup;

#[cfg(feature = "http")]
pub mod http;

pub use credentials::{LibraryKind, LibraryRef, ZoteroCredentials};
pub use setup::{SetupOutcome, SetupService};

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ZoteroError {
    /// The key was rejected. Almost always a typo or a revoked key.
    #[error("Zotero rejected the API key")]
    Unauthorized,

    /// The key is valid but does not grant access to the requested library.
    #[error("the key does not have access to this library")]
    Forbidden,

    #[error("no such library")]
    LibraryNotFound,

    /// Zotero asked us to slow down. `retry_after_secs` is its own figure.
    #[error("rate limited by Zotero; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    #[error("network unavailable: {0}")]
    Network(String),

    /// A response we could not make sense of. Kept distinct from a network
    /// failure because the remedies differ.
    #[error("unexpected response from Zotero: {0}")]
    Protocol(String),
}

impl ZoteroError {
    /// Whether retrying the same request later could plausibly succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ZoteroError::Network(_) | ZoteroError::RateLimited { .. }
        )
    }

    /// What to tell the user, in the four-part shape the error model requires.
    pub fn user_message(&self) -> &'static str {
        match self {
            ZoteroError::Unauthorized => {
                "Zotero did not accept that API key. Check it, or create a new one \
                 in your Zotero account settings."
            }
            ZoteroError::Forbidden => {
                "That key is valid but cannot read this library. Check the key's \
                 permissions in Zotero, or use a different library."
            }
            ZoteroError::LibraryNotFound => "No Zotero library with that ID. Check the library ID.",
            ZoteroError::RateLimited { .. } => {
                "Zotero asked Marginalia to slow down. It will retry shortly; \
                 nothing was lost."
            }
            ZoteroError::Network(_) => {
                "Could not reach Zotero. Check your connection — your library and \
                 annotations are unaffected."
            }
            ZoteroError::Protocol(_) => {
                "Zotero replied with something Marginalia did not understand. \
                 Nothing was changed."
            }
        }
    }
}

/// What a key can do, discovered from the key alone.
///
/// WHY this exists: asking a user for a library ID is asking them to go and
/// find a number on a settings page while they are in the middle of setting
/// something up. Zotero will tell us the number if we ask it, so we ask it.
/// The ID field disappears from the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDescription {
    /// Numeric user ID — this is the "library ID" users are otherwise sent to
    /// look up by hand.
    pub user_id: String,
    pub username: Option<String>,
    /// Access to the user's own library, if the key grants any.
    pub personal: Option<LibraryAccess>,
    /// Specific group libraries the key names.
    pub group_ids: Vec<String>,
    /// Zotero can grant "all groups" without listing them. We cannot turn that
    /// into a list without a second request, so we report it honestly rather
    /// than pretending we know the group IDs.
    pub all_groups: bool,
}

impl KeyDescription {
    /// Every library we can offer without asking Zotero again.
    pub fn known_libraries(&self) -> Vec<LibraryRef> {
        let mut out = Vec::new();
        if self.personal.is_some() {
            out.push(LibraryRef::user(self.user_id.clone()));
        }
        out.extend(self.group_ids.iter().map(LibraryRef::group));
        out
    }

    /// Whether setup can proceed without asking the user anything else.
    pub fn has_exactly_one_library(&self) -> bool {
        self.known_libraries().len() == 1 && !self.all_groups
    }
}

/// What a key is permitted to do in one library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LibraryAccess {
    pub read: bool,
    pub write: bool,
    pub notes: bool,
    /// Whether the key may fetch attachment *content*. Recorded because Phase 3
    /// needs it; recording it does not download anything.
    pub files: bool,
}

/// What a successful verification told us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyVerification {
    /// The account the key belongs to, for showing the user which library they
    /// just connected without revealing the key.
    pub username: Option<String>,
    pub user_id: Option<String>,
    pub grants_read: bool,
    /// Only needed once the user enables annotation export.
    pub grants_write: bool,
}

/// Talking to Zotero.
///
/// Deliberately narrow. In particular there is **no method that returns file
/// bytes**: metadata sync cannot download a PDF because this trait cannot
/// express it, which is the same firewall technique used between
/// `MetadataOperation` and `TransferOperation`.
pub trait ZoteroClient {
    /// Confirm a key works against a specific library. One minimal request.
    fn verify(&self, credentials: &ZoteroCredentials) -> Result<KeyVerification, ZoteroError>;

    /// Ask Zotero what this key is and what it can reach.
    ///
    /// This is what lets setup take a key and nothing else: the user ID comes
    /// back in the answer, so the user never has to find it themselves.
    fn describe_key(
        &self,
        api_key: &marginalia_core::secret::Redacted<String>,
    ) -> Result<KeyDescription, ZoteroError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_errors_are_distinguished_from_permanent_ones() {
        // The retry policy depends on this, and retrying an Unauthorized
        // forever is how an app gets a key locked out.
        assert!(ZoteroError::Network("timeout".into()).is_transient());
        assert!(ZoteroError::RateLimited {
            retry_after_secs: 30
        }
        .is_transient());

        assert!(!ZoteroError::Unauthorized.is_transient());
        assert!(!ZoteroError::Forbidden.is_transient());
        assert!(!ZoteroError::LibraryNotFound.is_transient());
    }

    #[test]
    fn every_error_has_a_user_message_that_says_what_to_do() {
        let errors = [
            ZoteroError::Unauthorized,
            ZoteroError::Forbidden,
            ZoteroError::LibraryNotFound,
            ZoteroError::RateLimited {
                retry_after_secs: 1,
            },
            ZoteroError::Network("x".into()),
            ZoteroError::Protocol("x".into()),
        ];
        for e in &errors {
            let msg = e.user_message();
            assert!(msg.len() > 30, "{e:?} has a stub message");
            assert!(
                !msg.contains("HTTP") && !msg.contains("401"),
                "{e:?} leaks protocol detail into user-facing copy: {msg}"
            );
        }
    }

    fn description(personal: bool, groups: &[&str], all_groups: bool) -> KeyDescription {
        KeyDescription {
            user_id: "1234567".into(),
            username: Some("youcef".into()),
            personal: personal.then(|| LibraryAccess {
                read: true,
                ..Default::default()
            }),
            group_ids: groups.iter().map(|g| g.to_string()).collect(),
            all_groups,
        }
    }

    #[test]
    fn a_key_with_only_a_personal_library_needs_no_further_questions() {
        // The common case: setup completes from the key alone.
        let d = description(true, &[], false);
        assert!(d.has_exactly_one_library());
        assert_eq!(d.known_libraries(), vec![LibraryRef::user("1234567")]);
    }

    #[test]
    fn a_key_with_groups_offers_a_choice() {
        let d = description(true, &["98765"], false);
        assert!(!d.has_exactly_one_library());
        assert_eq!(
            d.known_libraries(),
            vec![LibraryRef::user("1234567"), LibraryRef::group("98765")]
        );
    }

    #[test]
    fn all_groups_access_is_reported_rather_than_invented() {
        // Zotero can say "all groups" without naming them. Claiming to know
        // the list would be a guess, so setup asks instead of assuming.
        let d = description(true, &[], true);
        assert!(d.all_groups);
        assert!(
            !d.has_exactly_one_library(),
            "we cannot skip the choice when there may be groups we have not listed"
        );
    }

    #[test]
    fn a_key_with_no_library_access_offers_nothing() {
        let d = description(false, &[], false);
        assert!(d.known_libraries().is_empty());
        assert!(!d.has_exactly_one_library());
    }

    #[test]
    fn an_error_message_never_contains_a_key() {
        // The realistic leak: an error built by interpolating the request.
        let e = ZoteroError::Network("connect to https://api.zotero.org failed".into());
        assert!(!format!("{e}").contains("Bearer"));
        assert!(!e.user_message().contains("key="));
    }
}
