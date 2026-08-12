//! Explicit user intent.
//!
//! WHY this type exists: the product's central promise is that a sync moves
//! knowledge and only a deliberate click moves a file. That promise is worth
//! nothing if it lives in a comment. `ExplicitUserIntent` is a value that
//! represents "a human confirmed this specific action on this specific
//! document, at this moment" — and the transfer machinery cannot be called
//! without one.
//!
//! Two properties make it meaningful:
//!
//! 1. It is **consumed by value**. An intent authorises exactly one operation;
//!    it cannot be cloned and replayed for a second document.
//! 2. It is **scoped to a document and an action**. An intent to send *this*
//!    paper cannot authorise removing *that* one.

use crate::ids::DocumentId;
use crate::Timestamp;
use serde::Serialize;

/// The user-initiated actions that may touch a device or an external system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserAction {
    SendToRemarkable,
    RemoveFromRemarkable,
    ExportToZotero,
    ApplyTagMapping,
}

/// Proof that a human asked for one specific operation.
///
/// Deliberately **not** `Clone`, **not** `Copy`, and **not** `Deserialize`: it
/// must not be duplicated, and it must not be resurrected from a config file,
/// a queue, or a saved job. The only way to obtain one is [`Self::record`],
/// which is called from the command handler bound to a button the user pressed.
#[derive(Debug, Serialize)]
pub struct ExplicitUserIntent {
    action: UserAction,
    document_id: DocumentId,
    confirmed_at: Timestamp,
}

impl ExplicitUserIntent {
    /// Record that the user confirmed `action` on `document_id`.
    ///
    /// Call sites are limited to interactive command handlers. A scheduler or
    /// background job calling this would be a review-blocking defect — and the
    /// simulator asserts at runtime that no intent appears during an automated
    /// job (see `SyncJobKind::may_be_triggered_by`).
    pub fn record(action: UserAction, document_id: DocumentId, confirmed_at: Timestamp) -> Self {
        Self {
            action,
            document_id,
            confirmed_at,
        }
    }

    pub fn action(&self) -> UserAction {
        self.action
    }

    pub fn document_id(&self) -> &DocumentId {
        &self.document_id
    }

    pub fn confirmed_at(&self) -> Timestamp {
        self.confirmed_at
    }

    /// Whether this intent authorises `action` on `document`.
    pub fn authorises(&self, action: UserAction, document: &DocumentId) -> bool {
        self.action == action && &self.document_id == document
    }

    /// Whether the confirmation is still fresh.
    ///
    /// A stale intent — a dialog left open for an hour while the device
    /// changed, filled up, or was updated — must not authorise a write.
    pub fn is_fresh(&self, now: Timestamp, max_age_secs: i64) -> bool {
        let age = now.signed_duration_since(self.confirmed_at).num_seconds();
        (0..=max_age_secs).contains(&age)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn intent(action: UserAction, doc: &DocumentId) -> ExplicitUserIntent {
        ExplicitUserIntent::record(action, doc.clone(), Utc::now())
    }

    #[test]
    fn intent_is_scoped_to_one_document_and_one_action() {
        let doc_a = DocumentId::new();
        let doc_b = DocumentId::new();
        let i = intent(UserAction::SendToRemarkable, &doc_a);

        assert!(i.authorises(UserAction::SendToRemarkable, &doc_a));
        assert!(!i.authorises(UserAction::SendToRemarkable, &doc_b));
        assert!(!i.authorises(UserAction::RemoveFromRemarkable, &doc_a));
    }

    #[test]
    fn stale_intent_is_rejected() {
        let doc = DocumentId::new();
        let old = Utc::now() - Duration::seconds(600);
        let i = ExplicitUserIntent::record(UserAction::SendToRemarkable, doc, old);
        assert!(!i.is_fresh(Utc::now(), 300));
        assert!(i.is_fresh(Utc::now(), 900));
    }

    #[test]
    fn intent_from_the_future_is_rejected() {
        // Clock skew or a doctored value must not extend an intent's life.
        let doc = DocumentId::new();
        let future = Utc::now() + Duration::seconds(120);
        let i = ExplicitUserIntent::record(UserAction::SendToRemarkable, doc, future);
        assert!(!i.is_fresh(Utc::now(), 300));
    }
}
