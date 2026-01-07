// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Control flow integration tests.
//!
//! Note: Conditional expressions with return values are not yet implemented in
//! the Cranelift backend. These tests use assert_valid for type-checking only.

use crate::test_utils::{assert_invalid, assert_valid};

// =============================================================================
// Conditional Expressions (type-check only for now)
// =============================================================================

#[test]
fn test_simple_if_else_typecheck() {
    assert_valid(
        r#"
fn main()
    let x = 10
    if x > 5
        print("greater")
    else
        print("smaller")
"#,
    );
}

#[test]
fn test_if_else_chain_typecheck() {
    assert_valid(
        r#"
fn main()
    let x = 50
    if x < 10
        print("small")
    else if x < 30
        print("medium")
    else
        print("large")
"#,
    );
}

#[test]
fn test_nested_conditionals_typecheck() {
    assert_valid(
        r#"
fn main()
    let a = 5
    let b = 10
    if a > 0
        if b > 5
            print("both positive")
"#,
    );
}

// =============================================================================
// Loop tests (type-check only)
// =============================================================================

#[test]
fn test_while_loop_typecheck() {
    assert_valid(
        r#"
fn main()
    var i = 0
    while i < 10
        print(i)
        i = i + 1
"#,
    );
}

#[test]
fn test_for_loop_typecheck() {
    assert_valid(
        r#"
fn main()
    for i in [1, 2, 3]
        print(i)
"#,
    );
}

#[test]
fn test_break_continue_typecheck() {
    assert_valid(
        r#"
fn main()
    var i = 0
    while i < 10
        i = i + 1
        if i == 5
            continue
        if i == 8
            break
        print(i)
"#,
    );
}

// =============================================================================
// Error cases
// =============================================================================

#[test]
fn test_break_outside_loop_error() {
    assert_invalid(
        r#"
fn main()
    break
"#,
        &["Break statement outside of loop"],
    );
}

#[test]
fn test_continue_outside_loop_error() {
    assert_invalid(
        r#"
fn main()
    continue
"#,
        &["Continue statement outside of loop"],
    );
}
