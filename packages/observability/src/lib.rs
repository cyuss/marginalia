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

pub mod safety_log;

pub use safety_log::{SafetyEvent, SafetyLogEntry, SafetyOutcome};

/// Re-exported from the domain core.
///
/// It moved there because a secret is a domain concern and the credential port
/// needs to name one, and the core cannot depend on this crate. The guarantee
/// is unchanged: `<redacted>` in both `Display` and `Debug`.
pub use marginalia_core::secret::Redacted;

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
    struct SyncConfig {
        endpoint: String,
        api_key: Redacted<String>,
    }

    /// The redaction contract matters most at a logging call site, so this
    /// crate keeps a test for it even though the type now lives in core.
    #[test]
    fn a_struct_logged_with_debug_does_not_leak_its_secret() {
        let config = SyncConfig {
            endpoint: "https://api.zotero.org".into(),
            api_key: Redacted::new("zotero-api-key-abc123".into()),
        };
        let rendered = format!("{config:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("abc123"));
    }
}
