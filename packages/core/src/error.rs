//! Typed domain errors.
//!
//! WHY typed: every user-facing error must be able to answer four questions —
//! what happened, what was affected, is my data safe, what can I do. A `String`
//! cannot answer those; an enum with structured fields can, and the UI can
//! render each variant properly instead of dumping a message.

use crate::document::{DocumentEvent, DocumentState};
use thiserror::Error;

/// An attempt to drive a state machine along an edge that does not exist.
///
/// Illegal transitions are never silently ignored: a no-op would hide a real
/// bug behind a UI that looks like it worked.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("illegal transition: {from:?} cannot handle {event:?}")]
pub struct IllegalTransition {
    pub from: DocumentState,
    pub event: DocumentEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    #[error(transparent)]
    IllegalTransition(#[from] IllegalTransition),

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("invalid checksum format: {0}")]
    InvalidChecksum(String),

    #[error("{field} is required but was empty")]
    MissingField { field: &'static str },

    #[error("invalid firmware version: {0}")]
    InvalidFirmware(String),

    #[error("page number must be 1-based, got {0}")]
    InvalidPageNumber(i64),
}

/// What the user should be told, and what they can do about it.
///
/// Produced from a domain error at the presentation boundary so that error
/// copy lives with the domain knowledge rather than scattered across the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFacingError {
    /// What happened, in one sentence, in plain language.
    pub what_happened: String,
    /// What was touched.
    pub what_was_affected: String,
    /// Whether the user's data is intact. Almost always `true` — say so.
    pub data_is_safe: bool,
    /// The concrete next step, if there is one.
    pub remediation: Option<String>,
}

impl UserFacingError {
    pub fn safe(
        what_happened: impl Into<String>,
        what_was_affected: impl Into<String>,
        remediation: Option<String>,
    ) -> Self {
        Self {
            what_happened: what_happened.into(),
            what_was_affected: what_was_affected.into(),
            data_is_safe: true,
            remediation,
        }
    }
}
