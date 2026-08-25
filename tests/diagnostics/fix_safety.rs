// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the fix-safety taxonomy.

use miri::diagnostics::{DiagnosticCode, FixSafety};

// Note: Every diagnostic code must have a fix_safety level specified in the macro
// at src/diagnostics/codes.rs. The macro expands each 7-field tuple into a match arm
// in the fix_safety() method, so a missing fix_safety field is a compile error, not
// a runtime discovery. This property is therefore enforced statically.
// No test is needed: if a code lacks a fix_safety field, the build fails.

#[test]
fn test_tar_codes_are_target_changing() {
    for code in DiagnosticCode::all() {
        if code.area() == "TAR" {
            assert_eq!(
                code.fix_safety(),
                FixSafety::TargetChanging,
                "TAR codes must be target-changing: {}",
                code.as_str()
            );
        }
    }
}

#[test]
fn test_bld_codes_are_not_format_only() {
    // BLD codes relate to build/CLI operations and should not be format-only.
    for code in DiagnosticCode::all() {
        if code.area() == "BLD" {
            assert_ne!(
                code.fix_safety(),
                FixSafety::FormatOnly,
                "BLD codes should not be format-only: {}",
                code.as_str()
            );
        }
    }
}

#[test]
fn test_fix_safety_ordering_is_total() {
    // The ordering is used by the gate to conservatively take the riskier of two labels.
    // This test pins the exact order: FormatOnly < BehaviorPreserving < LocalEdit <
    // ApiChanging < TargetChanging < RequiresHumanReview
    let levels = vec![
        FixSafety::FormatOnly,
        FixSafety::BehaviorPreserving,
        FixSafety::LocalEdit,
        FixSafety::ApiChanging,
        FixSafety::TargetChanging,
        FixSafety::RequiresHumanReview,
    ];

    // Verify the ordering is strict and total
    for i in 0..levels.len() {
        for j in 0..levels.len() {
            if i < j {
                assert!(
                    levels[i] < levels[j],
                    "{:?} should be less than {:?}",
                    levels[i],
                    levels[j]
                );
            } else if i == j {
                assert_eq!(levels[i], levels[j]);
            } else {
                assert!(
                    levels[i] > levels[j],
                    "{:?} should be greater than {:?}",
                    levels[i],
                    levels[j]
                );
            }
        }
    }
}

#[test]
fn test_typ_021_is_target_changing() {
    // GPU Function Host Buffer Mismatch changes GPU residency (let -> gpu let)
    assert_eq!(
        DiagnosticCode::TypGpuFunctionHostBufferMismatch.fix_safety(),
        FixSafety::TargetChanging,
        "MER_TYP_021 must be target-changing because repair changes GPU residency"
    );
}
