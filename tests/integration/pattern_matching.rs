// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs_with_output;

#[test]
fn test_match_integer_literals() {
    assert_runs_with_output(
        r#"
use system.io

let val = 1
let result = match val
    1: 100
    2: 200
    _: 0
print(f"{result}")
        "#,
        "100",
    );
}

#[test]
fn test_match_variable() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
let result = match x
    0: 0
    42: 1
    _: 2
print(f"{result}")
    "#,
        "1",
    );
}

#[test]
fn test_match_with_guards() {
    assert_runs_with_output(
        r#"
use system.io

let x = 15
let result = match x
    n if n > 10: 1
    n if n > 5: 2
    _: 3
print(f"{result}")
    "#,
        "1",
    );
}

#[test]
fn test_match_identifier_binding() {
    assert_runs_with_output(
        r#"
use system.io

let x = 42
let result = match x
    value: value * 2
print(f"{result}")
    "#,
        "84",
    );
}

#[test]
fn test_match_default() {
    assert_runs_with_output(
        r#"
use system.io

let val = 99
let result = match val
    1: 10
    2: 20
    _: 999
print(f"{result}")
    "#,
        "999",
    );
}

#[test]
fn test_match_nested() {
    assert_runs_with_output(
        r#"
use system.io

let outer = 1
let inner = 2
let result = match outer
    1: match inner
        1: 10
        2: 20
        _: 30
    _: 0
print(f"{result}")
    "#,
        "20",
    );
}

// =============================================================================
// Multiple patterns per arm (OR patterns)
// =============================================================================

#[test]
fn test_match_or_patterns_first() {
    assert_runs_with_output(
        r#"
use system.io

let x = 1
let result = match x
    1 | 2 | 3: 10
    4 | 5: 20
    _: 99
print(f"{result}")
        "#,
        "10",
    );
}

#[test]
fn test_match_or_patterns_second_arm() {
    assert_runs_with_output(
        r#"
use system.io

let x = 5
let result = match x
    1 | 2 | 3: 10
    4 | 5: 20
    _: 99
print(f"{result}")
        "#,
        "20",
    );
}

#[test]
fn test_match_or_patterns_default() {
    assert_runs_with_output(
        r#"
use system.io

let x = 7
let result = match x
    1 | 2 | 3: 10
    4 | 5: 20
    _: 99
print(f"{result}")
        "#,
        "99",
    );
}

// =============================================================================
// Boolean pattern matching
// =============================================================================

#[test]
fn test_match_bool_true() {
    assert_runs_with_output(
        r#"
use system.io

let b = true
let result = match b
    true: 1
    false: 0
print(f"{result}")
        "#,
        "1",
    );
}

#[test]
fn test_match_bool_false() {
    assert_runs_with_output(
        r#"
use system.io

let b = false
let result = match b
    true: 1
    false: 0
print(f"{result}")
        "#,
        "0",
    );
}

// =============================================================================
// Negative integer patterns
// =============================================================================

#[test]
fn test_match_negative_int() {
    assert_runs_with_output(
        r#"
use system.io

fn sign(n int) int
    match n
        0: 0
        _: if n > 0: 1 else: -1

fn main()
    println(f"{sign(-5)}")
    println(f"{sign(0)}")
    println(f"{sign(3)}")
        "#,
        "1",
    );
}

// =============================================================================
// Match as a statement (side effects, no return value used)
// =============================================================================

#[test]
fn test_match_used_as_statement() {
    // Match expression used as a statement (result discarded).
    // The selected arm's expression value is computed but not assigned.
    // Verify that execution flows to the correct arm and side-effects after match are correct.
    assert_runs_with_output(
        r#"
use system.io

var count = 0
let x = 2
let delta = match x
    1: 10
    2: 20
    _: 99
count = count + delta
print(f"{count}")
        "#,
        "20",
    );
}

// =============================================================================
// Match in a function context
// =============================================================================

#[test]
fn test_match_in_function() {
    assert_runs_with_output(
        r#"
use system.io

fn describe(n int) int
    match n
        1: 100
        2: 200
        3: 300
        _: 0

fn main()
    println(f"{describe(1)}")
    println(f"{describe(2)}")
    println(f"{describe(3)}")
    println(f"{describe(99)}")
        "#,
        "100",
    );
}

#[test]
fn test_match_function_fibonacci() {
    assert_runs_with_output(
        r#"
use system.io

fn fib(n int) int
    match n
        0: 0
        1: 1
        _: fib(n - 1) + fib(n - 2)

fn main()
    println(f"{fib(0)}")
    println(f"{fib(1)}")
    println(f"{fib(5)}")
    println(f"{fib(7)}")
        "#,
        "0",
    );
}

// =============================================================================
// Guard with identifier binding used in body
// =============================================================================

#[test]
fn test_match_guard_binding_used() {
    assert_runs_with_output(
        r#"
use system.io

let x = 15
let result = match x
    n if n >= 10: n * 2
    n if n >= 5: n + 10
    n: n
print(f"{result}")
        "#,
        "30",
    );
}

#[test]
fn test_match_guard_second_arm() {
    assert_runs_with_output(
        r#"
use system.io

let x = 7
let result = match x
    n if n >= 10: n * 2
    n if n >= 5: n + 10
    n: n
print(f"{result}")
        "#,
        "17",
    );
}

#[test]
fn test_match_guard_catch_all() {
    assert_runs_with_output(
        r#"
use system.io

let x = 3
let result = match x
    n if n >= 10: n * 2
    n if n >= 5: n + 10
    n: n
print(f"{result}")
        "#,
        "3",
    );
}

// =============================================================================
// Match with zero as a discriminant
// =============================================================================

#[test]
fn test_match_zero() {
    assert_runs_with_output(
        r#"
use system.io

let x = 0
let result = match x
    0: 42
    _: 99
print(f"{result}")
        "#,
        "42",
    );
}

// =============================================================================
// Match without default (compiler should handle gracefully when arm is hit)
// =============================================================================

#[test]
fn test_match_without_default_hits_arm() {
    assert_runs_with_output(
        r#"
use system.io

let x = 1
let result = match x
    1: 10
    2: 20
print(f"{result}")
        "#,
        "10",
    );
}
