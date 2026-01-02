// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_float_literals() {
    check_exprs_type(vec![
        ("1.5", type_f32()),
        ("0.0", type_f32()),
        ("-1.5", type_f32()),
        ("1.1234567890123456789", type_f64()), // High precision forces F64
    ]);
}

#[test]
fn test_float_arithmetic_expressions() {
    check_exprs_type(vec![
        ("1.0 + 2.0", type_f32()),
        ("1.0 - 2.0", type_f32()),
        ("1.0 * 2.0", type_f32()),
        ("1.0 / 2.0", type_f32()),
        ("1.0 % 2.0", type_f32()),
    ]);
}

#[test]
fn test_float_unary_expressions() {
    check_exprs_type(vec![("-1.0", type_f32())]);
}

#[test]
fn test_float_comparisons() {
    check_vars_type(
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
    check_vars_type(
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
    check_vars_type(
        "
let x f32 = 1.5
let y f64 = 1.1234567890123456789
",
        vec![("x", type_f32()), ("y", type_f64())],
    );
}

#[test]
fn test_mixed_numeric_types_error() {
    check_error(
        "
let x = 1 + 2.5
",
        "Type mismatch",
    );
}

#[test]
fn test_float_int_mismatch() {
    check_error(
        "
let x = 1.0 + 2
",
        "Type mismatch",
    );
}

#[test]
fn test_float_bool_mismatch() {
    check_error(
        "
let x = 1.0 + true
",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_explicit_type_mismatch() {
    check_error(
        "
let x f32 = 1
",
        "Type mismatch for variable 'x'",
    );

    check_error(
        "
let x f32 = 1.1234567890123456789
",
        "Type mismatch for variable 'x'",
    );
}

#[test]
fn test_float_assignment_operators() {
    check_vars_type(
        "
var x = 1.0
x += 2.0
",
        vec![("x", type_f32())],
    );
}

#[test]
fn test_invalid_float_assignment() {
    check_error(
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
    check_exprs_type(vec![
        (&format!("{} + {}", f64_val, f64_val), type_f64()),
        (&format!("{} - {}", f64_val, f64_val), type_f64()),
        (&format!("{} * {}", f64_val, f64_val), type_f64()),
        (&format!("{} / {}", f64_val, f64_val), type_f64()),
    ]);
}

#[test]
fn test_f32_f64_mismatch() {
    check_error("1.0 + 1.1234567890123456789", "Type mismatch");
}

#[test]
fn test_float_bitwise_invalid() {
    check_error("1.0 & 2.0", "Invalid types for bitwise operation");
    check_error("1.0 | 2.0", "Invalid types for bitwise operation");
    check_error("1.0 ^ 2.0", "Invalid types for bitwise operation");
}

#[test]
fn test_float_unary_plus() {
    check_expr_type("+1.0", type_f32());
}

#[test]
fn test_float_function() {
    check_success(
        "
fn add(a f32, b f32) f32
    return a + b

let x = add(1.0, 2.0)
",
    );
}

#[test]
fn test_float_list() {
    check_expr_type("[1.0, 2.0, 3.0]", type_list(type_f32()));
}

#[test]
fn test_float_list_mismatch() {
    check_error("[1.0, 1]", "List elements must have the same type");
}

#[test]
fn test_nullable_float() {
    check_expr_type(
        "
let x f32? = 1.0
x
",
        type_null(type_f32()),
    );
    check_expr_type(
        "
let y f32? = None
y
",
        type_null(type_f32()),
    );
}

#[test]
fn test_assignment_compatibility() {
    // Float literal to specific type is allowed
    check_success("let a f32 = 1.0");

    // Specific type to Float (variable) - inferred as specific type
    check_vars_type("let a f32 = 1.0\nlet b = a", vec![("b", type_f32())]);

    // Smaller to larger - allowed
    check_vars_type("let a f32 = 1.0\nlet b f64 = a", vec![("b", type_f64())]);

    // Larger to smaller - fail
    check_error("let a f64 = 1.0\nlet b f32 = a", "Type mismatch");
}
