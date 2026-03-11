// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_option_assignment_and_none() {
    assert_runs(
        r#"
var x int? = 10
x = None
x = 20
"#,
    );
}

#[test]
fn test_some_constructor() {
    assert_runs_with_output(
        r#"
use system.io

let x = Some(42)
println(f"{x ?? 0}")
"#,
        "42",
    );
}

#[test]
fn test_option_arithmetic_error() {
    assert_compiler_error(
        r#"
let x int? = 5
let y = x + 1
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_none_to_non_optional_error() {
    assert_compiler_error(
        r#"
let x int = None
"#,
        "Type mismatch",
    );
}

#[test]
fn test_if_let_some_immutable_error() {
    assert_compiler_error(
        r#"
fn test(input String?)
    if let Some(s) = input
        s = "changed"
"#,
        "Cannot assign to immutable variable 's'",
    );
}

#[test]
fn test_option_type_mismatch_inner() {
    assert_compiler_error(
        r#"
fn main()
    let a int? = Some("string")
"#,
        "Type mismatch",
    );
}
