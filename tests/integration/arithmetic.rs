// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{
    assert_returns_many, assert_returns_with_warning, assert_runs_many, assert_runtime_error,
};

#[test]
fn test_binary_operations_on_integers() {
    assert_returns_many(&[
        ("123 + 456", 579),
        ("123 - 456", -333),
        ("123 * 456", 56088),
        ("123 / 456", 0),
        ("123 % 456", 123),
    ]);
}

#[test]
fn test_division_by_zero() {
    assert_runtime_error("5 / 0", "attempt to divide by zero");
    assert_runtime_error(
        "10 % 0",
        "attempt to calculate the remainder with a divisor of zero",
    );
}

#[test]
fn test_double_negation() {
    assert_returns_with_warning("--5", 5, "use of a double negation");
}

#[test]
fn test_binary_operations_on_floats() {
    assert_runs_many(&[
        "1.5 + 2.5",
        "3.0 * 2.5",
        "10.0 / 4.0",
        "1.0 - 0.5",
        "-1.5 * 2.0",
    ]);
}

#[test]
fn test_mixed_type_operations() {
    assert_runtime_error("1 + 2.5", "Type mismatch: cannot add a float to an integer");
}

#[test]
fn test_precedence() {
    assert_returns_many(&[
        // Check * and / have higher precedence than + and -
        ("2 + 3 * 4", 14),
        ("2 * 3 + 4", 10),
        ("20 / 4 + 2", 7),
        ("2 + 20 / 4", 7),
        ("10 - 2 * 3", 4),
        ("10 * 2 - 3", 17),
        // Check left-to-right associativity
        ("10 - 5 - 2", 3),   // (10-5)-2=3
        ("100 / 10 / 2", 5), // (100/10)/2=5
        ("20 % 10 % 3", 0),
        // Mixed precedence and associativity
        ("2 + 3 * 4 - 5", 9),        // 2 + 12 - 5
        ("2 * 3 + 4 * 5", 26),       // 6 + 20
        ("100 / 2 / 2 * 3 + 1", 76), // 50 / 2 * 3 + 1 -> 25 * 3 + 1 -> 75 + 1
    ]);
}

#[test]
fn test_parentheses() {
    assert_returns_many(&[
        // Basic override precedence
        ("(2 + 3) * 4", 20),
        ("2 * (3 + 4)", 14),
        ("20 / (4 + 6)", 2),
        // Nested parentheses
        ("((2 + 3) * 4)", 20),
        ("(2 + (3 * 4))", 14),
        ("((10 - 2) * (3 + 1)) / 4", 8), // (8 * 4) / 4 = 8
        // "Crazy" nesting
        ("(((1 + 2) * 3) + 4) * 5", 65), // (3 * 3 + 4) * 5 -> (9 + 4) * 5 -> 13 * 5 = 65
        ("(1 + (2 * (3 + (4 * (5 + 6)))))", 95), // 1 + (2 * (3 + (4 * 11))) -> 1 + (2 * (3 + 44)) -> 1 + (2 * 47) -> 1 + 94 = 95
        ("((((((1 + 1))))))", 2),
    ]);
}

#[test]
fn test_unary_operators() {
    assert_returns_many(&[
        ("-5", -5),
        ("-(-5)", 5),
        ("-(2 + 3)", -5),
        ("-2 * 3", -6),
        ("2 * -3", -6),
        ("5 - -3", 8),
        ("-(-(-5))", -5),
        ("-(-(-(-5)))", 5),
    ]);
}
