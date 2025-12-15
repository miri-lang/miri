// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_integer_literals() {
    assert_expressions_type(vec![
        ("1", Type::Int),
        ("0", Type::Int),
        ("-1", Type::Int),
        ("1234567890", Type::Int),
    ]);
}

#[test]
fn test_integer_arithmetic_expressions() {
    assert_expressions_type(vec![
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
    assert_expressions_type(vec![
        ("-1", Type::Int),
        ("+1", Type::Int),
        ("-(1 + 2)", Type::Int),
    ]);
}

#[test]
fn test_integer_comparisons() {
    assert_expressions_type(vec![
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
    assert_expressions_type(vec![
        ("1 & 2", Type::Int),
        ("1 | 2", Type::Int),
        ("1 ^ 2", Type::Int),
        ("~1", Type::Int),
    ]);
}

#[test]
fn test_valid_integer_arithmetic_variables() {
    assert_variable_types("
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
    assert_variable_types("
let x int = 1
let y int = -5
", vec![
        ("x", Type::Int),
        ("y", Type::Int),
    ]);
}

#[test]
fn test_integer_assignment_operators() {
    assert_variable_types("
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
    assert_type_check_error("
let x int = 1.5
", "Type mismatch for variable 'x'");
    
    assert_type_check_error("
let x int = true
", "Type mismatch for variable 'x'");
}

#[test]
fn test_integer_bool_mismatch() {
    assert_type_check_error("
let x = 1 + true
", "Invalid types for arithmetic operation");
}

#[test]
fn test_integer_float_mismatch() {
    assert_type_check_error("
let x = 1 + 1.5
", "Type mismatch: Int and F32 are not compatible for arithmetic operation");
}

#[test]
fn test_invalid_integer_assignment() {
    assert_type_check_error("
var x = 1
x = 1.5
", "Type mismatch in assignment");
}

#[test]
fn test_invalid_bitwise_operands() {
    assert_type_check_error("
let x = 1 & 1.5
", "Invalid types for bitwise operation");
    
    assert_type_check_error("
let x = 1 | true
", "Invalid types for bitwise operation");
}
