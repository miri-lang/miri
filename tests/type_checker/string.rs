// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_string_literals() {
    assert_expressions_type(vec![
        ("\"hello\"", Type::String),
        ("\"\"", Type::String),
        ("'hello'", Type::String),
    ]);
}

#[test]
fn test_string_concatenation() {
    assert_expressions_type(vec![
        ("\"hello\" + \" world\"", Type::String),
        ("'a' + 'b'", Type::String),
    ]);
}

#[test]
fn test_string_comparisons() {
    assert_expressions_type(vec![
        ("\"a\" == \"b\"", Type::Boolean),
        ("\"a\" != \"b\"", Type::Boolean),
        ("\"a\" < \"b\"", Type::Boolean),
        ("\"a\" <= \"b\"", Type::Boolean),
        ("\"a\" > \"b\"", Type::Boolean),
        ("\"a\" >= \"b\"", Type::Boolean),
    ]);
}

#[test]
fn test_explicit_string_type() {
    assert_variable_types("
let x string = \"hello\"
let y string = 'world'
", vec![
        ("x", Type::String),
        ("y", Type::String),
    ]);
}

#[test]
fn test_string_assignment_operators() {
    assert_variable_types("
var x = \"hello\"
x += \" world\"
", vec![("x", Type::String)]);
}

#[test]
fn test_string_int_mismatch() {
    assert_type_check_error("
let x = \"hello\" + 1
", "Invalid types for arithmetic operation");
}

#[test]
fn test_string_bool_mismatch() {
    assert_type_check_error("
let x = \"hello\" + true
", "Invalid types for arithmetic operation");
}

#[test]
fn test_invalid_string_assignment() {
    assert_type_check_error("
var x = \"hello\"
x = 1
", "Type mismatch in assignment");
}
