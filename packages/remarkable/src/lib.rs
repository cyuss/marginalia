//! The device boundary.
//!
//! **Phase 0 ships no transport.** There is no USB code, no SSH code, and no
//! HTTP client here. What exists is the shape of the boundary — the port every
//! future transport must implement, and the capability layer that decides what
//! a given device is allowed to do.
//!
//! The ports are designed so that the safety rules are carried by the function
//! signatures: reads take no grant, writes take a [`WriteGrant`] that only
//! `marginalia-safety` can mint.
//!
//! There are two of them — [`DeviceIntrospection`] for an app running on the
//! device, [`RemoteDeviceTransport`] for the desktop companion talking to one.

pub mod capability;
pub mod compatibility;
pub mod provider;

pub use capability::CapabilityResolver;
pub use compatibility::{CompatibilityMatrix, MatrixEntry};
pub use provider::{
    DeviceIntrospection, DeviceProviderError, RemoteDeviceTransport, RemoteDocument, ValidatedPdf,
};
