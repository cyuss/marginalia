//! # Marginalia — domain core
//!
//! This crate is pure. It has no filesystem, network, database or device
//! access, and it depends on no other crate in the workspace. Everything here
//! is a value, a state machine, or a total function over them.
//!
//! That purity is what makes the safety strategy work: the rules that protect a
//! user's reMarkable are expressed as types and transitions that can be tested
//! exhaustively without a device in the room.

pub mod annotation;
pub mod checksum;
pub mod clock;
pub mod credentials;
pub mod device;
pub mod document;
pub mod error;
pub mod ids;
pub mod intent;
pub mod secret;
pub mod sync;
pub mod tag;
pub mod zotero;

pub use checksum::Checksum;
pub use clock::{Clock, FixedClock, SteppingClock, SystemClock, SYSTEM_CLOCK};
pub use credentials::{CredentialError, CredentialKey, CredentialStore};
pub use error::{CoreError, IllegalTransition};
pub use ids::*;
pub use secret::Redacted;

/// Timestamps are UTC everywhere. Local time is a presentation concern.
pub type Timestamp = chrono::DateTime<chrono::Utc>;
