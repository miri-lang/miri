// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

// =============================================================================
// Match on Option
// =============================================================================

#[test]
fn test_match_option_some() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    let result = match input
                    Some(s): f"Some: {s}"
                    None: "None"
    println(result)

test("Hello")
        "#,
        "Some: Hello",
    );
}

#[test]
fn test_match_option_none() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    let result = match input
                    Some(s): f"Some: {s}"
                    None: "None"
    println(result)

test(None)
        "#,
        "None",
    );
}

// =============================================================================
// If let Some pattern matching
// =============================================================================

#[test]
fn test_if_let_some() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if let Some(s) = input
        println(f"unwrapped: {s}")

test("Hello")
        "#,
        "unwrapped: Hello",
    );
}

#[test]
fn test_if_let_some_none_skips() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if let Some(s) = input
        println("should not print")
    println("done")

test(None)
        "#,
        "done",
    );
}

#[test]
fn test_if_let_some_immutable() {
    assert_compiler_error(
        r#"
fn test(input String?)
    if let Some(s) = input
        s = "changed"
        "#,
        "Cannot assign to immutable variable 's'",
    );
}

// =============================================================================
// If var Some pattern matching (mutable binding)
// =============================================================================

#[test]
fn test_if_var_some_mutable() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if var Some(s) = input
        s = f"{s} changed"
        println(s)

test("Hello")
        "#,
        "Hello changed",
    );
}

#[test]
fn test_if_var_some_none_skips() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if var Some(s) = input
        println("should not print")
    println("done")

test(None)
        "#,
        "done",
    );
}

// =============================================================================
// Null coalescing operator (??)
// =============================================================================

#[test]
fn test_null_coalesce_some() {
    assert_runs_with_output(
        r#"
use system.io

let x int? = 42
println(f"{x ?? 0}")
        "#,
        "42",
    );
}

#[test]
fn test_null_coalesce_none() {
    assert_runs_with_output(
        r#"
use system.io

let x int? = None
println(f"{x ?? 99}")
        "#,
        "99",
    );
}

#[test]
fn test_null_coalesce_string_some() {
    assert_runs_with_output(
        r#"
use system.io

let s String? = "hello"
println(s ?? "default")
        "#,
        "hello",
    );
}

#[test]
fn test_null_coalesce_string_none() {
    assert_runs_with_output(
        r#"
use system.io

let s String? = None
println(s ?? "default")
        "#,
        "default",
    );
}

#[test]
fn test_null_coalesce_with_function_call() {
    assert_runs_with_output(
        r#"
use system.io

fn f() int?
    return None

println(f"{f() ?? 0}")
        "#,
        "0",
    );
}

#[test]
fn test_null_coalesce_with_some_constructor() {
    assert_runs_with_output(
        r#"
use system.io

let x = Some(42)
println(f"{x ?? 0}")
        "#,
        "42",
    );
}

// =============================================================================
// While let Some / while var Some
// =============================================================================

#[test]
fn test_while_let_some() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    while let Some(s) = input
        println(f"value: {s}")
        break

test("Hello")
        "#,
        "value: Hello",
    );
}

#[test]
fn test_while_let_some_none_skips() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    while let Some(s) = input
        println("should not print")
        break
    println("done")

test(None)
        "#,
        "done",
    );
}

#[test]
fn test_while_var_some_mutable() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    while var Some(s) = input
        s = f"{s} changed"
        println(s)
        break

test("Hello")
        "#,
        "Hello changed",
    );
}

// =============================================================================
// Option with basic operations
// =============================================================================

#[test]
fn test_option_assignment_and_none() {
    assert_runs(
        r#"
var x int? = 10
x = None
x = 20
        "#,
    );
}

#[test]
fn test_some_constructor() {
    assert_runs_with_output(
        r#"
use system.io

let x = Some(42)
println(f"{x ?? 0}")
        "#,
        "42",
    );
}

#[test]
fn test_option_arithmetic_error() {
    assert_compiler_error(
        r#"
let x int? = 5
let y = x + 1
        "#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_none_to_non_optional_error() {
    assert_compiler_error(
        r#"
let x int = None
        "#,
        "Type mismatch",
    );
}

// =============================================================================
// Option wrapping managed collections (RC / drop test)
// =============================================================================

#[test]
fn test_option_wrapping_collection() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let opt List<int>? = Some(List([1, 2, 3]))

    match opt
        Some(l): println(f"{l.length()}")
        None: println("error")

    // Reassigning to None should drop the collection
    var opt2 List<int>? = Some(List([4, 5]))
    opt2 = None
    println("done")
        "#,
        "3\ndone",
    );
}
