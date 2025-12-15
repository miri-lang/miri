// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_boolean_literals() {
    assert_expressions_type(vec![
        ("true", Type::Boolean),
        ("false", Type::Boolean),
    ]);
}

#[test]
fn test_boolean_expressions() {
    assert_expressions_type(vec![
        ("true and false", Type::Boolean),
        ("true or false", Type::Boolean),
        ("not true", Type::Boolean),
        ("true and (false or true)", Type::Boolean),
    ]);
}

#[test]
fn test_boolean_logic() {
    assert_variable_types("
let x = true and false
let y = not x
let z = x or y
", vec![
        ("x", Type::Boolean),
        ("y", Type::Boolean),
        ("z", Type::Boolean),
    ]);
}

#[test]
fn test_equality() {
    assert_expressions_type(vec![
        ("true == false", Type::Boolean),
        ("true != false", Type::Boolean),
        ("1 == 1", Type::Boolean),
        ("1 != 2", Type::Boolean),
        ("1.5 == 1.5", Type::Boolean),
        // ("\"a\" == \"b\"", Type::Boolean), // TODO: Enable when string equality is supported
    ]);
}

#[test]
fn test_comparison() {
    assert_variable_types("
let x = 1 > 2
let y = 1.5 <= 2.5
", vec![
        ("x", Type::Boolean),
        ("y", Type::Boolean),
    ]);
}

#[test]
fn test_explicit_type() {
    assert_variable_types("
let x bool = true
let y bool = false
", vec![
        ("x", Type::Boolean),
        ("y", Type::Boolean),
    ]);
}

#[test]
fn test_invalid_boolean_logic_and() {
    assert_type_check_error("
let x = true and 1
", "Logical operations require booleans");
}

#[test]
fn test_invalid_boolean_logic_or() {
    assert_type_check_error("
let x = 1 or false
", "Logical operations require booleans");
}

#[test]
fn test_invalid_boolean_logic_not() {
    assert_type_check_error("
let x = not 1
", "Logical NOT requires boolean");
}

#[test]
fn test_invalid_equality_types() {
    assert_type_check_error("
let x = 1 == true
", "Type mismatch");
}

#[test]
fn test_boolean_comparison() {
    // Boolean comparison is valid (e.g. true > false)
    assert_variable_types("
let x = true > false
", vec![
        ("x", Type::Boolean),
    ]);
}

#[test]
fn test_if_condition_type_mismatch() {
    assert_type_check_error("
if 1
    let x = 1
", "If condition must be a boolean");
}

#[test]
fn test_while_condition_type_mismatch() {
    assert_type_check_error("
while 1
    let x = 1
", "While condition must be a boolean");
}

#[test]
fn test_conditional_expression_type_mismatch() {
    // TODO: Implement check for conditional expression type
    // assert_type_check_error("
    // let x = 10 if 1 else 20
    // ", "Condition must be boolean");
}
