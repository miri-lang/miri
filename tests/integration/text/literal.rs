// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

/// Criterion 1: Basic regex literal with matches() method
#[test]
fn test_regex_literal_basic_match() {
    assert_runs_with_output(
        r#"
use system.text

let r = re"^\d+$"
if r.matches("12345")
    println("true")
else
    println("false")
"#,
        "true",
    );
}

#[test]
fn test_regex_literal_basic_no_match() {
    assert_runs_with_output(
        r#"
use system.text

let r = re"^\d+$"
if r.matches("abc")
    println("true")
else
    println("false")
"#,
        "false",
    );
}

/// Criterion 2: Case-insensitive flag
#[test]
fn test_regex_literal_ignore_case() {
    assert_runs_with_output(
        r#"
use system.text

let r_case = re"hello"i
let r_no_case = re"hello"
if r_case.matches("HELLO")
    println("case_match")
else
    println("case_no_match")
if r_no_case.matches("HELLO")
    println("no_case_match")
else
    println("no_case_no_match")
"#,
        "case_match\nno_case_no_match",
    );
}

/// Criterion 3: Dotall and multiline flags
#[test]
fn test_regex_literal_dotall_flag() {
    assert_runs_with_output(
        r#"
use system.text

let r_dotall = re"a.c"s
if r_dotall.matches("a
c")
    println("dotall_match")
else
    println("dotall_no_match")
"#,
        "dotall_match",
    );
}

#[test]
fn test_regex_literal_multiline_flag() {
    assert_runs_with_output(
        r#"
use system.text

let r_multiline = re"^b"m
if r_multiline.matches("a
b")
    println("multiline_match")
else
    println("multiline_no_match")
"#,
        "multiline_match",
    );
}

/// Criterion 4: Regex literal methods
#[test]
fn test_regex_literal_find_all() {
    assert_runs_with_output(
        r#"
use system.text

let r = re"\d+"
let matches = r.find_all("a1b22c333")
println(f"{matches.length()}")
if matches.length() == 3
    var i = 0
    while i < matches.length()
        let m = matches.element_at(i)
        println(m.text())
        i = i + 1
"#,
        "3\n1\n22\n333",
    );
}

/// Criterion 5: Malformed pattern compile error
#[test]
fn test_regex_literal_invalid_pattern() {
    assert_compiler_error(
        r#"
use system.text

let r = re"[invalid"
"#,
        "unclosed character class",
    );
}

/// Criterion 6: Global flag rejection
#[test]
fn test_regex_literal_global_flag_rejected() {
    assert_compiler_error(
        r#"
use system.text

let r = re"hi"g
"#,
        "does not support the 'g' flag",
    );
}

/// Criterion 7: Regex literal in gpu fn
#[test]
fn test_regex_literal_gpu_fn_rejected() {
    assert_compiler_error(
        r#"
use system.text

public gpu fn test_kernel()
    let r = re"pattern"
"#,
        "cannot be used inside a GPU function",
    );
}

/// Criterion 8: Caching in loop
#[test]
fn test_regex_literal_caching() {
    assert_runs_with_output(
        r#"
use system.text

var count = 0
var i = 0
while i < 10000
    let r = re"\d+"
    if r.matches("123")
        count = count + 1
    i = i + 1
if count == 10000
    println("all_matched")
else
    println("mismatch")
"#,
        "all_matched",
    );
}

/// Criterion 9: Backward compatibility with Regex.compile()
#[test]
fn test_regex_compile_still_returns_result() {
    assert_runs_with_output(
        r#"
use system.text

let result = Regex.compile("[0-9]+")
match result
    Result.Ok(r): println("ok")
    Result.Err(e): println("err")
"#,
        "ok",
    );
}
