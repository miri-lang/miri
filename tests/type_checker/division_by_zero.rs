// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_vars_type_test};
use miri::ast::factory::make_type;
use miri::ast::types::TypeKind;

#[test]
fn test_divide_by_zero() {
    type_checker_error_test("let x = 10 / 0", "Division by zero");
    type_checker_error_test("let x = 10.0 / 0.0", "Division by zero");
    type_checker_error_test("let x = 10 / +0", "Division by zero");
    type_checker_error_test("let x = 10.0 / +0.0", "Division by zero");
    type_checker_error_test("let x i8 = 10 / 0", "Division by zero");
    type_checker_error_test("let x f32 = 10.0 / 0.0", "Division by zero");
    type_checker_error_test("let x = 10 / (0)", "Division by zero");
    type_checker_error_test("let x = 10 / (+0)", "Division by zero");
    type_checker_error_test("let x = 10.0 / (-0.0)", "Division by zero");
}

#[test]
fn test_modulo_by_zero() {
    type_checker_error_test("let x = 10 % 0", "Division by zero");
    type_checker_error_test("let x = 10.0 % 0.0", "Division by zero");
    type_checker_error_test("let x = 10 % +0", "Division by zero");
    type_checker_error_test("let x = 10.0 % +0.0", "Division by zero");
    type_checker_error_test("let x u64 = 10 % 0", "Division by zero");
    type_checker_error_test("let x f64 = 10.0 % 0.0", "Division by zero");
}

#[test]
fn test_divide_assign_by_zero() {
    type_checker_error_test("var x = 10\nx /= 0", "Division by zero");
    type_checker_error_test("var x = 10\nx /= -0", "Division by zero");
    type_checker_error_test("var x = 10\nx /= +0", "Division by zero");
    type_checker_error_test("var x = 10.0\nx /= 0.0", "Division by zero");
    type_checker_error_test("var x = 10.0\nx /= -0.0", "Division by zero");
    type_checker_error_test("var x = 10.0\nx /= +0.0", "Division by zero");
}

#[test]
fn test_modulo_assign_by_zero() {
    type_checker_error_test("var x = 10\nx %= 0", "Division by zero");
    type_checker_error_test("var x = 10\nx %= -0", "Division by zero");
    type_checker_error_test("var x = 10\nx %= +0", "Division by zero");
    type_checker_error_test("var x = 10.0\nx %= 0.0", "Division by zero");
    type_checker_error_test("var x = 10.0\nx %= -0.0", "Division by zero");
    type_checker_error_test("var x = 10.0\nx %= +0.0", "Division by zero");
}

#[test]
fn test_divide_by_non_zero_literal() {
    type_checker_vars_type_test("let x = 10 / 2", vec![("x", make_type(TypeKind::Int))]);
}

#[test]
fn test_divide_by_negative_non_zero_literal() {
    type_checker_vars_type_test("let x = 10 / -2", vec![("x", make_type(TypeKind::Int))]);
    type_checker_vars_type_test(
        "let x = 10.0 / -2.5",
        vec![("x", make_type(TypeKind::Float))],
    );
    type_checker_vars_type_test("var x = 10\nx /= -2", vec![("x", make_type(TypeKind::Int))]);
}

#[test]
fn test_divide_by_variable() {
    // At compile time, the variable `y` is not evaluated as a literal `0` unless it's a `const` and we've implemented constant propagation.
    // Currently, our check only looks for direct literals.
    type_checker_vars_type_test(
        "let y = 0\nlet x = 10 / y",
        vec![
            ("y", make_type(TypeKind::Int)),
            ("x", make_type(TypeKind::Int)),
        ],
    );
}

#[test]
fn test_divide_and_modulo_by_bare_negative_zero_literals() {
    type_checker_error_test("let x = 10 / -0", "Division by zero");
    type_checker_error_test("let x = 10.0 / -0.0", "Division by zero");
    type_checker_error_test("let x = 10 % -0", "Division by zero");
    type_checker_error_test("let x = 10.0 % -0.0", "Division by zero");
}

#[test]
fn test_division_by_zero_across_numeric_type_widths() {
    type_checker_error_test("let x u8 = 10 / 0", "Division by zero");
    type_checker_error_test("let x u16 = 10 / 0", "Division by zero");
    type_checker_error_test("let x u32 = 10 / 0", "Division by zero");
    type_checker_error_test("let x u64 = 10 / 0", "Division by zero");
    type_checker_error_test("let x u128 = 10 / 0", "Division by zero");
    type_checker_error_test("let x i16 = 10 / 0", "Division by zero");
    type_checker_error_test("let x i32 = 10 / 0", "Division by zero");
    type_checker_error_test("let x i64 = 10 / 0", "Division by zero");
    type_checker_error_test("let x i128 = 10 / 0", "Division by zero");
    type_checker_error_test("let x f16 = 1.0 / 0.0", "Division by zero");
    type_checker_error_test("let x f32 = 1.0 / -0.0", "Division by zero");
    type_checker_error_test("let x f64 = 1.0 / +0.0", "Division by zero");
}

#[test]
fn test_division_by_zero_in_nested_subexpressions_and_collections() {
    type_checker_error_test("let x = 1 + (10 / 0)", "Division by zero");
    type_checker_error_test("let arr = [10 / 0]", "Division by zero");
    type_checker_error_test("let m = {\"key\": 10 / 0}", "Division by zero");
    type_checker_error_test("let t = (1, 10 / 0)", "Division by zero");
}

#[test]
fn test_divide_by_near_zero_and_non_zero_expressions() {
    type_checker_vars_type_test(
        "let x = 10.0 / 1e-300",
        vec![("x", make_type(TypeKind::Float))],
    );
    type_checker_vars_type_test(
        "let x = 10.0 / 0.0001",
        vec![("x", make_type(TypeKind::Float))],
    );
    type_checker_vars_type_test(
        "let x = 10 / (1 - 0)",
        vec![("x", make_type(TypeKind::Int))],
    );
    type_checker_vars_type_test("let x = 10 / 1", vec![("x", make_type(TypeKind::Int))]);
}
