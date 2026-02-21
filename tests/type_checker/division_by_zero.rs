// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_divide_by_zero() {
    type_checker_error_test("let x = 10 / 0", "Division by zero");
}

#[test]
fn test_divide_by_zero_float() {
    type_checker_error_test("let x = 10.0 / 0.0", "Division by zero");
}

#[test]
fn test_modulo_by_zero() {
    type_checker_error_test("let x = 10 % 0", "Division by zero");
}

#[test]
fn test_modulo_by_zero_float() {
    type_checker_error_test("let x = 10.0 % 0.0", "Division by zero");
}

#[test]
fn test_divide_assign_by_zero() {
    type_checker_error_test("var x = 10\nx /= 0", "Division by zero");
}

#[test]
fn test_modulo_assign_by_zero() {
    type_checker_error_test("var x = 10\nx %= 0", "Division by zero");
}

#[test]
fn test_divide_by_non_zero_literal() {
    type_checker_test("let x = 10 / 2");
}

#[test]
fn test_divide_by_variable() {
    // At compile time, the variable `y` is not evaluated as a literal `0` unless it's a `const` and we've implemented constant propagation.
    // Currently, our check only looks for direct literals.
    type_checker_test("let y = 0\nlet x = 10 / y");
}
