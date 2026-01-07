// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Integration tests that verify full compilation and type-checking.
//!
//! These tests use `assert_valid` to verify type-checking passes,
//! and `assert_compiles` to verify full compilation works.

use crate::test_utils::{assert_compiles, assert_invalid, assert_valid};

// =============================================================================
// Basic Arithmetic - These actually compile and run!
// =============================================================================

#[test]
fn test_return_constant() {
    assert_compiles(
        r#"
fn main() int
    42
"#,
    );
}

#[test]
fn test_addition() {
    assert_compiles(
        r#"
fn main() int
    let a = 10
    let b = 20
    a + b
"#,
    );
}

#[test]
fn test_subtraction() {
    assert_compiles(
        r#"
fn main() int
    let a = 100
    let b = 42
    a - b
"#,
    );
}

#[test]
fn test_multiplication() {
    assert_compiles(
        r#"
fn main() int
    let a = 6
    let b = 7
    a * b
"#,
    );
}

#[test]
fn test_division() {
    assert_compiles(
        r#"
fn main() int
    let a = 100
    let b = 5
    a / b
"#,
    );
}

#[test]
fn test_modulo() {
    assert_compiles(
        r#"
fn main() int
    let a = 17
    let b = 5
    a % b
"#,
    );
}

#[test]
fn test_complex_arithmetic() {
    assert_compiles(
        r#"
fn main() int
    let a = 10
    let b = 20
    let c = 30
    (a + b) * c - 100
"#,
    );
}

#[test]
fn test_unary_negation() {
    assert_compiles(
        r#"
fn main() int
    let a = 42
    -a
"#,
    );
}

#[test]
fn test_multiple_variables() {
    assert_compiles(
        r#"
fn main() int
    let a = 1
    let b = 2
    let c = 3
    let d = 4
    let e = 5
    a + b + c + d + e
"#,
    );
}

#[test]
fn test_mutable_variable() {
    assert_compiles(
        r#"
fn main() int
    var x = 0
    x = x + 10
    x = x + 20
    x
"#,
    );
}

#[test]
fn test_explicit_return() {
    assert_compiles(
        r#"
fn main() int
    return 100
"#,
    );
}

// =============================================================================
// Type Checking tests (use assert_valid - not compiled yet)
// =============================================================================

#[test]
fn test_type_mismatch_error() {
    assert_invalid(
        r#"
fn main()
    let a int = "string"
"#,
        &["Type mismatch"],
    );
}

#[test]
fn test_immutable_reassignment_error() {
    assert_invalid(
        r#"
fn main()
    let a = 10
    a = 20
"#,
        &["Cannot assign to immutable variable 'a'"],
    );
}

#[test]
fn test_undefined_variable_error() {
    assert_invalid(
        r#"
fn main()
    let a = b + 1
"#,
        &["Undefined variable", "b"],
    );
}

// =============================================================================
// These tests verify type-checking of features not yet in codegen
// =============================================================================

#[test]
fn test_hello_world_typecheck() {
    assert_valid(
        r#"
fn main()
    print("Hello, World!")
"#,
    );
}

#[test]
fn test_conditional_typecheck() {
    // If/else with values - type-check only
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
fn test_variable_shadowing_typecheck() {
    assert_valid(
        r#"
fn main()
    let a = 10
    if true
        let a = 20
        print(a)
    print(a)
"#,
    );
}
