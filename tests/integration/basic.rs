// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{
    assert_returns, assert_returns_many, assert_runs, assert_runs_many,
};

#[test]
fn empty_program() {
    assert_runs("");
}

#[test]
fn program_with_integer_literal() {
    assert_returns("42", 42);
}

#[test]
fn program_with_multiple_literals() {
    assert_returns(
        r#"
123.456
"Hello, World!"
1000
"#,
        1000,
    );
}

#[test]
fn comments_only() {
    assert_runs("// just a comment");
    assert_runs(
        r#"
// comment 1
// comment 2
"#,
    );
}

#[test]
fn whitespace_only() {
    assert_runs_many(&["   ", "\n\n", "  \n  \t  "]);
}

#[test]
fn basic_literals() {
    assert_runs_many(&["true", "false", "1.23", r#""string""#]);
}

#[test]
fn integer_edge_cases() {
    assert_returns_many(&[
        ("0", 0),
        ("-1", -1),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
    ]);
}

#[test]
fn mixed_basic_program() {
    assert_runs(
        r#"
    // Start
    123
    "text"
    // End
"#,
    );
}
