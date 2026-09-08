// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Importing the same thing twice.
//!
//! A repeated `use` line adds nothing and costs a reader a moment working out
//! whether the second one differs from the first. Nothing about it is fatal, so
//! it is reported as a warning — but reported, because the alternative is a
//! file that silently accumulates them.

use super::utils::*;

/// Assert that `code` checks clean and raises no duplicate-import warning.
///
/// The clean check is half the assertion: a program that fails to compile
/// carries no import warning either, so without it the test would pass on a
/// broken fixture and prove nothing.
fn assert_imports_are_not_duplicates(code: &str) {
    let result = crate::utils::miri_check(code);
    let output = result.output();
    assert!(
        result.success,
        "the fixture must compile for its warnings to mean anything:\n{}",
        output
    );
    assert!(
        !output.contains("MER_IMP_004"),
        "these imports are not repeats of one another:\n{}",
        output
    );
}

#[test]
fn test_the_same_import_written_twice_is_reported() {
    let code = r#"
use system.testing.{assert_eq}
use system.testing.{assert_eq}

fn main()
    assert_eq(1, 1)
"#;
    assert_compiler_warning(code, "MER_IMP_004");
}

#[test]
fn test_the_report_names_the_repeated_import() {
    let code = r#"
use system.testing.{assert_eq}
use system.testing.{assert_eq}

fn main()
    assert_eq(1, 1)
"#;
    assert_compiler_warning(code, "system.testing");
}

#[test]
fn test_a_repeated_import_is_a_warning_and_still_runs() {
    let code = r#"
use system.testing.{assert_eq}
use system.testing.{assert_eq}

fn main()
    assert_eq(1, 1)
    println("ran")
"#;
    assert_runs_with_output(code, "ran");
}

#[test]
fn test_a_selection_beside_the_whole_module_is_not_a_duplicate() {
    let code = r#"
use system.testing
use system.testing.{assert_eq}

fn main()
    assert_eq(1, 1)
"#;
    assert_imports_are_not_duplicates(code);
}

#[test]
fn test_the_same_names_in_a_different_order_is_a_duplicate() {
    let code = r#"
use system.testing.{assert_eq, assert_ne}
use system.testing.{assert_ne, assert_eq}

fn main()
    assert_eq(1, 1)
    assert_ne(1, 2)
"#;
    assert_compiler_warning(code, "MER_IMP_004");
}

#[test]
fn test_a_repeated_import_of_a_whole_module_is_reported() {
    let code = r#"
use system.testing
use system.testing

fn main()
    assert_eq(1, 1)
"#;
    assert_compiler_warning(code, "MER_IMP_004");
}

#[test]
fn test_two_modules_are_not_a_duplicate() {
    let code = r#"
use system.testing
use system.math

fn main()
    assert_eq(abs(0 - 1), 1)
"#;
    assert_imports_are_not_duplicates(code);
}
