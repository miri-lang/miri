// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_float_literals() {
    check_exprs_type(vec![
        ("1.5", Type::F32),
        ("0.0", Type::F32),
        ("-1.5", Type::F32),
        ("1.1234567890123456789", Type::F64), // High precision forces F64
    ]);
}

#[test]
fn test_float_arithmetic_expressions() {
    check_exprs_type(vec![
        ("1.0 + 2.0", Type::F32),
        ("1.0 - 2.0", Type::F32),
        ("1.0 * 2.0", Type::F32),
        ("1.0 / 2.0", Type::F32),
        ("1.0 % 2.0", Type::F32),
    ]);
}

#[test]
fn test_float_unary_expressions() {
    check_exprs_type(vec![
        ("-1.0", Type::F32),
    ]);
}

#[test]
fn test_float_comparisons() {
    check_vars_type("
let a = 1.0 < 2.0
let b = 1.0 <= 2.0
let c = 1.0 > 2.0
let d = 1.0 >= 2.0
let e = 1.0 == 2.0
let f = 1.0 != 2.0
", vec![
        ("a", Type::Boolean),
        ("b", Type::Boolean),
        ("c", Type::Boolean),
        ("d", Type::Boolean),
        ("e", Type::Boolean),
        ("f", Type::Boolean),
    ]);
}

#[test]
fn test_valid_float_arithmetic_variables() {
    check_vars_type("
let x = 1.5 + 2.5
let y = x / 2.0
let z = y * 3.0
", vec![
        ("x", Type::F32),
        ("y", Type::F32),
        ("z", Type::F32),
    ]);
}

#[test]
fn test_explicit_float_type() {
    check_vars_type("
let x f32 = 1.5
let y f64 = 1.1234567890123456789
", vec![
        ("x", Type::F32),
        ("y", Type::F64),
    ]);
}

#[test]
fn test_mixed_numeric_types_error() {
    check_error("
let x = 1 + 2.5
", "Type mismatch");
}

#[test]
fn test_float_int_mismatch() {
    check_error("
let x = 1.0 + 2
", "Type mismatch");
}

#[test]
fn test_float_bool_mismatch() {
    check_error("
let x = 1.0 + true
", "Invalid types for arithmetic operation");
}

#[test]
fn test_explicit_type_mismatch() {
    check_error("
let x f32 = 1
", "Type mismatch for variable 'x'");
    
    check_error("
let x f32 = 1.1234567890123456789
", "Type mismatch for variable 'x'");
}

#[test]
fn test_float_assignment_operators() {
    check_vars_type("
var x = 1.0
x += 2.0
", vec![
        ("x", Type::F32),
    ]);
}

#[test]
fn test_invalid_float_assignment() {
    check_error("
var x = 1.0
x += 1
", "Type mismatch in assignment");
}
