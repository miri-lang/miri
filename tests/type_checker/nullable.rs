// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_nullable_assignment() {
    check_vars_type("var x int? = 5", vec![("x", type_null(type_int()))]);
}

#[test]
fn test_none_assignment_to_nullable() {
    check_vars_type("var x int? = None", vec![("x", type_null(type_int()))]);
}

#[test]
fn test_none_assignment_to_non_nullable_error() {
    check_error("var x int = None", "Type mismatch");
}

#[test]
fn test_nullable_immutable_warning() {
    check_warning(
        "let x int? = 5",
        "Variable 'x' is immutable but declared as nullable",
    );
}

#[test]
fn test_nullable_list_of_non_nullable() {
    // [int]? - List itself can be None, but elements must be int
    check_vars_type(
        "var list [int]? = [1, 2, 3]",
        vec![("list", type_null(type_list(type_int())))],
    );

    check_vars_type(
        "var list [int]? = None",
        vec![("list", type_null(type_list(type_int())))],
    );

    check_error("var list [int]? = [1, None]", "Type mismatch");
}

#[test]
fn test_non_nullable_list_of_nullable() {
    // [int?] - List cannot be None, but elements can be None
    // Note: List literal inference is strict, so we init with ints and assign None
    check_success(
        "
var list [int?] = [1, 2, 3]
list[1] = None
        ",
    );
    check_error("var list [int?] = None", "Type mismatch");
}

#[test]
fn test_nullable_list_of_nullable() {
    // [int?]? - List can be None, and elements can be None
    check_vars_type(
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
    check_success(
        "
var map {string: int?} = {\"a\": 1}
map[\"b\"] = None
        ",
    );

    check_error("var map {string: int} = {\"a\": None}", "Type mismatch");
}

#[test]
fn test_nullable_map_itself() {
    check_vars_type(
        "
var map {string: int}? = {\"a\": 1}
map = None
    ",
        vec![("map", type_null(type_map(type_string(), type_int())))],
    );
}

#[test]
fn test_nullable_map_key() {
    check_error(
        "var map {string?: int} = {None: 1}",
        "Map keys cannot be nullable",
    );
}

#[test]
fn test_arithmetic_on_nullable_error() {
    // TODO: currently unwrapping nullable in arithmetic is not supported
    check_error(
        "
var x int? = 5
var y = x + 1
        ",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_member_access_on_nullable_error() {
    check_error(
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
    check_success(
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
    check_error(
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
    check_success(
        "
fn foo() int?
    return None
        ",
    );
}

#[test]
fn test_function_return_non_nullable_error() {
    check_error(
        "
fn foo() int
    return None
        ",
        "Invalid return type",
    );
}

#[test]
fn test_nullable_boolean_logic_error() {
    check_error(
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
    check_success("var x bool? = true");
    check_success("var x bool? = false");
    check_success("var x bool? = None");
}

#[test]
fn test_option_methods() {
    let source = "
let o int? = 10
o.is_some()
    ";
    check_expr_type(source, type_bool());
}
