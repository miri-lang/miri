// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_formatted_string() {
    check_exprs_type(vec![
        ("f\"hello {1}\"", Type::String),
        ("f\"val: {true}\"", Type::String),
        ("f\"{1} + {2} = {3}\"", Type::String),
    ]);
}

#[test]
fn test_formatted_string_with_variables() {
    check_vars_type(
        "
let name = \"World\"
let greeting = f\"Hello {name}\"
",
        vec![("greeting", Type::String)],
    );
}

#[test]
fn test_formatted_string_nested_expressions() {
    check_exprs_type(vec![("f\"result: {1 + 2}\"", Type::String)]);
}

#[test]
fn test_formatted_string_undefined_variable() {
    check_error(
        "f\"Hello {undefined_var}\"",
        "Undefined variable: undefined_var",
    );
}

#[test]
fn test_formatted_string_type_error_in_expression() {
    check_error(
        "f\"result: {1 + 'x'}\"",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_formatted_string_function_call() {
    check_success(
        "
fn get_name() string: \"World\"
let s = f\"Hello {get_name()}\"
",
    );
}

#[test]
fn test_formatted_string_complex_expression() {
    check_expr_type("f\"Value: {1 + 2 * 3}\"", Type::String);
    check_expr_type("f\"Bool: {true and false}\"", Type::String);
}

#[test]
fn test_formatted_string_member_access() {
    check_success(
        "
struct Point: x int, y int
let p = Point(1, 2)
let s = f\"Point: {p.x}, {p.y}\"
",
    );
}

#[test]
fn test_formatted_string_empty() {
    check_expr_type("f\"\"", Type::String);
}

#[test]
fn test_formatted_string_only_expression() {
    check_expr_type("f\"{1}\"", Type::String);
}

#[test]
fn test_formatted_string_void_expression() {
    // Void expressions are currently allowed in formatted strings
    check_success(
        "
fn void_func()
    return
let s = f\"Void: {void_func()}\"
",
    );
}

#[test]
fn test_formatted_string_multiline() {
    check_expr_type(
        "f\"
    Line 1
    Line 2 {1}
    \"",
        Type::String,
    );
}
