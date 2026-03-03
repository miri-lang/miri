// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_float_literals() {
    type_checker_exprs_type_test(vec![
        ("1.5", type_f32()),
        ("0.0", type_f32()),
        ("-1.5", type_f32()),
        ("1.1234567890123456789", type_f64()), // High precision forces F64
    ]);
}

#[test]
fn test_float_arithmetic_expressions() {
    type_checker_exprs_type_test(vec![
        ("1.0 + 2.0", type_f32()),
        ("1.0 - 2.0", type_f32()),
        ("1.0 * 2.0", type_f32()),
        ("1.0 / 2.0", type_f32()),
        ("1.0 % 2.0", type_f32()),
    ]);
}

#[test]
fn test_float_unary_expressions() {
    type_checker_exprs_type_test(vec![("-1.0", type_f32())]);
}

#[test]
fn test_float_comparisons() {
    type_checker_vars_type_test(
        "
let a = 1.0 < 2.0
let b = 1.0 <= 2.0
let c = 1.0 > 2.0
let d = 1.0 >= 2.0
let e = 1.0 == 2.0
let f = 1.0 != 2.0
",
        vec![
            ("a", type_bool()),
            ("b", type_bool()),
            ("c", type_bool()),
            ("d", type_bool()),
            ("e", type_bool()),
            ("f", type_bool()),
        ],
    );
}

#[test]
fn test_valid_float_arithmetic_variables() {
    type_checker_vars_type_test(
        "
let x = 1.5 + 2.5
let y = x / 2.0
let z = y * 3.0
",
        vec![("x", type_f32()), ("y", type_f32()), ("z", type_f32())],
    );
}

#[test]
fn test_explicit_float_type() {
    type_checker_vars_type_test(
        "
let x f32 = 1.5
let y f64 = 1.1234567890123456789
",
        vec![("x", type_f32()), ("y", type_f64())],
    );
}

#[test]
fn test_mixed_numeric_types_error() {
    type_checker_error_test(
        "
let x = 1 + 2.5
",
        "Type mismatch",
    );
}

#[test]
fn test_float_int_mismatch() {
    type_checker_error_test(
        "
let x = 1.0 + 2
",
        "Type mismatch",
    );
}

#[test]
fn test_float_bool_mismatch() {
    type_checker_error_test(
        "
let x = 1.0 + true
",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_explicit_type_mismatch() {
    type_checker_error_test(
        "
let x f32 = 1
",
        "Type mismatch for variable 'x'",
    );

    type_checker_error_test(
        "
let x f32 = 1.1234567890123456789
",
        "Type mismatch for variable 'x'",
    );
}

#[test]
fn test_float_assignment_operators() {
    type_checker_vars_type_test(
        "
var x = 1.0
x += 2.0
",
        vec![("x", type_f32())],
    );
}

#[test]
fn test_invalid_float_assignment() {
    type_checker_error_test(
        "
var x = 1.0
x += 1
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_f64_arithmetic() {
    // Using high precision literals to force F64
    let f64_val = "1.1234567890123456789";
    type_checker_exprs_type_test(vec![
        (&format!("{} + {}", f64_val, f64_val), type_f64()),
        (&format!("{} - {}", f64_val, f64_val), type_f64()),
        (&format!("{} * {}", f64_val, f64_val), type_f64()),
        (&format!("{} / {}", f64_val, f64_val), type_f64()),
    ]);
}

#[test]
fn test_f32_f64_mismatch() {
    type_checker_error_test("1.0 + 1.1234567890123456789", "Type mismatch");
}

#[test]
fn test_float_bitwise_invalid() {
    type_checker_error_test("1.0 & 2.0", "Invalid types for bitwise operation");
    type_checker_error_test("1.0 | 2.0", "Invalid types for bitwise operation");
    type_checker_error_test("1.0 ^ 2.0", "Invalid types for bitwise operation");
}

#[test]
fn test_float_unary_plus() {
    type_checker_expr_type_test("+1.0", type_f32());
}

#[test]
fn test_float_function() {
    type_checker_test(
        "
fn add(a f32, b f32) f32
    return a + b

let x = add(1.0, 2.0)
",
    );
}

#[test]
fn test_float_list() {
    type_checker_expr_type_test("[1.0, 2.0, 3.0]", type_list(type_f32()));
}

#[test]
fn test_float_list_mismatch() {
    type_checker_error_test("[1.0, 1]", "List elements must have the same type");
}

#[test]
fn test_nullable_float() {
    type_checker_expr_type_test(
        "
let x f32? = 1.0
x
",
        type_option(type_f32()),
    );
    type_checker_expr_type_test(
        "
let y f32? = None
y
",
        type_option(type_f32()),
    );
}

#[test]
fn test_assignment_compatibility() {
    // Float literal to specific type is allowed
    type_checker_test("let a f32 = 1.0");

    // Specific type to Float (variable) - inferred as specific type
    type_checker_vars_type_test("let a f32 = 1.0\nlet b = a", vec![("b", type_f32())]);

    // Smaller to larger - allowed
    type_checker_vars_type_test("let a f32 = 1.0\nlet b f64 = a", vec![("b", type_f64())]);

    // Larger to smaller - fail
    type_checker_error_test("let a f64 = 1.0\nlet b f32 = a", "Type mismatch");
}
