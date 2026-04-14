// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

/// Plan 1.3 — Test 1: aliasing then reassigning the original still lets the alias read correctly.
/// `var x = "hello"; var y = x; x = "world"; println(y)` → prints `hello`
#[test]
fn test_string_alias_then_reassign_original() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    var x = "hello"
    var y = x
    x = "world"
    println(y)
"#,
        "hello",
    );
}

/// Plan 1.3 — Test 2: simple reassignment drops the old value, prints new one.
/// `var s = "hello"; s = "world"; println(s)` → prints `world`
#[test]
fn test_string_simple_reassignment() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn main()
    var s = "hello"
    s = "world"
    println(s)
"#,
        "world",
    );
}

/// Plan 1.3 — Test 3: string passed to a function that prints it; no leak, no double-free.
/// `fn consume(s String): println(s)` + `let x = "hello"; consume(x)` → prints `hello`
#[test]
fn test_string_consume_in_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn consume(s String)
    println(s)

fn main()
    let x = "hello"
    consume(x)
"#,
        "hello",
    );
}

/// Plan 1.3 — Test 4: aliasing in function parameters — caller's copy survives after callee returns.
#[test]
fn test_string_alias_survives_callee() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn take(s String)
    let x = 1

fn main()
    let s1 = "hello world"
    let s2 = s1
    take(s1)
    println(s2)
"#,
        "hello world",
    );
}

/// Plan 1.3 — Test 4b: string aliasing through function return value.
#[test]
fn test_string_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn make() String
    let s = "returned"
    return s

fn main()
    let r = make()
    println(r)
"#,
        "returned",
    );
}

/// Pre-existing aliasing + consume test (kept for regression coverage).
#[test]
fn test_string_rc_aliasing() {
    assert_runs(
        r#"
use system.io
use system.string

fn consume(s String)
    // s goes out of scope here, should not drop underlying buffer if RC > 1
    let x = 1

fn main()
    let s1 = "hello world"
    let s2 = s1 // IncRef

    consume(s1)

    // s2 should still be valid here, no double free or use-after-free
    let len = s2.length()

    var s3 = "temporary string"
    s3 = s2 // Reassignment drops "temporary string"
"#,
    );
}
