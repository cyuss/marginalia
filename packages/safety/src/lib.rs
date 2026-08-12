//! # Marginalia — safety layer
//!
//! This crate is the only place in the codebase that can authorise a change to
//! a physical reMarkable.
//!
//! The enforcement is not a convention and not a code review checklist. A
//! device write function's signature requires a [`WriteGrant`], and a
//! `WriteGrant` contains a field of a private type that no other module can
//! name. Code that tries to write to a device without going through
//! [`SafetyManager::authorize`] does not compile.
//!
//! ```compile_fail
//! # use marginalia_safety::grant::WriteGrant;
//! // There is no public constructor, and the sealing field is private:
//! let grant = WriteGrant { /* ... */ };  // error[E0451]: field `seal` is private
//! ```
//!
//! See `docs/safety/SAFETY_MODEL.md` and `docs/safety/DEVICE_WRITE_POLICY.md`.

pub mod classification;
pub mod flags;
pub mod grant;
pub mod manager;
pub mod snapshot;

pub use classification::{DeviceOperation, SafetyClass};
pub use flags::{FeatureFlag, FeatureFlagManager};
pub use grant::WriteGrant;
pub use manager::{Authorization, DenialReason, OperationRequest, Preconditions, SafetyManager};
pub use snapshot::{SafetySnapshot, SnapshotStatus};
