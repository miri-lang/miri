// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::{type_check_test, type_check_error_test};


#[test]
fn test_valid_integer_arithmetic() {
    type_check_test("
let x = 1 + 2
let y = x * 3
");
}

#[test]
fn test_valid_float_arithmetic() {
    type_check_test("
let x = 1.5 + 2.5
let y = x / 2.0
");
}

#[test]
fn test_mixed_numeric_types_error() {
    type_check_error_test("
let x = 1 + 2.5
", "Type mismatch");
}

#[test]
fn test_invalid_arithmetic_types() {
    type_check_error_test("
let x = 1 + true
", "Invalid types for arithmetic operation");
}

#[test]
fn test_variable_type_mismatch() {
    type_check_error_test("
let x int = 1.5
", "Type mismatch for variable 'x'");
}

#[test]
fn test_boolean_logic() {
    type_check_test("
let x = true and false
let y = not x
");
}

#[test]
fn test_invalid_boolean_logic() {
    type_check_error_test("
let x = true and 1
", "Logical operations require booleans");
}

#[test]
fn test_comparison() {
    type_check_test("
let x = 1 > 2
let y = 1.5 <= 2.5
");
}
