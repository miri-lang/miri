// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// --- Acceptance criteria tests ---

#[test]
fn test_identity_int() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    println(f"{identity(42)}")
    "#,
        "42",
    );
}

#[test]
fn test_identity_string() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    println(identity("hello"))
    "#,
        "hello",
    );
}

#[test]
fn test_identity_two_instantiations() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    println(f"{identity(42)}")
    println(identity("hello"))
    "#,
        "42",
    );
}

#[test]
fn test_identity_float() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    let v = identity(3.14)
    println(f"{v > 3.0}")
    "#,
        "true",
    );
}

#[test]
fn test_identity_bool() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    println(f"{identity(true)}")
    "#,
        "true",
    );
}

// --- Additional functionality tests ---

#[test]
fn test_generic_wrap_int() {
    assert_runs_with_output(
        r#"
use system.io

fn wrap<T>(val T) T
    val

fn main()
    let result = wrap(100)
    println(f"{result}")
    "#,
        "100",
    );
}

#[test]
fn test_generic_result_used_in_expression() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    let a = identity(10)
    let b = identity(20)
    println(f"{a + b}")
    "#,
        "30",
    );
}

#[test]
fn test_generic_called_multiple_times_same_type() {
    assert_runs_with_output(
        r#"
use system.io

fn identity<T>(x T) T
    x

fn main()
    println(f"{identity(1)}")
    println(f"{identity(2)}")
    println(f"{identity(3)}")
    "#,
        "1",
    );
}
