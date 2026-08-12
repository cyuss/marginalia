//! The real HTTP client. Behind the `http` feature.
//!
//! Only one operation lives here so far: verifying a key. That is what the
//! setup flow needs, and it is the smallest possible request — which makes it
//! the right thing to build first, because it exercises DNS, TLS, auth and
//! error mapping without touching the user's library.
//!
//! # What this deliberately cannot do
//!
//! It implements [`ZoteroClient`], and that trait has no method returning file
//! bytes. There is no code path here that downloads a PDF, and adding one would
//! mean changing the trait — which is the point at which someone has to think
//! about invariants 8 and 9.

use marginalia_core::secret::Redacted;

use crate::credentials::ZoteroCredentials;
use crate::{KeyVerification, ZoteroClient, ZoteroError};

const API_BASE: &str = "https://api.zotero.org";
/// Zotero's API version. Pinned: an unversioned request gets whatever is
/// current, which is how a working integration breaks silently.
const API_VERSION: &str = "3";

pub struct HttpZoteroClient {
    base_url: String,
    timeout_secs: u64,
}

impl Default for HttpZoteroClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpZoteroClient {
    pub fn new() -> Self {
        Self {
            base_url: API_BASE.to_string(),
            // A device on flaky wifi should fail in seconds, not hang a setup
            // screen indefinitely.
            timeout_secs: 20,
        }
    }

    /// Point at a different host. For tests against a local stub — never for
    /// production configuration, because a user-supplied API base is a way to
    /// send someone's key somewhere it should not go.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            timeout_secs: 5,
        }
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
    }
}

/// Map a transport failure to a domain error **without** including the request
/// in the message.
///
/// A naive `e.to_string()` here would be how a key ends up in a log file: the
/// error text can contain the full URL, and a header-bearing request formatted
/// for debugging can contain the header.
fn transport_error(kind: &str) -> ZoteroError {
    ZoteroError::Network(kind.to_string())
}

impl ZoteroClient for HttpZoteroClient {
    fn verify(&self, credentials: &ZoteroCredentials) -> Result<KeyVerification, ZoteroError> {
        let library = credentials.library();
        // The cheapest request that proves the key can read this library:
        // one item, or none at all if the library is empty.
        let url = format!("{}{}/items?limit=1", self.base_url, library.base_path());

        let response = self
            .agent()
            .get(&url)
            .set("Zotero-API-Version", API_VERSION)
            // Zotero accepts the key in a header; never as a query parameter,
            // which would put it in server logs and browser history.
            .set(
                "Authorization",
                &format!("Bearer {}", credentials.api_key().expose_secret()),
            )
            .call();

        match response {
            Ok(res) => {
                let username = res
                    .header("Last-Modified-Version")
                    .map(|_| ())
                    .and(None::<String>);
                // A 200 means the key can read this library. Write capability
                // is not implied and must not be assumed: it is reported
                // separately by Zotero, and until we read it explicitly we
                // claim only what we know.
                let _ = username;
                Ok(KeyVerification {
                    username: None,
                    user_id: Some(library.id.clone()),
                    grants_read: true,
                    grants_write: false,
                })
            }
            Err(ureq::Error::Status(code, res)) => Err(match code {
                401 | 403 => {
                    // Zotero uses both; the distinction that matters to the user
                    // is "bad key" vs "key cannot see this library", and the
                    // body does not reliably tell us which.
                    if code == 401 {
                        ZoteroError::Unauthorized
                    } else {
                        ZoteroError::Forbidden
                    }
                }
                404 => ZoteroError::LibraryNotFound,
                429 | 503 => ZoteroError::RateLimited {
                    retry_after_secs: res
                        .header("Retry-After")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(60),
                },
                other => ZoteroError::Protocol(format!("status {other}")),
            }),
            Err(ureq::Error::Transport(t)) => Err(transport_error(
                // The kind, not the detail: the detail can contain the URL.
                match t.kind() {
                    ureq::ErrorKind::Dns => "could not resolve api.zotero.org",
                    ureq::ErrorKind::ConnectionFailed => "connection failed",
                    ureq::ErrorKind::Io => "network error",
                    ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme => "invalid URL",
                    _ => "request failed",
                },
            )),
        }
    }
}

/// Build a credential from a secret and a library, for callers that hold the
/// two separately (the setup screen, and the integration tests).
pub fn credentials_from(
    api_key: Redacted<String>,
    library: crate::credentials::LibraryRef,
) -> ZoteroCredentials {
    ZoteroCredentials::new(api_key, library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::LibraryRef;

    #[test]
    fn the_api_version_is_pinned() {
        // An unversioned request gets whatever is current, which is how a
        // working integration breaks silently one morning.
        assert_eq!(API_VERSION, "3");
    }

    #[test]
    fn transport_errors_carry_no_request_detail() {
        // The realistic leak is `e.to_string()` on an error whose text contains
        // the full URL and, in some clients, the headers.
        let e = transport_error("connection failed");
        let rendered = format!("{e}");
        assert!(!rendered.contains("Bearer"));
        assert!(!rendered.contains("api.zotero.org/users"));
    }

    #[test]
    fn a_verification_url_targets_the_right_library() {
        let client = HttpZoteroClient::with_base_url("http://localhost:1");
        let creds = credentials_from(
            Redacted::new("aaaaaaaaaaaaaaaaaaaaaaaa".into()),
            LibraryRef::group("777"),
        );
        // No network here; this asserts the path construction that the request
        // would use.
        assert_eq!(creds.library().base_path(), "/groups/777");
        assert_eq!(client.base_url, "http://localhost:1");
    }

    #[test]
    fn an_unreachable_host_is_a_network_error_not_a_rejection() {
        // Distinguishing these matters: one means "check your wifi", the other
        // means "your key is wrong". Port 1 is reliably closed.
        let client = HttpZoteroClient::with_base_url("http://127.0.0.1:1");
        let creds = credentials_from(
            Redacted::new("aaaaaaaaaaaaaaaaaaaaaaaa".into()),
            LibraryRef::user("12345"),
        );

        let err = client.verify(&creds).unwrap_err();
        assert!(
            err.is_transient(),
            "an unreachable host must be transient, got {err:?}"
        );
    }
}
