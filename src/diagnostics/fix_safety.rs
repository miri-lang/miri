// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Fix-safety taxonomy for diagnostic repairs.
//!
//! Every diagnostic and every repair carries one label describing the risk level
//! of applying the repair. This allows the CLI to make intelligent decisions about
//! whether a repair is safe to apply automatically or should be surfaced to a human.

use serde::{Deserialize, Serialize};

/// Safety classification for a repair.
///
/// The label is the floor — the minimum risk level for any repair of that condition.
/// A repair's actual risk may be equal to or higher than the code's floor.
/// The effective label of a diagnostic is the maximum (riskier) of the code's floor
/// and the repair's actual safety level (if a repair exists).
///
/// # Ordering
///
/// The variants are ordered from least risky to most risky:
/// FormatOnly < BehaviorPreserving < LocalEdit < ApiChanging < TargetChanging < RequiresHumanReview.
/// This total order allows the gate to conservatively take the riskier of two labels
/// when combining a code's floor and a repair's actual safety using the `join` method.
/// Note: ApiChanging and TargetChanging are not comparable as kinds of risk (they affect
/// different dimensions); their relative order is only a reporting tie-break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixSafety {
    /// Whitespace / comment-only change. Always safe to auto-apply.
    FormatOnly = 0,
    /// Semantically equivalent rewrite. Safe for trusted agents to auto-apply.
    BehaviorPreserving = 1,
    /// Confined to the current function / file. Safe for IDE quick-fix flows.
    LocalEdit = 2,
    /// Changes a public signature, exported name, or package surface.
    /// Must be surfaced to a human.
    ApiChanging = 3,
    /// Changes target support, required capabilities, scalar width, or GPU residency.
    /// Must be surfaced to a human — an auto-applied repair here silently changes
    /// where and how the program runs.
    TargetChanging = 4,
    /// Ambiguous or risky; show the plan but never apply automatically.
    RequiresHumanReview = 5,
}

impl FixSafety {
    /// Get the wire string for this safety level (e.g., "local-edit").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FormatOnly => "format-only",
            Self::BehaviorPreserving => "behavior-preserving",
            Self::LocalEdit => "local-edit",
            Self::ApiChanging => "api-changing",
            Self::TargetChanging => "target-changing",
            Self::RequiresHumanReview => "requires-human-review",
        }
    }

    /// Return true if this repair can be auto-applied without user interaction.
    ///
    /// Auto-applicable repairs are those that do not require human review:
    /// format-only, behavior-preserving, and local-edit. The riskier labels
    /// (api-changing, target-changing, requires-human-review) must be
    /// explicitly approved by the user.
    pub fn is_auto_applicable(&self) -> bool {
        matches!(
            self,
            Self::FormatOnly | Self::BehaviorPreserving | Self::LocalEdit
        )
    }

    /// Return the riskier of two safety levels.
    ///
    /// Used to compute the effective label when both a code ceiling and a repair
    /// label exist. Returns the maximum of the two using the derived Ord impl.
    pub fn join(&self, other: FixSafety) -> FixSafety {
        (*self).max(other)
    }
}

impl std::fmt::Display for FixSafety {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for FixSafety {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "format-only" => Ok(Self::FormatOnly),
            "behavior-preserving" => Ok(Self::BehaviorPreserving),
            "local-edit" => Ok(Self::LocalEdit),
            "api-changing" => Ok(Self::ApiChanging),
            "target-changing" => Ok(Self::TargetChanging),
            "requires-human-review" => Ok(Self::RequiresHumanReview),
            _ => Err(format!("unknown fix safety level: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_strings_are_unique() {
        let mut names = vec![
            FixSafety::FormatOnly.as_str(),
            FixSafety::BehaviorPreserving.as_str(),
            FixSafety::LocalEdit.as_str(),
            FixSafety::ApiChanging.as_str(),
            FixSafety::TargetChanging.as_str(),
            FixSafety::RequiresHumanReview.as_str(),
        ];
        let orig_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), orig_len, "wire strings must be unique");
    }

    #[test]
    fn test_round_trip_parse() {
        let levels = vec![
            FixSafety::FormatOnly,
            FixSafety::BehaviorPreserving,
            FixSafety::LocalEdit,
            FixSafety::ApiChanging,
            FixSafety::TargetChanging,
            FixSafety::RequiresHumanReview,
        ];
        for level in levels {
            let wire = level.as_str();
            let parsed = wire.parse::<FixSafety>().expect("should parse");
            assert_eq!(parsed, level, "round-trip should preserve identity");
        }
    }

    #[test]
    fn test_auto_applicable_boundary() {
        assert!(FixSafety::FormatOnly.is_auto_applicable());
        assert!(FixSafety::BehaviorPreserving.is_auto_applicable());
        assert!(FixSafety::LocalEdit.is_auto_applicable());
        assert!(!FixSafety::ApiChanging.is_auto_applicable());
        assert!(!FixSafety::TargetChanging.is_auto_applicable());
        assert!(!FixSafety::RequiresHumanReview.is_auto_applicable());
    }

    #[test]
    fn test_join_returns_riskier() {
        assert_eq!(
            FixSafety::LocalEdit.join(FixSafety::ApiChanging),
            FixSafety::ApiChanging
        );
        assert_eq!(
            FixSafety::BehaviorPreserving.join(FixSafety::TargetChanging),
            FixSafety::TargetChanging
        );
        assert_eq!(
            FixSafety::FormatOnly.join(FixSafety::FormatOnly),
            FixSafety::FormatOnly
        );
        assert_eq!(
            FixSafety::LocalEdit.join(FixSafety::ApiChanging),
            FixSafety::ApiChanging.join(FixSafety::LocalEdit),
            "join is commutative"
        );
    }

    #[test]
    fn test_serde_and_as_str_agree() {
        let levels = vec![
            FixSafety::FormatOnly,
            FixSafety::BehaviorPreserving,
            FixSafety::LocalEdit,
            FixSafety::ApiChanging,
            FixSafety::TargetChanging,
            FixSafety::RequiresHumanReview,
        ];
        for level in levels {
            let as_str_value = level.as_str();
            let serialized = serde_json::to_value(level).expect("should serialize");
            let serde_value = serialized.as_str().expect("serde value should be a string");
            assert_eq!(
                as_str_value, serde_value,
                "as_str() and serde must produce the same wire string for {:?}",
                level
            );
        }
    }
}
