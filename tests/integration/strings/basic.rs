// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_literals() {
    assert_runs(r#""hello""#);
    assert_runs(r#""hello, world!""#);
}

#[test]
fn test_string_with_escapes() {
    assert_runs(r#""line1\nline2""#);
    assert_runs(r#""tab\there""#);
}

#[test]
fn test_string_println() {
    assert_runs_with_output(
        r#"
use system.io

let s = "hello"
println(s)
"#,
        "hello",
    );
}

#[test]
fn test_string_concatenation() {
    assert_runs_with_output(
        r#"
use system.io

let a = "hello"
let b = " world"
print(a + b)
"#,
        "hello world",
    );
}

#[test]
fn test_string_empty() {
    assert_runs_with_output(
        r#"
use system.io

let s = ""
println(s)
"#,
        "",
    );
}

#[test]
fn test_string_multiple_variables() {
    assert_runs_with_output(
        r#"
use system.io

let first = "foo"
let second = "bar"
let third = "baz"
print(first + second + third)
"#,
        "foobarbaz",
    );
}

#[test]
fn test_string_escape_newline() {
    assert_runs_with_output(
        r#"
use system.io

print("line1\nline2")
"#,
        "line1\nline2",
    );
}

#[test]
fn test_string_escape_tab() {
    assert_runs_with_output(
        r#"
use system.io

print("col1\tcol2")
"#,
        "col1\tcol2",
    );
}
