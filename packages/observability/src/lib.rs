//! Structured logging.
//!
//! Levels are the usual four plus one: `SAFETY`. WHY a fifth: a user who is
//! deciding whether to trust this software with their device needs to be able
//! to read exactly what it did to that device, without wading through
//! debug noise. `SAFETY` events go to their own persisted, user-viewable
//! channel.
//!
//! # Redaction
//!
//! Secrets and note contents never reach a log. This is enforced at the call
//! site by [`Redacted`], which renders as `<redacted>` in every format,
//! including `Debug` — so a lazy `{:?}` on a struct containing one cannot leak
//! it.

use std::fmt;

pub mod safety_log;

pub use safety_log::{SafetyEvent, SafetyLogEntry, SafetyOutcome};

/// A value that must never appear in output.
///
/// Wrap API keys, tokens, passwords and note contents in this at the boundary,
/// and the compiler carries the guarantee for you.
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

/// Initialise logging. Call once at startup.
///
/// `RUST_LOG` controls verbosity; the default is `info` plus everything on the
/// safety target, because safety events are never noise.
pub fn init(json: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,marginalia::safety=trace"));

    let builder = tracing_subscriber::fmt().with_env_filter(filter);

    if json {
        builder.json().init();
    } else {
        builder.init();
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
        // The realistic leak is `tracing::info!(?config)`, not a deliberate
        // print of the secret itself.
        let config = Config {
            endpoint: "https://api.zotero.org".into(),
            api_key: Redacted::new("zotero-api-key-abc123".into()),
        };
        let rendered = format!("{config:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("abc123"));
    }

    #[test]
    fn the_secret_is_still_reachable_when_genuinely_needed() {
        let secret = Redacted::new("token".to_string());
        assert_eq!(secret.expose_secret(), "token");
    }
}
