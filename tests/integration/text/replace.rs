// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

#[test]
fn test_regex_replace_basic() {
    assert_runs_with_output(
        r#"
use system.text

fn test_replace(pattern Regex)
    let result = pattern.replace("a1b2c3", "X")
    println(result)

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_replace(pattern)
        Result.Err(e): println("error")
"#,
        "aXbXcX",
    );
}

#[test]
fn test_regex_replace_no_matches() {
    assert_runs_with_output(
        r#"
use system.text

fn test_replace(pattern Regex)
    let result = pattern.replace("no numbers", "X")
    println(result)

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_replace(pattern)
        Result.Err(e): println("error")
"#,
        "no numbers",
    );
}

#[test]
fn test_regex_replace_empty_replacement() {
    assert_runs_with_output(
        r#"
use system.text

fn test_replace(pattern Regex)
    let result = pattern.replace("a,b,c", "")
    println(result)

fn main()
    let pattern_result = Regex.compile("[,]")
    match pattern_result
        Result.Ok(pattern): test_replace(pattern)
        Result.Err(e): println("error")
"#,
        "abc",
    );
}

#[test]
fn test_regex_replace_multi_char_replacement() {
    assert_runs_with_output(
        r#"
use system.text

fn test_replace(pattern Regex)
    let result = pattern.replace("a1b2c3", "NUM")
    println(result)

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_replace(pattern)
        Result.Err(e): println("error")
"#,
        "aNUMbNUMcNUM",
    );
}

#[test]
fn test_regex_replace_empty_string() {
    assert_runs_with_output(
        r#"
use system.text

fn test_replace(pattern Regex)
    let result = pattern.replace("", "X")
    println(result)

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_replace(pattern)
        Result.Err(e): println("error")
"#,
        "",
    );
}

#[test]
fn test_regex_replace_literal_no_expansion() {
    assert_runs_with_output(
        r#"
use system.text

fn test_replace(pattern Regex)
    let result = pattern.replace("a1b2", "[$0]")
    println(result)

fn main()
    let pattern_result = Regex.compile("[0-9]")
    match pattern_result
        Result.Ok(pattern): test_replace(pattern)
        Result.Err(e): println("error")
"#,
        "a[$0]b[$0]",
    );
}
