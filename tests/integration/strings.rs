// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_with_output};

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
fn test_string_interpolation() {
    assert_runs_with_output(
        r#"
use system.io

let name = "Miri"
print(f"Hello, {name}!")
    "#,
        "Hello, Miri!",
    );
}

#[test]
fn test_string_interpolation_expression() {
    assert_runs_with_output(
        r#"
use system.io

let x = 5
print(f"5 + 3 = {x + 3}")
    "#,
        "5 + 3 = 8",
    );
}
