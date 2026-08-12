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

#[derive(Debug, Error, PartialEq, Eq)]
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
    /// Confirm a key works and report what it can do. One minimal request.
    fn verify(&self, credentials: &ZoteroCredentials) -> Result<KeyVerification, ZoteroError>;
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

    #[test]
    fn an_error_message_never_contains_a_key() {
        // The realistic leak: an error built by interpolating the request.
        let e = ZoteroError::Network("connect to https://api.zotero.org failed".into());
        assert!(!format!("{e}").contains("Bearer"));
        assert!(!e.user_message().contains("key="));
    }
}
