// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_nullable_assignment() {
    type_checker_vars_type_test("var x int? = 5", vec![("x", type_null(type_int()))]);
}

#[test]
fn test_none_assignment_to_nullable() {
    type_checker_vars_type_test("var x int? = None", vec![("x", type_null(type_int()))]);
}

#[test]
fn test_none_assignment_to_non_nullable_error() {
    type_checker_error_test("var x int = None", "Type mismatch");
}

#[test]
fn test_nullable_immutable_warning() {
    type_checker_warning_test(
        "let x int? = 5",
        "Unnecessary nullable declaration for variable 'x'",
    );
}

#[test]
fn test_nullable_list_of_non_nullable() {
    // [int]? - List itself can be None, but elements must be int
    type_checker_vars_type_test(
        "var list [int]? = [1, 2, 3]",
        vec![("list", type_null(type_list(type_int())))],
    );

    type_checker_vars_type_test(
        "var list [int]? = None",
        vec![("list", type_null(type_list(type_int())))],
    );

    type_checker_error_test("var list [int]? = [1, None]", "Type mismatch");
}

#[test]
fn test_non_nullable_list_of_nullable() {
    // [int?] - List cannot be None, but elements can be None
    // Note: List literal inference is strict, so we init with ints and assign None
    type_checker_test(
        "
var list [int?] = [1, 2, 3]
list[1] = None
        ",
    );
    type_checker_error_test("var list [int?] = None", "Type mismatch");
}

#[test]
fn test_nullable_list_of_nullable() {
    // [int?]? - List can be None, and elements can be None
    type_checker_vars_type_test(
        "
var inner [int?] = [1, 2, 3]
inner[1] = None
var list [int?]? = inner
list = None
        ",
        vec![("list", type_null(type_list(type_null(type_int()))))],
    );
}

#[test]
fn test_nullable_map_values() {
    // {string: int?}
    type_checker_test(
        "
var map {string: int?} = {\"a\": 1}
map[\"b\"] = None
        ",
    );

    type_checker_error_test("var map {string: int} = {\"a\": None}", "Type mismatch");
}

#[test]
fn test_nullable_map_itself() {
    type_checker_vars_type_test(
        "
var map {string: int}? = {\"a\": 1}
map = None
    ",
        vec![("map", type_null(type_map(type_string(), type_int())))],
    );
}

#[test]
fn test_nullable_map_key() {
    type_checker_error_test(
        "var map {string?: int} = {None: 1}",
        "Map keys cannot be nullable",
    );
}

#[test]
fn test_arithmetic_on_nullable_error() {
    // TODO: currently unwrapping nullable in arithmetic is not supported
    type_checker_error_test(
        "
var x int? = 5
var y = x + 1
        ",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_member_access_on_nullable_error() {
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
fn test_function_argument_nullable() {
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
fn test_function_argument_non_nullable_error() {
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
fn test_function_return_nullable() {
    type_checker_test(
        "
fn foo() int?
    return None
        ",
    );
}

#[test]
fn test_function_return_non_nullable_error() {
    type_checker_error_test(
        "
fn foo() int
    return None
        ",
        "Invalid return type",
    );
}

#[test]
fn test_nullable_boolean_logic_error() {
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
fn test_nullable_boolean_assignment() {
    type_checker_test("var x bool? = true");
    type_checker_test("var x bool? = false");
    type_checker_test("var x bool? = None");
}

#[test]
fn test_option_methods() {
    let source = "
let o int? = 10
o.is_some()
    ";
    type_checker_expr_type_test(source, type_bool());
}
