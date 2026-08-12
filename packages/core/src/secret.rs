//! Values that must never be printed.
//!
//! WHY this lives in the domain core rather than in the logging crate: a secret
//! is a domain concern. The credential port below it needs to name one, and
//! the core cannot depend on the logging crate. Moving it here also makes the
//! guarantee available to any adapter, on any target, including one that never
//! initialises a logging subscriber at all.
//!
//! The realistic leak this prevents is not `println!("{}", api_key)` — nobody
//! writes that. It is `tracing::info!(?config)` on a struct that happens to
//! contain a key three fields down. `Redacted` renders as `<redacted>` in
//! `Debug` as well as `Display`, so the lazy call site is safe by default.

use std::fmt;

/// A value that must never appear in output.
///
/// Deliberately not `Serialize`: a secret that can be serialised will
/// eventually be serialised into a log line, a crash report, or a config file.
/// Persisting one is the job of a [`crate::credentials::CredentialStore`]
/// implementation, which reaches for the inner value explicitly.
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Deliberately verbose. Reading a secret should look unusual in a diff.
    pub fn expose_secret(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper. Same warning as [`Self::expose_secret`].
    pub fn into_secret(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct Config {
        endpoint: String,
        api_key: Redacted<String>,
    }

    #[test]
    fn a_secret_never_renders() {
        let secret = Redacted::new("zotero-api-key-abc123".to_string());
        assert_eq!(format!("{secret}"), "<redacted>");
        assert_eq!(format!("{secret:?}"), "<redacted>");
    }

    #[test]
    fn a_lazy_debug_on_the_whole_struct_still_redacts() {
        let config = Config {
            endpoint: "https://api.zotero.org".into(),
            api_key: Redacted::new("zotero-api-key-abc123".into()),
        };
        let rendered = format!("{config:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("abc123"));
    }

    #[test]
    fn the_secret_is_reachable_when_genuinely_needed() {
        let secret = Redacted::new("token".to_string());
        assert_eq!(secret.expose_secret(), "token");
        assert_eq!(secret.into_secret(), "token");
    }
}
