// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs;

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
    assert_runs(
        r#"
let a = "hello"
let b = " world"
a + b
    "#,
    );
}

#[test]
fn test_string_interpolation() {
    assert_runs(
        r#"
let name = "Miri"
f"Hello, {name}!"
    "#,
    );
}

#[test]
fn test_string_interpolation_expression() {
    assert_runs(
        r#"
let x = 5
f"5 + 3 = {x + 3}"
    "#,
    );
}
