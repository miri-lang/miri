// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{
    assert_compiler_error, assert_compiler_warning, assert_operation_outputs, assert_runtime_error,
};

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
fn test_binary_operations_on_integers() {
    assert_operation_outputs(&[
        ("123 + 456", "579"),
        ("123 - 456", "-333"),
        ("123 * 456", "56088"),
        ("123 / 456", "0"),
        ("123 % 456", "123"),
    ]);
}

#[test]
fn test_double_negation() {
    assert_compiler_warning("--5", "use of a double negation");
}

#[test]
fn test_division_by_zero_compile_time() {
    // Miri should catch division by zero at compile time, at least for basic cases.
    let examples = [
        "5 / 0",
        "123 / 0.0",
        "10 % 0",
        "10 % -0",
        "0 / 0",
        "0.0 / 0.0",
        "
// Compiler should detect this as well
let x = 0
let y = 1

1 / x
",
        "
// And this (because of optimization)
let x = 1
let y = 1
let z = 1

1 / (x - y)
",
    ];

    for example in examples {
        assert_compiler_error(example, "attempt to divide by zero");
    }
}

#[test]
fn test_division_by_zero_runtime() {
    // Trickier cases that should be caught at runtime
    // TODO: add more examples
    let examples = ["
var x = 10

while x > 0: x -= 1

1 / x
"];

    for example in examples {
        assert_runtime_error(example, "attempt to divide by zero");
    }
}

#[test]
fn test_binary_operations_on_floats() {
    assert_operation_outputs(&[
        ("1.5 + 2.5", "4.0"),
        ("3.0 * 2.5", "7.5"),
        ("10.0 / 4.0", "2.5"),
        ("1.0 - 0.5", "0.5"),
        ("-1.5 * 2.0", "-3.0"),
    ]);
}

#[test]
fn test_mixed_type_operations() {
    assert_compiler_error("1 + 2.5", "Type mismatch: cannot add a float to an integer");
}

#[test]
fn test_precedence() {
    assert_operation_outputs(&[
        // Check * and / have higher precedence than + and -
        ("2 + 3 * 4", "14"),
        ("2 * 3 + 4", "10"),
        ("20 / 4 + 2", "7"),
        ("2 + 20 / 4", "7"),
        ("10 - 2 * 3", "4"),
        ("10 * 2 - 3", "17"),
        // Check left-to-right associativity
        ("10 - 5 - 2", "3"),   // (10-5)-2=3
        ("100 / 10 / 2", "5"), // (100/10)/2=5
        ("20 % 10 % 3", "0"),
        // Mixed precedence and associativity
        ("2 + 3 * 4 - 5", "9"),        // 2 + 12 - 5
        ("2 * 3 + 4 * 5", "26"),       // 6 + 20
        ("100 / 2 / 2 * 3 + 1", "76"), // 50 / 2 * 3 + 1 -> 25 * 3 + 1 -> 75 + 1
    ]);
}

#[test]
fn test_parentheses() {
    assert_operation_outputs(&[
        // Basic override precedence
        ("(2 + 3) * 4", "20"),
        ("2 * (3 + 4)", "14"),
        ("20 / (4 + 6)", "2"),
        // Nested parentheses
        ("((2 + 3) * 4)", "20"),
        ("(2 + (3 * 4))", "14"),
        ("((10 - 2) * (3 + 1)) / 4", "8"), // (8 * 4) / 4 = 8
        // "Crazy" nesting
        ("(((1 + 2) * 3) + 4) * 5", "65"), // (3 * 3 + 4) * 5 -> (9 + 4) * 5 -> 13 * 5 = 65
        ("(1 + (2 * (3 + (4 * (5 + 6)))))", "95"), // 1 + (2 * (3 + (4 * 11))) -> 1 + (2 * (3 + 44)) -> 1 + (2 * 47) -> 1 + 94 = 95
        ("((((((1 + 1))))))", "2"),
    ]);
}
