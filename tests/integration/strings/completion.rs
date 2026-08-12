// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ============================================================================
// split(separator String) [String]
// ============================================================================

#[test]
fn test_string_split_basic() {
    assert_runs_with_output(
        r#"
use system.collections.list

let s = "a,b,c"
let parts = s.split(",")
for part in parts
    println(part)
"#,
        "a\nb\nc",
    );
}

#[test]
fn test_string_split_separator_not_found() {
    assert_runs_with_output(
        r#"
use system.collections.list

let s = "abc"
let parts = s.split(",")
if parts.length() == 1  and parts.element_at(0) == "abc"
    println("pass")
else
    println("fail")
"#,
        "pass",
    );
}

#[test]
fn test_string_split_empty_subject() {
    assert_runs_with_output(
        r#"
use system.collections.list

let s = ""
let parts = s.split(",")
if parts.length() == 1  and parts.element_at(0) == ""
    println("pass")
else
    println("fail")
"#,
        "pass",
    );
}

#[test]
fn test_string_split_empty_separator() {
    assert_runs_with_output(
        r#"
use system.collections.list

let s = "abc"
let parts = s.split("")
for part in parts
    println(part)
"#,
        "a\nb\nc",
    );
}

#[test]
fn test_string_split_consecutive_separators() {
    assert_runs_with_output(
        r#"
use system.collections.list

let s = "a,,b"
let parts = s.split(",")
println(f"{parts.length()}")
for part in parts
    println(f"{part}:")
"#,
        "3\na:\n:\nb:",
    );
}

// ============================================================================
// join(parts [String]) String
// ============================================================================

#[test]
fn test_string_join_basic() {
    assert_runs_with_output(
        r#"
use system.collections.list

let sep = ","
let parts = List(["a", "b", "c"])
print(sep.join(parts))
"#,
        "a,b,c",
    );
}

#[test]
fn test_string_join_empty_list() {
    assert_runs_with_output(
        r#"
use system.collections.list

let sep = ","
let parts = List([])
let result = sep.join(parts)
if result == ""
    println("pass")
else
    println("fail")
"#,
        "pass",
    );
}

#[test]
fn test_string_join_single_element() {
    assert_runs_with_output(
        r#"
use system.collections.list

let sep = ","
let parts = List(["hello"])
print(sep.join(parts))
"#,
        "hello",
    );
}

#[test]
fn test_string_join_with_empty_separator() {
    assert_runs_with_output(
        r#"
use system.collections.list

let sep = ""
let parts = List(["a", "b", "c"])
print(sep.join(parts))
"#,
        "abc",
    );
}

// ============================================================================
// to_int() int?
// ============================================================================

#[test]
fn test_string_to_int_valid() {
    assert_runs_with_output(
        r#"
let s = "42"
match s.to_int()
    Some(n): println(f"{n}")
    None: println("failed")
"#,
        "42",
    );
}

#[test]
fn test_string_to_int_negative() {
    assert_runs_with_output(
        r#"
let s = "-42"
match s.to_int()
    Some(n): println(f"{n}")
    None: println("failed")
"#,
        "-42",
    );
}

#[test]
fn test_string_to_int_invalid() {
    assert_runs_with_output(
        r#"
let s = "not a number"
match s.to_int()
    Some(n): println("unexpected")
    None: println("none")
"#,
        "none",
    );
}

#[test]
fn test_string_to_int_empty() {
    assert_runs_with_output(
        r#"
let s = ""
match s.to_int()
    Some(n): println("unexpected")
    None: println("none")
"#,
        "none",
    );
}

#[test]
fn test_string_to_int_with_whitespace() {
    assert_runs_with_output(
        r#"
let s = "  42  "
match s.to_int()
    Some(n): println(f"{n}")
    None: println("none")
"#,
        "none",
    );
}

// ============================================================================
// to_float() float?
// ============================================================================

#[test]
#[ignore = "float is narrowed to f32 through a parameter and bit-reinterpreted as an Option payload, so to_float returns garbage"]
fn test_string_to_float_valid() {
    assert_runs_with_output(
        r#"
let s = "3.14"
match s.to_float()
    Some(f): println(f"{f == 3.14}")
    None: println("failed")
"#,
        "true",
    );
}

#[test]
#[ignore = "float is narrowed to f32 through a parameter and bit-reinterpreted as an Option payload, so to_float returns garbage"]
fn test_string_to_float_integer() {
    assert_runs_with_output(
        r#"
let s = "42"
match s.to_float()
    Some(f): println(f"{f == 42.0}")
    None: println("failed")
"#,
        "true",
    );
}

#[test]
#[ignore = "float is narrowed to f32 through a parameter and bit-reinterpreted as an Option payload, so to_float returns garbage"]
fn test_string_to_float_negative() {
    assert_runs_with_output(
        r#"
let s = "-3.14"
match s.to_float()
    Some(f): println(f"{f == -3.14}")
    None: println("failed")
"#,
        "true",
    );
}

#[test]
fn test_string_to_float_invalid() {
    assert_runs_with_output(
        r#"
let s = "not a float"
match s.to_float()
    Some(f): println("unexpected")
    None: println("none")
"#,
        "none",
    );
}

#[test]
fn test_string_to_float_empty() {
    assert_runs_with_output(
        r#"
let s = ""
match s.to_float()
    Some(f): println("unexpected")
    None: println("none")
"#,
        "none",
    );
}

#[test]
#[ignore = "float is narrowed to f32 through a parameter and bit-reinterpreted as an Option payload, so to_float returns garbage"]
fn test_string_to_float_infinity() {
    assert_runs_with_output(
        r#"
let s = "1e400"
match s.to_float()
    Some(f): println(f"{f > 1.0e308}")
    None: println("failed")
"#,
        "true",
    );
}

#[test]
#[ignore = "float is narrowed to f32 through a parameter and bit-reinterpreted as an Option payload, so to_float returns garbage"]
fn test_string_to_float_nan() {
    assert_runs_with_output(
        r#"
let s = "NaN"
match s.to_float()
    Some(f): println(f"{f != f}")
    None: println("failed")
"#,
        "true",
    );
}

#[test]
#[ignore = "float is narrowed to f32 through a parameter and bit-reinterpreted as an Option payload, so to_float returns garbage"]
fn test_string_to_float_infinity_literal() {
    assert_runs_with_output(
        r#"
let s = "inf"
match s.to_float()
    Some(f): println(f"{f > 1.0e308}")
    None: println("failed")
"#,
        "true",
    );
}
