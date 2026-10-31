// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_boolean_literals() {
    assert_runs_with_output(
        r#"
println(f"{true}")
println(f"{false}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_boolean_not() {
    assert_operation_outputs(&[
        ("if not false: 1 else: 0", "1"),
        ("if not true: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_boolean_comparisons() {
    assert_operation_outputs(&[
        ("if true == true: 1 else: 0", "1"),
        ("if true != false: 1 else: 0", "1"),
        ("if false == false: 1 else: 0", "1"),
    ]);
}
