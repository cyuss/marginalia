//! The SAFETY audit trail.
//!
//! Every authorisation decision — **granted and denied** — every rollback, and
//! every capability change is recorded here. WHY record the grants too: an
//! audit trail that only shows refusals cannot answer the question a user
//! actually asks, which is "what did this program do to my device?".

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SafetyEvent {
    AuthorizationRequested,
    AuthorizationGranted,
    AuthorizationDenied,
    ProhibitedOperationRefused,
    OperationStarted,
    OperationCompleted,
    OperationFailed,
    RollbackStarted,
    RollbackCompleted,
    RollbackFailed,
    DeviceMarkedReadOnly,
    CapabilityChanged,
    SafeModeChanged,
    FeatureFlagChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SafetyOutcome {
    Granted,
    Denied,
    Succeeded,
    Failed,
    /// Informational entry with no pass/fail sense.
    Noted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyLogEntry {
    pub created_at: DateTime<Utc>,
    pub event: SafetyEvent,
    pub outcome: SafetyOutcome,
    pub operation: Option<String>,
    pub device_id: Option<String>,
    pub document_id: Option<String>,
    /// Machine-readable reason (a `DenialReason` variant name, for instance).
    pub reason: Option<String>,
    /// The sentence shown to the user, kept so the log and the UI agree.
    pub user_message: Option<String>,
}

impl SafetyLogEntry {
    pub fn new(event: SafetyEvent, outcome: SafetyOutcome, at: DateTime<Utc>) -> Self {
        Self {
            created_at: at,
            event,
            outcome,
            operation: None,
            device_id: None,
            document_id: None,
            reason: None,
            user_message: None,
        }
    }

    pub fn with_operation(mut self, op: impl Into<String>) -> Self {
        self.operation = Some(op.into());
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn with_user_message(mut self, message: impl Into<String>) -> Self {
        self.user_message = Some(message.into());
        self
    }

    pub fn with_device(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    pub fn with_document(mut self, document_id: impl Into<String>) -> Self {
        self.document_id = Some(document_id.into());
        self
    }

    /// Whether this entry describes something the user should be shown
    /// proactively rather than only on request.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self.event,
            SafetyEvent::RollbackFailed | SafetyEvent::DeviceMarkedReadOnly
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denial_records_both_the_reason_and_the_user_message() {
        let entry = SafetyLogEntry::new(
            SafetyEvent::AuthorizationDenied,
            SafetyOutcome::Denied,
            Utc::now(),
        )
        .with_operation("UPLOAD_DOCUMENT")
        .with_reason("FIRMWARE_UNKNOWN")
        .with_user_message("Your reMarkable's firmware has not been tested.");

        assert_eq!(entry.reason.as_deref(), Some("FIRMWARE_UNKNOWN"));
        assert!(entry.user_message.is_some());
        assert!(!entry.is_blocking());
    }

    #[test]
    fn a_failed_rollback_is_blocking() {
        let entry = SafetyLogEntry::new(
            SafetyEvent::RollbackFailed,
            SafetyOutcome::Failed,
            Utc::now(),
        );
        assert!(
            entry.is_blocking(),
            "a failed rollback must interrupt the user, not sit in a log"
        );
    }

    #[test]
    fn entries_serialise_for_the_activity_view() {
        let entry = SafetyLogEntry::new(
            SafetyEvent::AuthorizationGranted,
            SafetyOutcome::Granted,
            Utc::now(),
        )
        .with_operation("UPLOAD_DOCUMENT");
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("AUTHORIZATION_GRANTED"));
    }
}
