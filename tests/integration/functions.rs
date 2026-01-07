// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Function-related integration tests.
//!
//! Note: Function calls are not yet implemented in the Cranelift backend.
//! These tests use assert_valid for type-checking until call support is added.

use crate::test_utils::{assert_compiles, assert_invalid, assert_valid};

// =============================================================================
// Simple function definitions (compile without calls)
// =============================================================================

#[test]
fn test_single_function_returns_constant() {
    assert_compiles(
        r#"
fn main() int
    42
"#,
    );
}

#[test]
fn test_function_with_explicit_return() {
    assert_compiles(
        r#"
fn main() int
    return 100
"#,
    );
}

#[test]
fn test_function_with_computation() {
    assert_compiles(
        r#"
fn main() int
    let x = 10
    let y = 20
    return x * y + 5
"#,
    );
}

// =============================================================================
// These tests verify type-checking of function calls (not yet in codegen)
// =============================================================================

#[test]
fn test_function_call_typecheck() {
    assert_valid(
        r#"
fn add(a int, b int) int
    return a + b

fn main()
    let result = add(1, 2)
    print(result)
"#,
    );
}

#[test]
fn test_recursion_typecheck() {
    assert_valid(
        r#"
fn factorial(n int) int
    if n <= 1
        return 1
    return n * factorial(n - 1)

fn main()
    print(factorial(5))
"#,
    );
}

#[test]
fn test_fibonacci_typecheck() {
    assert_valid(
        r#"
fn fib(n int) int
    if n <= 1
        return n
    return fib(n - 1) + fib(n - 2)

fn main()
    print(fib(10))
"#,
    );
}

// Note: Mutual recursion (forward declarations) is not yet supported

// =============================================================================
// Error cases
// =============================================================================

#[test]
fn test_missing_argument_error() {
    assert_invalid(
        r#"
fn add(a int, b int) int
    return a + b

fn main()
    add(1)
"#,
        &["Missing argument for parameter 'b'"],
    );
}

#[test]
fn test_type_mismatch_argument_error() {
    assert_invalid(
        r#"
fn add(a int, b int) int
    return a + b

fn main()
    add(1, "2")
"#,
        &["Type mismatch"],
    );
}

#[test]
fn test_return_type_mismatch_error() {
    assert_invalid(
        r#"
fn get_number() int
    return "string"
"#,
        &["Invalid return type", "expected Int", "got String"],
    );
}

#[test]
fn test_undefined_function_error() {
    assert_invalid(
        r#"
fn main()
    let x = undefined_function()
"#,
        &["Undefined variable", "undefined_function"],
    );
}
