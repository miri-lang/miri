// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_string_literals() {
    type_checker_exprs_type_test(vec![
        ("\"hello\"", type_string()),
        ("\"\"", type_string()),
        ("'hello'", type_string()),
    ]);
}

#[test]
fn test_string_concatenation() {
    // The prelude loads system.string (which loads system.ops with Addable),
    // so string concatenation via `+` works without an explicit import.
    type_checker_expr_type_test("\"hello\" + \" world\"", type_string());
}

#[test]
fn test_string_comparisons() {
    type_checker_exprs_type_test(vec![
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
    type_checker_vars_type_test(
        "
let x String = \"hello\"
let y String = 'world'
",
        vec![("x", type_string()), ("y", type_string())],
    );
}

#[test]
fn test_string_assignment_operators() {
    type_checker_vars_type_test(
        "
var x = \"hello\"
x += \" world\"
",
        vec![("x", type_string())],
    );
}

#[test]
fn test_string_int_mismatch() {
    // String + int is invalid — both sides must be String for Addable (concat).
    type_checker_error_test(
        "
let x = \"hello\" + 1
",
        "Type mismatch: cannot add String and int",
    );
}

#[test]
fn test_string_bool_mismatch() {
    // String + bool is invalid — both sides must be String for Addable (concat).
    type_checker_error_test(
        "
let x = \"hello\" + true
",
        "Type mismatch: cannot add String and bool",
    );
}

#[test]
fn test_invalid_string_assignment() {
    type_checker_error_test(
        "
var x = \"hello\"
x = 1
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_formatted_string() {
    type_checker_exprs_type_test(vec![
        ("f\"hello {1}\"", type_string()),
        ("f\"val: {true}\"", type_string()),
        ("f\"{1} + {2} = {3}\"", type_string()),
    ]);
}

#[test]
fn test_formatted_string_with_variables() {
    type_checker_vars_type_test(
        "
let name = \"World\"
let greeting = f\"Hello {name}\"
",
        vec![("greeting", type_string())],
    );
}

#[test]
fn test_formatted_string_nested_expressions() {
    type_checker_exprs_type_test(vec![("f\"result: {1 + 2}\"", type_string())]);
}

#[test]
fn test_formatted_string_undefined_variable() {
    type_checker_error_test(
        "f\"Hello {undefined_var}\"",
        "Undefined variable: undefined_var",
    );
}

#[test]
fn test_formatted_string_type_error_in_expression() {
    type_checker_error_test(
        "f\"result: {1 + 'x'}\"",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_formatted_string_function_call() {
    type_checker_test(
        "
fn get_name() String: \"World\"
let s = f\"Hello {get_name()}\"
",
    );
}

#[test]
fn test_formatted_string_complex_expression() {
    type_checker_expr_type_test("f\"Value: {1 + 2 * 3}\"", type_string());
    type_checker_expr_type_test("f\"Bool: {true and false}\"", type_string());
}

#[test]
fn test_formatted_string_member_access() {
    type_checker_test(
        "
struct Point: x int, y int
let p = Point(1, 2)
let s = f\"Point: {p.x}, {p.y}\"
",
    );
}

#[test]
fn test_formatted_string_empty() {
    type_checker_expr_type_test("f\"\"", type_string());
}

#[test]
fn test_formatted_string_only_expression() {
    type_checker_expr_type_test("f\"{1}\"", type_string());
}

#[test]
fn test_formatted_string_void_expression() {
    // Void expressions are currently allowed in formatted strings
    type_checker_test(
        "
fn void_func()
    return
let s = f\"Void: {void_func()}\"
",
    );
}

#[test]
fn test_formatted_string_multiline() {
    type_checker_expr_type_test(
        "f\"
    Line 1
    Line 2 {1}
    \"",
        type_string(),
    );
}

#[test]
fn test_string_indexing() {
    type_checker_expr_type_test("\"hello\"[0]", type_string());
    type_checker_expr_type_test(
        "
let s = \"hello\"
s[1]
",
        type_string(),
    );
}

#[test]
fn test_string_indexing_invalid_index() {
    type_checker_error_test("\"hello\"[\"a\"]", "String index must be an integer");
}

#[test]
fn test_string_membership() {
    type_checker_expr_type_test("\"a\" in \"abc\"", type_bool());
}

#[test]
fn test_string_repetition() {
    // The prelude loads system.string (which loads Multiplicable from system.ops),
    // so string repetition via `*` works without an explicit import.
    type_checker_expr_type_test("\"a\" * 3", type_string());
}

#[test]
fn test_string_slicing() {
    type_checker_expr_type_test("\"hello\"[0..1]", type_string());
    type_checker_expr_type_test("\"hello\"[0..=1]", type_string());
}

#[test]
fn test_string_length_method_type() {
    // `s.length()` returns int via normal class method dispatch (prelude loads String).
    type_checker_expr_type_test("\"hello\".length()", type_int());
}

#[test]
fn test_string_property_access_error() {
    // After Phase 2 the class method dispatch reports "no field or method".
    type_checker_error_test(
        "\"hello\".invalid",
        "Type 'String' has no field or method 'invalid'",
    );
}
