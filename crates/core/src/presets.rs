//! Presets only add gates; owner approval is never optional.
use serde::{Deserialize, Serialize};
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    Minimal,
    Standard,
    Strict,
}
impl Preset {
    pub fn research(self) -> bool {
        self == Self::Strict
    }
    pub fn milestone(self, tagged: bool) -> bool {
        self == Self::Strict || (self == Self::Standard && tagged)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_only_add_gates() {
        assert!(!Preset::Minimal.milestone(true));
        assert!(!Preset::Standard.milestone(false));
        assert!(Preset::Standard.milestone(true));
        assert!(Preset::Strict.milestone(false));
        assert!(Preset::Strict.research());
        assert!(!Preset::Standard.research());
    }
}
