// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

#[test]
fn test_string_literals() {
    assert_runs(r#""hello""#);
    assert_runs(r#""hello, world!""#);
}

#[test]
fn test_string_with_escapes() {
    assert_runs(r#""line1\nline2""#);
    assert_runs(r#""tab\there""#);
}

#[test]
fn test_string_println() {
    assert_runs_with_output(
        r#"
use system.io

let s = "hello"
println(s)
    "#,
        "hello",
    );
}

#[test]
fn test_string_concatenation() {
    assert_runs_with_output(
        r#"
use system.io

let a = "hello"
let b = " world"
print(a + b)
    "#,
        "hello world",
    );
}

#[test]
fn test_string_interpolation() {
    assert_runs_with_output(
        r#"
use system.io

let name = "Miri"
print(f"Hello, {name}!")
    "#,
        "Hello, Miri!",
    );
}

#[test]
fn test_string_interpolation_expression() {
    assert_runs_with_output(
        r#"
use system.io

let x = 5
print(f"5 + 3 = {x + 3}")
    "#,
        "5 + 3 = 8",
    );
}

#[test]
fn test_string_equality() {
    assert_runs_with_output(
        r#"
use system.io

let a = "hello"
let b = "hello"
let c = "world"
if a == b
    println("equal")
if a != c
    println("not equal")
    "#,
        "equal",
    );
}

#[test]
fn test_string_empty() {
    assert_runs_with_output(
        r#"
use system.io

let s = ""
println(s)
    "#,
        "",
    );
}

#[test]
fn test_string_function_parameter() {
    assert_runs_with_output(
        r#"
use system.io

fn greet(name String)
    println(f"Hello, {name}!")

greet("Miri")
    "#,
        "Hello, Miri!",
    );
}

#[test]
fn test_string_function_return() {
    assert_runs_with_output(
        r#"
use system.io

fn get_greeting() String
    return "Hello from function"

let s = get_greeting()
println(s)
    "#,
        "Hello from function",
    );
}

#[test]
fn test_string_multiple_variables() {
    assert_runs_with_output(
        r#"
use system.io

let first = "foo"
let second = "bar"
let third = "baz"
print(first + second + third)
    "#,
        "foobarbaz",
    );
}

#[test]
fn test_string_in_conditional() {
    assert_runs_with_output(
        r#"
use system.io

let s = "yes"
if s == "yes"
    println("got yes")
else
    println("got no")
    "#,
        "got yes",
    );
}

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

#[test]
fn test_string_invalid_method_error() {
    assert_compiler_error(
        r#"
use system.string

let s = "hello"
let _ = s.nonexistent()
    "#,
        "no field or method",
    );
}
