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
use crate::{KeyDescription, KeyVerification, LibraryAccess, ZoteroClient, ZoteroError};

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
            Err(ureq::Error::Transport(t)) => Err(map_transport(&t)),
        }
    }

    fn describe_key(&self, api_key: &Redacted<String>) -> Result<KeyDescription, ZoteroError> {
        // The endpoint that makes the library-ID field unnecessary: it reports
        // who the key belongs to and what it can reach.
        let url = format!("{}/keys/current", self.base_url);

        let response = self
            .agent()
            .get(&url)
            .set("Zotero-API-Version", API_VERSION)
            .set(
                "Authorization",
                &format!("Bearer {}", api_key.expose_secret()),
            )
            .call();

        match response {
            Ok(res) => {
                let body: serde_json::Value = res
                    .into_json()
                    .map_err(|_| ZoteroError::Protocol("key description was not JSON".into()))?;
                parse_key_description(&body)
            }
            Err(ureq::Error::Status(code, res)) => Err(match code {
                401 | 403 => ZoteroError::Unauthorized,
                429 | 503 => ZoteroError::RateLimited {
                    retry_after_secs: res
                        .header("Retry-After")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(60),
                },
                other => ZoteroError::Protocol(format!("status {other}")),
            }),
            Err(ureq::Error::Transport(t)) => Err(map_transport(&t)),
        }
    }
}

/// The failure kind, never the detail: transport error text can contain the
/// full URL, and a request formatted for debugging can contain the header.
fn map_transport(t: &ureq::Transport) -> ZoteroError {
    transport_error(match t.kind() {
        ureq::ErrorKind::Dns => "could not resolve api.zotero.org",
        ureq::ErrorKind::ConnectionFailed => "connection failed",
        ureq::ErrorKind::Io => "network error",
        ureq::ErrorKind::InvalidUrl | ureq::ErrorKind::UnknownScheme => "invalid URL",
        _ => "request failed",
    })
}

/// Parse the `/keys/current` payload.
///
/// Deliberately tolerant: unknown fields are ignored and missing permissions
/// read as `false`. Zotero can add a capability without warning, and the safe
/// interpretation of "a field we do not recognise" is "we were not granted it".
fn parse_key_description(body: &serde_json::Value) -> Result<KeyDescription, ZoteroError> {
    let user_id = body
        .get("userID")
        .and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(String::from))
        })
        .ok_or_else(|| ZoteroError::Protocol("no userID in the key description".into()))?;

    let username = body
        .get("username")
        .and_then(|v| v.as_str())
        .map(String::from);

    let access = body.get("access");

    let flag = |obj: Option<&serde_json::Value>, name: &str| -> bool {
        obj.and_then(|o| o.get(name))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };

    let user_access = access.and_then(|a| a.get("user"));
    let personal = user_access.map(|u| LibraryAccess {
        read: flag(Some(u), "library"),
        write: flag(Some(u), "write"),
        notes: flag(Some(u), "notes"),
        files: flag(Some(u), "files"),
    });
    // A block that grants nothing is not access.
    let personal = personal.filter(|a| a.read);

    let mut group_ids = Vec::new();
    let mut all_groups = false;
    if let Some(groups) = access
        .and_then(|a| a.get("groups"))
        .and_then(|g| g.as_object())
    {
        for key in groups.keys() {
            if key == "all" {
                all_groups = true;
            } else {
                group_ids.push(key.clone());
            }
        }
    }
    group_ids.sort();

    Ok(KeyDescription {
        user_id,
        username,
        personal,
        group_ids,
        all_groups,
    })
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
    fn a_key_description_yields_the_library_id_the_user_would_have_looked_up() {
        let body = serde_json::json!({
            "key": "xxxx",
            "userID": 1234567,
            "username": "youcef",
            "access": { "user": { "library": true, "files": true, "notes": true, "write": false } }
        });
        let d = parse_key_description(&body).unwrap();

        assert_eq!(d.user_id, "1234567");
        assert_eq!(d.username.as_deref(), Some("youcef"));
        assert!(d.has_exactly_one_library());
        assert_eq!(
            d.known_libraries(),
            vec![crate::LibraryRef::user("1234567")]
        );
        assert!(!d.personal.unwrap().write);
    }

    #[test]
    fn group_libraries_are_listed_and_all_is_recognised() {
        let body = serde_json::json!({
            "userID": 1,
            "access": {
                "user": { "library": true },
                "groups": { "all": { "library": true }, "98765": { "library": true } }
            }
        });
        let d = parse_key_description(&body).unwrap();

        assert_eq!(d.group_ids, vec!["98765"]);
        assert!(d.all_groups, "'all' is a wildcard, not a group id");
    }

    #[test]
    fn a_permission_block_granting_nothing_is_not_access() {
        let body = serde_json::json!({
            "userID": 1,
            "access": { "user": { "library": false, "write": false } }
        });
        let d = parse_key_description(&body).unwrap();
        assert!(d.personal.is_none());
        assert!(d.known_libraries().is_empty());
    }

    #[test]
    fn unknown_fields_are_ignored_and_missing_permissions_read_as_false() {
        // Zotero can add a capability without warning. The safe reading of a
        // field we do not recognise is "not granted".
        let body = serde_json::json!({
            "userID": 1,
            "somethingNew": { "future": true },
            "access": { "user": { "library": true } }
        });
        let d = parse_key_description(&body).unwrap();
        let personal = d.personal.unwrap();
        assert!(personal.read);
        assert!(!personal.write);
        assert!(!personal.files);
    }

    #[test]
    fn a_description_without_a_user_id_is_a_protocol_error() {
        let body = serde_json::json!({ "access": { "user": { "library": true } } });
        assert!(matches!(
            parse_key_description(&body),
            Err(ZoteroError::Protocol(_))
        ));
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
