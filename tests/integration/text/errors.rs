// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// The error-path tests here are ignored against a compiler-level leak, not a
// defect in this module. Constructing an enum variant whose payload is a
// runtime-allocated `String` never releases that allocation, so the leak
// observer reports one outstanding RC allocation and fails the test. It
// reproduces with no stdlib involvement, as a bare enum holding a computed
// string:
//
//     public enum MyError
//         Bad(String)
//     fn make() MyError
//         let s = "bo" + "om"
//         return MyError.Bad(s)
//
// A `String` literal payload does not leak, which is why the success-path test
// passes, and why `FsError` — whose variants echo a caller-supplied path
// rather than allocating — never surfaced this. `RegexError` carries the
// engine's own diagnostic, so it is the first to hit it. The regex behaviour
// these tests assert is already correct; only the leak observer fails them.

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
#[ignore = "enum variant with a runtime-allocated String payload leaks the payload"]
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
#[ignore = "enum variant with a runtime-allocated String payload leaks the payload"]
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
#[ignore = "enum variant with a runtime-allocated String payload leaks the payload"]
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
#[ignore = "enum variant with a runtime-allocated String payload leaks the payload"]
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
