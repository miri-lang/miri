// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_to_upper() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello"
print(s.to_upper())
"#,
        "HELLO",
    );
}

#[test]
fn test_string_to_lower() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "HELLO"
print(s.to_lower())
"#,
        "hello",
    );
}

#[test]
fn test_string_trim() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "  hello  "
print(s.trim())
"#,
        "hello",
    );
}

#[test]
fn test_string_trim_start() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "  hello"
print(s.trim_start())
"#,
        "hello",
    );
}

#[test]
fn test_string_trim_end() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello  "
print(s.trim_end())
"#,
        "hello",
    );
}

#[test]
fn test_string_contains() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello world"
if s.contains("world")
    println("found")
else
    println("not found")
"#,
        "found",
    );
}

#[test]
fn test_string_starts_with() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello world"
if s.starts_with("hello")
    println("yes")
else
    println("no")
"#,
        "yes",
    );
}

#[test]
fn test_string_ends_with() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello world"
if s.ends_with("world")
    println("yes")
else
    println("no")
"#,
        "yes",
    );
}

#[test]
fn test_string_replace() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello world"
print(s.replace("world", "miri"))
"#,
        "hello miri",
    );
}

#[test]
fn test_string_substring() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello"
print(s.substring(1, 4))
"#,
        "ell",
    );
}

#[test]
fn test_string_is_empty() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = ""
if s.is_empty()
    println("empty")
else
    println("not empty")
"#,
        "empty",
    );
}

#[test]
fn test_string_length_method() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello"
println(f"{s.length()}")
"#,
        "5",
    );
}

#[test]
fn test_string_chained_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "  HELLO  "
print(s.trim().to_lower())
"#,
        "hello",
    );
}
