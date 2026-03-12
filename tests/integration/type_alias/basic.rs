// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn type_alias_simple() {
    assert_runs("type MyInt is int\nlet x MyInt = 5");
}

#[test]
fn type_alias_string() {
    assert_runs_with_output(
        r#"
use system.io
type ID is String
let id ID = "abc-123"
println(id)
"#,
        "abc-123",
    );
}

#[test]
fn type_alias_in_variable() {
    assert_runs_with_output(
        r#"
use system.io
type MyInt is int
let x MyInt = 42
println(f"{x}")
"#,
        "42",
    );
}

#[test]
fn type_alias_chain() {
    assert_runs_with_output(
        r#"
use system.io
type A is int
type B is A
let x B = 99
println(f"{x}")
"#,
        "99",
    );
}

#[test]
fn type_alias_multiple_uses() {
    assert_runs_many(&[
        "type MyInt is int\nlet a MyInt = 1\nlet b MyInt = 2",
        "type MyFloat is float\nlet x MyFloat = 1.5\nlet y MyFloat = 2.5",
        "type MyBool is bool\nlet t MyBool = true\nlet f MyBool = false",
    ]);
}
