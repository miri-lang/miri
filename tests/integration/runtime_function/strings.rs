// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_runtime_string_len() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "Hello, Miri!"
    println(f"{s.size()}")
"#,
        "12",
    );
}

#[test]
fn test_runtime_string_empty() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s1 = ""
    let s2 = "not empty"
    let a = if s1.is_empty()
        1
    else
        0
    let b = if s2.is_empty()
        0
    else
        1
    println(f"{a * b}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_concat() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let a = "Hello, "
    let b = "World!"
    let combined = a.concat(b)
    let expected = "Hello, World!"

    let result = if combined.equals(expected)
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_case_conversion() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "Mixed CASE"
    let lower = s.to_lower()
    let upper = s.to_upper()

    let result = if lower.equals("mixed case") and upper.equals("MIXED CASE")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_trim() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "  val  "
    let trimmed = s.trim()

    let result = if trimmed.equals("val")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_contains_starts_ends() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "foobarbaz"

    let result = if s.contains("bar") and s.starts_with("foo") and s.ends_with("baz")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_string_substring() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "hello world"
    let sub = s.substring(0, 5)

    let result = if sub.equals("hello")
        1
    else
        0
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn probe_string_len() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    let s = "hello"
    println(f"{s.size()}")
"#,
        "5",
    );
}
