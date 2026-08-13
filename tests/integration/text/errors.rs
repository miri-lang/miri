// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

#[test]
fn test_regex_valid_compile() {
    assert_runs_with_output(
        r#"
use system.text

let pattern_result = Regex.compile("[a-z]+")
match pattern_result
    Result.Ok(r): println("success")
    Result.Err(e): println("error")
"#,
        "success",
    );
}

#[test]
fn test_regex_syntax_error_unclosed_bracket() {
    assert_runs_with_output(
        r#"
use system.text

let result = Regex.compile("[")
match result
    Result.Ok(r): println("unexpected success")
    Result.Err(e): println("error")
"#,
        "error",
    );
}

#[test]
fn test_regex_syntax_error_unclosed_paren() {
    assert_runs_with_output(
        r#"
use system.text

let result = Regex.compile("(unclosed")
match result
    Result.Ok(r): println("unexpected success")
    Result.Err(e): println("error")
"#,
        "error",
    );
}

#[test]
fn test_regex_syntax_error_invalid_range() {
    assert_runs_with_output(
        r#"
use system.text

let result = Regex.compile("a{2,1}")
match result
    Result.Ok(r): println("unexpected success")
    Result.Err(e): println("error")
"#,
        "error",
    );
}

#[test]
fn test_regex_error_compile_time() {
    assert_runs_with_output(
        r#"
use system.text

fn test_compile(pattern_str String)
    let result = Regex.compile(pattern_str)
    match result
        Result.Ok(r): println("ok")
        Result.Err(e): println("error at compile time")

fn main()
    test_compile("[")
"#,
        "error at compile time",
    );
}
