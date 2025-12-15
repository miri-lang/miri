// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_integer_literals() {
    check_exprs_type(vec![
        ("1", Type::Int),
        ("0", Type::Int),
        ("-1", Type::Int),
        ("1234567890", Type::Int),
    ]);
}

#[test]
fn test_integer_arithmetic_expressions() {
    check_exprs_type(vec![
        ("1 + 2", Type::Int),
        ("1 - 2", Type::Int),
        ("1 * 2", Type::Int),
        ("1 / 2", Type::Int),
        ("1 % 2", Type::Int),
        ("1 + 2 * 3", Type::Int),
        ("(1 + 2) * 3", Type::Int),
    ]);
}

#[test]
fn test_integer_unary_expressions() {
    check_exprs_type(vec![
        ("-1", Type::Int),
        ("+1", Type::Int),
        ("-(1 + 2)", Type::Int),
    ]);
}

#[test]
fn test_integer_comparisons() {
    check_exprs_type(vec![
        ("1 < 2", Type::Boolean),
        ("1 <= 2", Type::Boolean),
        ("1 > 2", Type::Boolean),
        ("1 >= 2", Type::Boolean),
        ("1 == 2", Type::Boolean),
        ("1 != 2", Type::Boolean),
    ]);
}

#[test]
fn test_integer_bitwise_operations() {
    check_exprs_type(vec![
        ("1 & 2", Type::Int),
        ("1 | 2", Type::Int),
        ("1 ^ 2", Type::Int),
        ("~1", Type::Int),
    ]);
}

#[test]
fn test_valid_integer_arithmetic_variables() {
    check_vars_type("
let x = 1 + 2
let y = x * 3
let z = y / x
let w = z % 2
", vec![
        ("x", Type::Int),
        ("y", Type::Int),
        ("z", Type::Int),
        ("w", Type::Int),
    ]);
}

#[test]
fn test_explicit_integer_type() {
    check_vars_type("
let x int = 1
let y int = -5
", vec![
        ("x", Type::Int),
        ("y", Type::Int),
    ]);
}

#[test]
fn test_integer_assignment_operators() {
    check_vars_type("
var x = 1
x += 2
x -= 1
x *= 3
x /= 2
x %= 2
", vec![("x", Type::Int)]);
}


#[test]
fn test_explicit_type_mismatch() {
    check_error("
let x int = 1.5
", "Type mismatch for variable 'x'");
    
    check_error("
let x int = true
", "Type mismatch for variable 'x'");
}

#[test]
fn test_integer_bool_mismatch() {
    check_error("
let x = 1 + true
", "Invalid types for arithmetic operation");
}

#[test]
fn test_integer_float_mismatch() {
    check_error("
let x = 1 + 1.5
", "Type mismatch: Int and F32 are not compatible for arithmetic operation");
}

#[test]
fn test_invalid_integer_assignment() {
    check_error("
var x = 1
x = 1.5
", "Type mismatch in assignment");
}

#[test]
fn test_invalid_bitwise_operands() {
    check_error("
let x = 1 & 1.5
", "Invalid types for bitwise operation");
    
    check_error("
let x = 1 | true
", "Invalid types for bitwise operation");
}
