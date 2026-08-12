// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

#[test]
fn test_regex_compile_valid_pattern() {
    assert_runs_with_output(
        r#"
use system.text

let pattern_result = Regex.compile("[0-9]+")
match pattern_result
    Result.Ok(r): println("compiled")
    Result.Err(e): println("error")
"#,
        "compiled",
    );
}

#[test]
fn test_regex_invalid_handle() {
    assert_runs_with_output(
        r#"
use system.text

let invalid = Regex(handle: 999)
let result = invalid.matches("test")
if result: println("unexpected") else: println("safe")
"#,
        "safe",
    );
}

#[test]
fn test_regex_invalid_handle_find() {
    assert_runs_with_output(
        r#"
use system.text

let invalid = Regex(handle: 999)
let m = invalid.find("test")
match m
    Some(x): println("unexpected")
    None: println("safe")
"#,
        "safe",
    );
}

#[test]
fn test_regex_invalid_handle_replace() {
    assert_runs_with_output(
        r#"
use system.text

let invalid = Regex(handle: 999)
let result = invalid.replace("test", "X")
if result == "test": println("safe") else: println("unexpected")
"#,
        "safe",
    );
}

#[test]
fn test_regex_matches_basic() {
    assert_runs_with_output(
        r#"
use system.text

fn test_matches(pattern Regex)
    if pattern.matches("123"): println("all digits") else: println("not all digits")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_matches(pattern)
        Result.Err(e): println("error")
"#,
        "all digits",
    );
}

#[test]
fn test_regex_matches_failure() {
    assert_runs_with_output(
        r#"
use system.text

fn test_matches(pattern Regex)
    if pattern.matches("abc"): println("matched") else: println("no match")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_matches(pattern)
        Result.Err(e): println("error")
"#,
        "no match",
    );
}

#[test]
fn test_regex_matches_unanchored() {
    assert_runs_with_output(
        r#"
use system.text

fn test_matches(pattern Regex)
    if pattern.matches("abc123"): println("found") else: println("not found")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_matches(pattern)
        Result.Err(e): println("error")
"#,
        "found",
    );
}

#[test]
fn test_regex_matches_anchored_full() {
    assert_runs_with_output(
        r#"
use system.text

fn test_matches(pattern Regex)
    if pattern.matches("123"): println("matched") else: println("no match")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_matches(pattern)
        Result.Err(e): println("error")
"#,
        "matched",
    );
}

#[test]
fn test_regex_matches_anchored_pattern() {
    assert_runs_with_output(
        r#"
use system.text

fn test_matches(pattern Regex)
    if pattern.matches("abc123"): println("matched") else: println("no match")

fn main()
    let pattern_result = Regex.compile("^[0-9]+$")
    match pattern_result
        Result.Ok(pattern): test_matches(pattern)
        Result.Err(e): println("error")
"#,
        "no match",
    );
}

#[test]
fn test_regex_find_basic() {
    assert_runs_with_output(
        r#"
use system.text

fn test_find(pattern Regex)
    let m = pattern.find("hello 123 world")
    match m
        Some(match_obj): println(match_obj.text())
        None: println("none")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find(pattern)
        Result.Err(e): println("error")
"#,
        "123",
    );
}

#[test]
fn test_regex_find_no_match() {
    assert_runs_with_output(
        r#"
use system.text

fn test_find(pattern Regex)
    let m = pattern.find("no numbers here")
    match m
        Some(match_obj): println("unexpected")
        None: println("none")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find(pattern)
        Result.Err(e): println("error")
"#,
        "none",
    );
}

#[test]
fn test_regex_find_offsets() {
    assert_runs_with_output(
        r#"
use system.text

fn print_offsets(m Match)
    let s = m.start()
    let e = m.end()
    println(f"{s},{e}")

fn test_find(pattern Regex)
    let m = pattern.find("hello 123 world")
    match m
        Some(match_obj): print_offsets(match_obj)
        None: println("none")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find(pattern)
        Result.Err(e): println("error")
"#,
        "6,9",
    );
}

#[test]
fn test_regex_find_empty_string() {
    assert_runs_with_output(
        r#"
use system.text

fn test_find(pattern Regex)
    let m = pattern.find("")
    match m
        Some(match_obj): println("unexpected")
        None: println("none")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find(pattern)
        Result.Err(e): println("error")
"#,
        "none",
    );
}

#[test]
fn test_regex_reuse() {
    assert_runs_with_output(
        r#"
use system.text

fn test_reuse(pattern Regex)
    let m1 = pattern.find("abc 123")
    let m2 = pattern.find("def 456")
    match m1
        Some(x): print(x.text())
        None: print("none")
    print(",")
    match m2
        Some(x): println(x.text())
        None: println("none")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_reuse(pattern)
        Result.Err(e): println("error")
"#,
        "123,456",
    );
}
