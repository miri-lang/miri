// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_add_operator() {
    assert_runs_with_output(
        r#"
use system.io

let a = "foo"
let b = "bar"
print(a + b)
"#,
        "foobar",
    );
}

#[test]
fn test_string_add_chained() {
    assert_runs_with_output(
        r#"
use system.io

print("a" + "b" + "c" + "d")
"#,
        "abcd",
    );
}

#[test]
fn test_string_equal_operator() {
    assert_runs_with_output(
        r#"
use system.io

if "hello" == "hello"
    print("yes")
else
    print("no")
"#,
        "yes",
    );
}

#[test]
fn test_string_not_equal_operator() {
    assert_runs_with_output(
        r#"
use system.io

if "hello" != "world"
    print("different")
else
    print("same")
"#,
        "different",
    );
}

#[test]
fn test_string_equal_false() {
    assert_runs_with_output(
        r#"
use system.io

if "hello" == "world"
    print("same")
else
    print("different")
"#,
        "different",
    );
}

#[test]
fn test_string_multiply_operator() {
    assert_runs_with_output(
        r#"
use system.io

let s = "ha" * 3
print(s)
"#,
        "hahaha",
    );
}

#[test]
fn test_string_multiply_single() {
    assert_runs_with_output(
        r#"
use system.io

print("x" * 1)
"#,
        "x",
    );
}

#[test]
fn test_string_multiply_zero() {
    assert_runs_with_output(
        r#"
use system.io

print("x" * 0)
"#,
        "",
    );
}

#[test]
fn test_string_multiply_expression() {
    assert_runs_with_output(
        r#"
use system.io

let count = 2 + 3
print("ab" * count)
"#,
        "ababababab",
    );
}

#[test]
fn test_string_multiply_invalid_rhs() {
    // String * String is invalid — the right-hand side must be an integer.
    assert_compiler_error(
        r#"
let s = "ha" * "3"
"#,
        "Type mismatch: cannot multiply String by String",
    );
}
