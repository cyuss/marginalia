//! Host adapters.
//!
//! Implementations of the ports declared in `marginalia-core`, for things that
//! actually touch a filesystem or an environment. Nothing here is imported by
//! the domain; the dependency points inward.
//!
//! This crate exists now because [`credentials`] has a real consumer: the setup
//! flow needs somewhere to put a Zotero API key. It was deliberately not
//! created when the port was written, per the rule about extracting adapters at
//! real seams rather than in advance.

pub mod credentials;

pub use credentials::{EnvCredentialStore, FileCredentialStore};
