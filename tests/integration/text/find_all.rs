// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

#[test]
fn test_regex_find_all_basic() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("a1b2c3")
    for m in matches
        print(m.text())
        print(",")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "1,2,3,",
    );
}

#[test]
fn test_regex_find_all_no_matches() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("no numbers")
    if matches.length() == 0: println("empty") else: println("not empty")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "empty",
    );
}

#[test]
fn test_regex_find_all_empty_string() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("")
    if matches.length() == 0: println("empty") else: println("not empty")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "empty",
    );
}

#[test]
fn test_regex_find_all_zero_width() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("bbb")
    if matches.length() == 4: println("correct") else: println(f"got {matches.length()}")

fn main()
    let pattern_result = Regex.compile("a*")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "correct",
    );
}

#[test]
fn test_regex_find_all_consecutive() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("11 22 33")
    if matches.length() == 3: println("correct") else: println(f"got {matches.length()}")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "correct",
    );
}

#[test]
fn test_regex_find_all_offsets() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn print_match_offsets(m Match)
    let s = m.start()
    let e = m.end()
    println(f"{s},{e}")

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("a1b2c3")
    println(f"{matches.length()}")
    for m in matches
        print_match_offsets(m)

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "3\n1,2\n3,4\n5,6",
    );
}

#[test]
fn test_regex_find_all_zero_width_non_ascii() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("äbc")
    if matches.length() == 4: println("correct") else: println(f"got {matches.length()}")

fn main()
    let pattern_result = Regex.compile("a*")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "correct",
    );
}

#[test]
fn test_regex_find_all_zero_width_non_ascii_offsets() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("äbc")
    println(f"{matches.length()}")
    for m in matches
        println(f"{m.start()},{m.end()}")

fn main()
    let pattern_result = Regex.compile("a*")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "4\n0,0\n2,2\n3,3\n4,4",
    );
}

#[test]
fn test_regex_find_all_non_zero_width_unicode() {
    assert_runs_with_output(
        r#"
use system.text
use system.collections.list

fn test_find_all(pattern Regex)
    let matches = pattern.find_all("日本1語23末")
    if matches.length() == 2: println("correct") else: println(f"got {matches.length()}")

fn main()
    let pattern_result = Regex.compile("[0-9]+")
    match pattern_result
        Result.Ok(pattern): test_find_all(pattern)
        Result.Err(e): println("error")
"#,
        "correct",
    );
}
