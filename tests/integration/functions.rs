// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::{assert_invalid, assert_valid};

#[test]
fn test_function_declaration_and_call() {
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
fn test_recursion() {
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
fn test_invalid_argument_count() {
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
fn test_invalid_argument_type() {
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
fn test_return_type_mismatch() {
    assert_invalid(
        r#"
fn get_number() int
    return "string"
"#,
        &["Invalid return type", "expected Int", "got String"],
    );
}
