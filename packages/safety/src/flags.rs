//! Feature flags.
//!
//! WHY every experimental flag defaults OFF and cannot be turned on by
//! inference: a flag is the difference between "Marginalia does the safe thing"
//! and "Marginalia does the interesting thing". Interesting is opt-in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureFlag {
    /// The four whitelisted device writes. ON once Phase 3 ships; the
    /// capability layer still has the final say.
    SafeDocumentTransfer,
    NativePdfAnnotations,
    BidirectionalTagSync,

    // ── Experimental. All default OFF. ───────────────────────────────────
    RemarkableCompanion,
    ExperimentalRmUi,
    StickyNotesRm,
    SideNotesRm,
    CommandPaletteRm,
}

impl FeatureFlag {
    /// The default state of every flag.
    ///
    /// Experimental features are OFF. This is a `const fn` over an exhaustive
    /// match so that adding a flag forces an explicit decision rather than
    /// inheriting someone else's default.
    pub const fn default_enabled(self) -> bool {
        match self {
            // Enabled once the corresponding phase has shipped and been tested.
            FeatureFlag::SafeDocumentTransfer => false,
            FeatureFlag::NativePdfAnnotations => false,
            FeatureFlag::BidirectionalTagSync => false,

            // Experimental — never on by default.
            FeatureFlag::RemarkableCompanion => false,
            FeatureFlag::ExperimentalRmUi => false,
            FeatureFlag::StickyNotesRm => false,
            FeatureFlag::SideNotesRm => false,
            FeatureFlag::CommandPaletteRm => false,
        }
    }

    /// Whether enabling this flag should require an extra confirmation with a
    /// plain-language warning.
    pub const fn is_experimental(self) -> bool {
        matches!(
            self,
            FeatureFlag::RemarkableCompanion
                | FeatureFlag::ExperimentalRmUi
                | FeatureFlag::StickyNotesRm
                | FeatureFlag::SideNotesRm
                | FeatureFlag::CommandPaletteRm
        )
    }

    pub const ALL: [FeatureFlag; 8] = [
        FeatureFlag::SafeDocumentTransfer,
        FeatureFlag::NativePdfAnnotations,
        FeatureFlag::BidirectionalTagSync,
        FeatureFlag::RemarkableCompanion,
        FeatureFlag::ExperimentalRmUi,
        FeatureFlag::StickyNotesRm,
        FeatureFlag::SideNotesRm,
        FeatureFlag::CommandPaletteRm,
    ];
}

#[derive(Debug, Clone, Default)]
pub struct FeatureFlagManager {
    overrides: BTreeMap<FeatureFlag, bool>,
}

impl FeatureFlagManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from persisted user settings. Unknown/absent flags fall back to
    /// their compiled default, never to "on".
    pub fn with_overrides(overrides: BTreeMap<FeatureFlag, bool>) -> Self {
        Self { overrides }
    }

    pub fn is_enabled(&self, flag: FeatureFlag) -> bool {
        self.overrides
            .get(&flag)
            .copied()
            .unwrap_or_else(|| flag.default_enabled())
    }

    pub fn set(&mut self, flag: FeatureFlag, enabled: bool) {
        self.overrides.insert(flag, enabled);
    }

    /// Return every flag to its compiled default. Used by the "reset to safe
    /// defaults" action and after a device reports unexpected behaviour.
    pub fn reset_to_defaults(&mut self) {
        self.overrides.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Safety test S12, at the configuration level.
    #[test]
    fn every_experimental_flag_is_off_by_default() {
        let flags = FeatureFlagManager::new();
        for flag in FeatureFlag::ALL {
            if flag.is_experimental() {
                assert!(
                    !flags.is_enabled(flag),
                    "{flag:?} is experimental and must default to OFF"
                );
            }
        }
    }

    #[test]
    fn no_flag_at_all_is_on_by_default() {
        // Phase 0 ships with nothing enabled; each phase turns on its own flag
        // deliberately, after its safety tests pass.
        let flags = FeatureFlagManager::new();
        for flag in FeatureFlag::ALL {
            assert!(!flags.is_enabled(flag), "{flag:?} must default to OFF");
        }
    }

    #[test]
    fn an_absent_override_falls_back_to_the_compiled_default() {
        let flags = FeatureFlagManager::with_overrides(BTreeMap::new());
        assert!(!flags.is_enabled(FeatureFlag::RemarkableCompanion));
    }

    #[test]
    fn reset_returns_to_safe_defaults() {
        let mut flags = FeatureFlagManager::new();
        flags.set(FeatureFlag::ExperimentalRmUi, true);
        assert!(flags.is_enabled(FeatureFlag::ExperimentalRmUi));

        flags.reset_to_defaults();
        assert!(!flags.is_enabled(FeatureFlag::ExperimentalRmUi));
    }
}
