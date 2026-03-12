// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
