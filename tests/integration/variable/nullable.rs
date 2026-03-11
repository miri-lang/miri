// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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
