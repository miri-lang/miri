// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_option_assignment() {
    type_checker_vars_type_test("var x int? = 5", vec![("x", type_option(type_int()))]);
}

#[test]
fn test_none_assignment_to_option() {
    type_checker_vars_type_test("var x int? = None", vec![("x", type_option(type_int()))]);
}

#[test]
fn test_none_assignment_to_non_option_error() {
    type_checker_error_test("var x int = None", "Type mismatch");
}

#[test]
fn test_option_immutable_warning() {
    type_checker_warning_test(
        "let x int? = 5",
        "Unnecessary optional declaration for variable 'x'",
    );
}

#[test]
fn test_option_list_of_non_option() {
    // [int]? - List itself can be None, but elements must be int
    type_checker_vars_type_test(
        "var list [int]? = [1, 2, 3]",
        vec![("list", type_option(type_list(type_int())))],
    );

    type_checker_vars_type_test(
        "var list [int]? = None",
        vec![("list", type_option(type_list(type_int())))],
    );

    type_checker_error_test(
        "var list [int]? = [1, None]",
        "List elements must have the same type",
    );
}

#[test]
fn test_non_option_list_of_option() {
    // [int?] - List cannot be None, but elements can be None
    type_checker_test(
        "
var list [int?] = [1, 2, 3]
list[1] = None
        ",
    );
    type_checker_error_test("var list [int?] = None", "Type mismatch");
}

#[test]
fn test_option_list_of_option() {
    // [int?]? - List can be None, and elements can be None
    type_checker_vars_type_test(
        "
var inner [int?] = [1, 2, 3]
inner[1] = None
var list [int?]? = inner
list = None
        ",
        vec![("list", type_option(type_list(type_option(type_int()))))],
    );
}

#[test]
fn test_option_map_values() {
    // {string: int?}
    type_checker_test(
        "
var map {String: int?} = {\"a\": 1}
map[\"b\"] = None
        ",
    );

    type_checker_error_test("var map {String: int} = {\"a\": None}", "Type mismatch");
}

#[test]
fn test_option_map_itself() {
    type_checker_vars_type_test(
        "
var map {String: int}? = {\"a\": 1}
map = None
    ",
        vec![("map", type_option(type_map(type_string(), type_int())))],
    );
}

#[test]
fn test_option_map_key() {
    type_checker_error_test(
        "var map {String?: int} = {None: 1}",
        "Map keys cannot be optional",
    );
}

#[test]
fn test_arithmetic_on_option_error() {
    type_checker_error_test(
        "
var x int? = 5
var y = x + 1
        ",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_member_access_on_option_error() {
    type_checker_error_test(
        "
struct Point: x int
var p Point? = Point(1)
var x = p.x
        ",
        "does not have members",
    );
}

#[test]
fn test_function_argument_option() {
    type_checker_test(
        "
fn foo(x int?)
    return

foo(5)
foo(None)
        ",
    );
}

#[test]
fn test_function_argument_non_option_error() {
    type_checker_error_test(
        "
fn foo(x int)
    return

var val int? = 5
foo(val)
        ",
        "Type mismatch",
    );
}

#[test]
fn test_function_return_option() {
    type_checker_test(
        "
fn foo() int?
    return None
        ",
    );
}

#[test]
fn test_function_return_non_option_error() {
    type_checker_error_test(
        "
fn foo() int
    return None
        ",
        "Invalid return type",
    );
}

#[test]
fn test_option_truthiness_if_error() {
    // Option values cannot be used directly as conditions
    type_checker_error_test(
        "
var x bool? = true
if x
    return
        ",
        "If condition must be a boolean",
    );
}

#[test]
fn test_option_truthiness_if_non_bool_error() {
    type_checker_error_test(
        "
var x int? = 5
if x: return
        ",
        "If condition must be a boolean",
    );
}

#[test]
fn test_option_truthiness_while_error() {
    type_checker_error_test(
        "
var x int? = 5
while x: break
        ",
        "While condition must be a boolean",
    );
}

#[test]
fn test_option_boolean_assignment() {
    type_checker_test("var x bool? = true");
    type_checker_test("var x bool? = false");
    type_checker_test("var x bool? = None");
}

// ==================== New tests for Option<T> / Some / postfix ? ====================

#[test]
fn test_option_explicit_type_syntax() {
    // Option<int> is equivalent to int?
    type_checker_vars_type_test(
        "var x Option<int> = 5",
        vec![("x", type_option(type_int()))],
    );
}

#[test]
fn test_option_explicit_type_with_none() {
    type_checker_vars_type_test(
        "var x Option<int> = None",
        vec![("x", type_option(type_int()))],
    );
}

#[test]
fn test_some_constructor() {
    type_checker_vars_type_test("let x = Some(5)", vec![("x", type_option(type_int()))]);
}

#[test]
fn test_some_constructor_string() {
    type_checker_vars_type_test(
        "let x = Some(\"hello\")",
        vec![("x", type_option(type_string()))],
    );
}

#[test]
fn test_some_constructor_bool() {
    type_checker_vars_type_test("let x = Some(true)", vec![("x", type_option(type_bool()))]);
}

// ==================== Tests for ?? operator ====================

#[test]
fn test_null_coalesce_basic() {
    type_checker_vars_type_test(
        "
var x int? = 5
let y = x ?? 0
        ",
        vec![("y", type_int())],
    );
}

#[test]
fn test_null_coalesce_string() {
    type_checker_vars_type_test(
        r#"
var s String? = None
let r = s ?? "default"
        "#,
        vec![("r", type_string())],
    );
}

#[test]
fn test_null_coalesce_type_mismatch_error() {
    type_checker_error_test(
        r#"
var x int? = 5
let y = x ?? "str"
        "#,
        "Type mismatch in '??'",
    );
}

#[test]
fn test_null_coalesce_non_option_error() {
    type_checker_error_test(
        "
let x int = 5
let y = x ?? 0
        ",
        "must be an Option type",
    );
}

#[test]
fn test_null_coalesce_nested() {
    // int?? ?? Some(0) → int?
    type_checker_vars_type_test(
        "
var x Option<int?> = None
let y = x ?? Some(0)
        ",
        vec![("y", type_option(type_int()))],
    );
}

#[test]
fn test_null_coalesce_chained() {
    // int?? ?? int? → int?, then int? ?? int → int
    type_checker_vars_type_test(
        "
var x Option<int?> = None
var z int? = Some(1)
let y = x ?? z ?? 0
        ",
        vec![("y", type_int())],
    );
}

#[test]
fn test_nested_option() {
    type_checker_vars_type_test(
        "var x Option<Option<int>> = None",
        vec![("x", type_option(type_option(type_int())))],
    );
}

#[test]
fn test_nested_option_with_question_mark() {
    // Option<int?> is equivalent to Option<Option<int>>
    type_checker_vars_type_test(
        "var x Option<int?> = None",
        vec![("x", type_option(type_option(type_int())))],
    );
}

#[test]
fn test_if_let_some() {
    type_checker_test(
        "
var x int? = 5
if let Some(v) = x
    let y int = v
        ",
    );
}

#[test]
fn test_if_let_some_else() {
    type_checker_test(
        "
var x int? = 5
if let Some(v) = x
    let y int = v
else
    let z = 0
        ",
    );
}

#[test]
fn test_while_let_some() {
    type_checker_test(
        "
var x int? = 5
while let Some(v) = x
    x = None
        ",
    );
}
