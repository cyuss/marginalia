//! Deterministic fault injection.
//!
//! Faults are addressed by (call name, occurrence number) so a test can say
//! "fail the second upload" and get exactly that, every run, on every machine.
//! No randomness, no timing dependence — a flaky safety test is worse than no
//! safety test, because it teaches people to re-run until green.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The cable is pulled mid-operation.
    ConnectionLost,
    /// The document lands, but the bytes are wrong.
    TruncatedWrite,
    /// The document lands intact but reports a different checksum.
    ChecksumMismatch,
    /// The upload reports success; the document is not actually there.
    ListingOmitsUploadedDoc,
    /// The device refuses.
    PermissionDenied,
    /// Cleanup itself fails — the case that must degrade to read-only rather
    /// than trigger an improvised second attempt.
    RollbackFails,
}

#[derive(Debug, Clone, Default)]
pub struct FaultScript {
    /// (call name, occurrence) → fault
    faults: BTreeMap<(&'static str, u32), Fault>,
}

impl FaultScript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inject `fault` on the `occurrence`-th call to `call` (1-based).
    pub fn on(mut self, call: &'static str, occurrence: u32, fault: Fault) -> Self {
        self.faults.insert((call, occurrence), fault);
        self
    }

    /// Convenience: fail the first call.
    pub fn once(call: &'static str, fault: Fault) -> Self {
        Self::new().on(call, 1, fault)
    }

    pub fn fault_for(&self, call: &'static str, occurrence: u32) -> Option<Fault> {
        self.faults.get(&(call, occurrence)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_fire_on_the_specified_occurrence_only() {
        let script = FaultScript::new().on("upload_document", 2, Fault::ConnectionLost);
        assert_eq!(script.fault_for("upload_document", 1), None);
        assert_eq!(
            script.fault_for("upload_document", 2),
            Some(Fault::ConnectionLost)
        );
        assert_eq!(script.fault_for("upload_document", 3), None);
    }

    #[test]
    fn an_empty_script_never_fires() {
        let script = FaultScript::new();
        assert_eq!(script.fault_for("upload_document", 1), None);
    }
}
