// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{
    assert_compiler_error, assert_runs, assert_runs_many, assert_runs_with_output,
};

#[test]
fn variable_declaration() {
    assert_runs_many(&[
        "let x = 10",
        "var y = 20",
        "let z int = 30",
        "var w float = 40.0",
        "let s String = \"hello\"",
        "var b bool = true",
    ]);
}

#[test]
fn implicit_typing() {
    assert_runs_with_output(
        r#"
use system.io
let x = 10
println(f"{x}")
        "#,
        "10",
    );
    assert_runs_with_output(
        r#"
use system.io
var x = 20
println(f"{x}")
        "#,
        "20",
    );
}

#[test]
fn explicit_typing() {
    assert_runs_with_output(
        r#"
use system.io

let x int = 42
println(f"{x}")
        "#,
        "42",
    );
    assert_runs_with_output(
        r#"
use system.io

var y i64 = 100
println(f"{y}")
        "#,
        "100",
    );
}

#[test]
fn mutability_checks() {
    assert_runs_with_output(
        r#"
use system.io

var x = 10
x = 20
println(f"{x}")
        "#,
        "20",
    );

    assert_compiler_error(
        r#"
let x = 10
x = 20
        "#,
        "Cannot assign to immutable variable 'x'",
    );
}

#[test]
fn scope_visibility() {
    assert_runs_with_output(
        r#"
use system.io

let x = 10
let result = if true
    let y = 20
    x + y
else
    0
println(f"{result}")
        "#,
        "30",
    );

    assert_runs_with_output(
        r#"
use system.io

let x = 10
if true
    let x = 20
println(f"{x}")
        "#,
        "10",
    );

    assert_compiler_error(
        r#"
if true:
    let x = 10
x
        "#,
        "Undefined variable",
    );
}

#[test]
fn incorrect_types() {
    assert_compiler_error(
        r#"
        let x int = "hello"
        "#,
        "Type mismatch",
    );

    assert_compiler_error(
        r#"
        var b bool = 1
        "#,
        "Type mismatch",
    );

    assert_compiler_error(
        r#"
        let x = 10
        x = "string"
        "#,
        "Type mismatch",
    );
}

#[test]
fn compatible_types() {
    // Testing implicit widening/compatibility
    // i8 -> i32
    assert_runs(
        r#"
        let small i8 = 10
        let big i32 = small
        "#,
    );

    // f32 -> f64
    assert_runs(
        r#"
        let small f32 = 1.0
        let big f64 = small
        "#,
    );

    // int literal to float
    assert_compiler_error(
        r#"
        let f float = 10
        "#,
        "Type mismatch",
    );
}

#[test]
fn nullable_types() {
    // Declaration initialized with value
    assert_runs("let x int? = 10");

    // Declaration initialized with None
    assert_runs("let x int? = None");

    // Assignment of None
    assert_runs(
        r#"
        var x int? = 10
        x = None
        "#,
    );

    // Assignment of value back to nullable
    assert_runs(
        r#"
        var x int? = None
        x = 20
        "#,
    );

    // Non-nullable cannot be None
    assert_compiler_error(
        r#"
        let x int = None
        "#,
        "Type mismatch",
    );

    // Null coalescing ?? operator
    assert_runs(
        r#"
        var x int? = 10
        let y int = x ?? 0
        "#,
    );

    // Some() constructor
    assert_runs("let x = Some(42)");
}
