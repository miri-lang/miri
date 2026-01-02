// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_string_literals() {
    check_exprs_type(vec![
        ("\"hello\"", type_string()),
        ("\"\"", type_string()),
        ("'hello'", type_string()),
    ]);
}

#[test]
fn test_string_concatenation() {
    check_exprs_type(vec![
        ("\"hello\" + \" world\"", type_string()),
        ("'a' + 'b'", type_string()),
    ]);
}

#[test]
fn test_string_comparisons() {
    check_exprs_type(vec![
        ("\"a\" == \"b\"", type_bool()),
        ("\"a\" != \"b\"", type_bool()),
        ("\"a\" < \"b\"", type_bool()),
        ("\"a\" <= \"b\"", type_bool()),
        ("\"a\" > \"b\"", type_bool()),
        ("\"a\" >= \"b\"", type_bool()),
    ]);
}

#[test]
fn test_explicit_string_type() {
    check_vars_type(
        "
let x string = \"hello\"
let y string = 'world'
",
        vec![("x", type_string()), ("y", type_string())],
    );
}

#[test]
fn test_string_assignment_operators() {
    check_vars_type(
        "
var x = \"hello\"
x += \" world\"
",
        vec![("x", type_string())],
    );
}

#[test]
fn test_string_int_mismatch() {
    check_error(
        "
let x = \"hello\" + 1
",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_string_bool_mismatch() {
    check_error(
        "
let x = \"hello\" + true
",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_invalid_string_assignment() {
    check_error(
        "
var x = \"hello\"
x = 1
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_formatted_string() {
    check_exprs_type(vec![
        ("f\"hello {1}\"", type_string()),
        ("f\"val: {true}\"", type_string()),
        ("f\"{1} + {2} = {3}\"", type_string()),
    ]);
}

#[test]
fn test_formatted_string_with_variables() {
    check_vars_type(
        "
let name = \"World\"
let greeting = f\"Hello {name}\"
",
        vec![("greeting", type_string())],
    );
}

#[test]
fn test_formatted_string_nested_expressions() {
    check_exprs_type(vec![("f\"result: {1 + 2}\"", type_string())]);
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
    check_expr_type("f\"Value: {1 + 2 * 3}\"", type_string());
    check_expr_type("f\"Bool: {true and false}\"", type_string());
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
    check_expr_type("f\"\"", type_string());
}

#[test]
fn test_formatted_string_only_expression() {
    check_expr_type("f\"{1}\"", type_string());
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
        type_string(),
    );
}

#[test]
fn test_string_indexing() {
    check_expr_type("\"hello\"[0]", type_string());
    check_expr_type(
        "
let s = \"hello\"
s[1]
",
        type_string(),
    );
}

#[test]
fn test_string_indexing_invalid_index() {
    check_error("\"hello\"[\"a\"]", "String index must be an integer");
}

#[test]
fn test_string_membership() {
    check_expr_type("\"a\" in \"abc\"", type_bool());
}

#[test]
fn test_string_multiplication() {
    check_expr_type("\"a\" * 3", type_string());
    check_expr_type("3 * \"a\"", type_string());
}

#[test]
fn test_string_slicing() {
    check_expr_type("\"hello\"[0..1]", type_string());
    check_expr_type("\"hello\"[0..=1]", type_string());
}

#[test]
fn test_string_property_access() {
    check_expr_type("\"hello\".length", type_int());
}

#[test]
fn test_string_property_access_error() {
    check_error("\"hello\".invalid", "Type 'String' has no field 'invalid'");
}
