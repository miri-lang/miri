// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{miri_check, strip_ansi};

/// Run the given source through the type checker, assert failure, and verify that stderr
/// contains every string in `expected_fragments`.
fn assert_example_error(source: &str, expected_fragments: &[&str]) {
    let result = miri_check(source);
    assert!(
        !result.success,
        "Expected program to fail type-checking, but it succeeded.\nOutput:\n{}",
        result.output()
    );
    let clean = strip_ansi(&result.output());
    for fragment in expected_fragments {
        assert!(
            clean.contains(fragment),
            "Error output did not contain expected fragment.\nExpected: '{fragment}'\nActual output:\n{clean}"
        );
    }
}

#[test]
fn example_01_syntax_error() {
    assert_example_error(
        include_str!("../examples/incorrect/01_syntax_error.mi"),
        &["Unexpected Token"],
    );
}

#[test]
fn example_02_undeclared_variable() {
    assert_example_error(
        include_str!("../examples/incorrect/02_undeclared_variable.mi"),
        &["Undefined variable: undefined_var"],
    );
}

#[test]
fn example_03_type_mismatch() {
    assert_example_error(
        include_str!("../examples/incorrect/03_type_mismatch.mi"),
        &["Type mismatch for variable 'x': expected int, got String"],
    );
}

#[test]
fn example_04_reassign_immutable() {
    assert_example_error(
        include_str!("../examples/incorrect/04_reassign_immutable.mi"),
        &["Cannot assign to immutable variable 'x'"],
    );
}

#[test]
fn example_05_wrong_arg_count() {
    assert_example_error(
        include_str!("../examples/incorrect/05_wrong_arg_count.mi"),
        &["Too many positional arguments: expected 2, got 3"],
    );
}

#[test]
fn example_06_non_exhaustive_match() {
    assert_example_error(
        include_str!("../examples/incorrect/06_non_exhaustive_match.mi"),
        &["Non-exhaustive match on Enum 'Color'. Missing variants: Blue"],
    );
}

#[test]
fn example_07_unknown_type() {
    assert_example_error(
        include_str!("../examples/incorrect/07_unknown_type.mi"),
        &["Unknown type: UnknownType"],
    );
}

#[test]
fn example_08_return_type_mismatch() {
    assert_example_error(
        include_str!("../examples/incorrect/08_return_type_mismatch.mi"),
        &["Invalid return type: expected int, got String"],
    );
}
