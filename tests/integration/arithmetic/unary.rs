// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_unary_operations_on_integers() {
    assert_operation_outputs(&[
        ("-123", "-123"),
        ("-(-123)", "123"),
        ("--10", "10"),
        ("+10", "10"),
        ("++10", "10"),
        ("+(-10)", "-10"),
        ("-(-10)", "10"),
        ("-(2 + 3)", "-5"),
        ("-2 * 3", "-6"),
        ("2 * -3", "-6"),
        ("5 - -3", "8"),
        ("-(-(-5))", "-5"),
        ("-(-(-(-5)))", "5"),
    ]);
}

#[test]
fn test_double_negation() {
    assert_compiler_warning("--5", "Decrement operator not supported");
}
