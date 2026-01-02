// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_boolean_literals() {
    check_exprs_type(vec![("true", type_bool()), ("false", type_bool())]);
}

#[test]
fn test_boolean_expressions() {
    check_exprs_type(vec![
        ("true and false", type_bool()),
        ("true or false", type_bool()),
        ("not true", type_bool()),
        ("true and (false or true)", type_bool()),
    ]);
}

#[test]
fn test_boolean_logic() {
    check_vars_type(
        "
let x = true and false
let y = not x
let z = x or y
",
        vec![("x", type_bool()), ("y", type_bool()), ("z", type_bool())],
    );
}

#[test]
fn test_equality() {
    check_exprs_type(vec![
        ("true == false", type_bool()),
        ("true != false", type_bool()),
        ("1 == 1", type_bool()),
        ("1 != 2", type_bool()),
        ("1.5 == 1.5", type_bool()),
        // ("\"a\" == \"b\"", type_bool()), // TODO: Enable when string equality is supported
    ]);
}

#[test]
fn test_comparison() {
    check_vars_type(
        "
let x = 1 > 2
let y = 1.5 <= 2.5
",
        vec![("x", type_bool()), ("y", type_bool())],
    );
}

#[test]
fn test_explicit_type() {
    check_vars_type(
        "
let x bool = true
let y bool = false
",
        vec![("x", type_bool()), ("y", type_bool())],
    );
}

#[test]
fn test_invalid_boolean_logic_and() {
    check_error(
        "
let x = true and 1
",
        "Logical operations require booleans",
    );
}

#[test]
fn test_invalid_boolean_logic_or() {
    check_error(
        "
let x = 1 or false
",
        "Logical operations require booleans",
    );
}

#[test]
fn test_invalid_boolean_logic_not() {
    check_error(
        "
let x = not 1
",
        "Logical NOT requires boolean",
    );
}

#[test]
fn test_invalid_equality_types() {
    check_error(
        "
let x = 1 == true
",
        "Type mismatch",
    );
}

#[test]
fn test_boolean_comparison() {
    // Boolean comparison is valid (e.g. true > false)
    check_vars_type(
        "
let x = true > false
",
        vec![("x", type_bool())],
    );
}

#[test]
fn test_if_condition_type_mismatch() {
    check_error(
        "
if 1
    let x = 1
",
        "If condition must be a boolean",
    );
}

#[test]
fn test_while_condition_type_mismatch() {
    check_error(
        "
while 1
    let x = 1
",
        "While condition must be a boolean",
    );
}

#[test]
fn test_conditional_expression_type_mismatch() {
    check_error(
        "
let x = 10 if 1 else 20
",
        "Conditional condition must be a boolean",
    );
}

#[test]
fn test_bitwise_operations_invalid() {
    check_error(
        "let x = true & false",
        "Invalid types for bitwise operation",
    );
    check_error(
        "let x = true | false",
        "Invalid types for bitwise operation",
    );
    check_error(
        "let x = true ^ false",
        "Invalid types for bitwise operation",
    );
}

#[test]
fn test_unary_operations_invalid() {
    check_error("let x = -true", "Unary operator requires numeric type");
    check_error("let x = +true", "Unary operator requires numeric type");
}

#[test]
fn test_assignment_mismatch() {
    check_error("let x bool = 1", "Type mismatch for variable 'x'");
    check_error(
        "
let x = true
x = 1
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_function_mismatch() {
    check_error(
        "
fn f(b bool) bool
    return 1
",
        "Invalid return type",
    );
    check_error(
        "
fn f(b bool)
    return

f(1)
",
        "Type mismatch for argument 'b'",
    );
}

#[test]
fn test_boolean_comparison_comprehensive() {
    check_exprs_type(vec![
        ("true < false", type_bool()),
        ("true <= false", type_bool()),
        ("true > false", type_bool()),
        ("true >= false", type_bool()),
    ]);
}

#[test]
fn test_match_boolean() {
    check_expr_type(
        "
match true
    true: 1
    false: 0
",
        type_int(),
    );
}

#[test]
fn test_invalid_iterable() {
    check_error(
        "
for i in true
    1
",
        "Type Boolean is not iterable",
    );
}

#[test]
fn test_boolean_map_key() {
    check_expr_type(
        "
{true: 1, false: 0}
",
        type_map(type_bool(), type_int()),
    );
}

#[test]
fn test_boolean_map_key_expression() {
    check_expr_type(
        "
fn predicate(x int) bool: x > 0

{
    predicate(10) or predicate(20) and 100 % 10 == 0: 'crazy predicate',
    predicate(25) or 1 - 1 == 0: 'another crazy predicate'
}
",
        type_map(type_bool(), type_string()),
    );
}

#[test]
fn test_boolean_list() {
    check_expr_type("[true, false, true]", type_list(type_bool()));
}

#[test]
fn test_boolean_list_expression() {
    check_expr_type("[1 > 0, 1 == 1, true or false]", type_list(type_bool()));
}

#[test]
fn test_boolean_tuple() {
    check_expr_type("(true, false)", type_tuple(vec![type_bool(), type_bool()]));
}

#[test]
fn test_boolean_tuple_mixed() {
    check_expr_type(
        "(true, 1, \"s\")",
        type_tuple(vec![type_bool(), type_int(), type_string()]),
    );
}

#[test]
fn test_boolean_set() {
    check_expr_type("{true, false}", type_set(type_bool()));
}

#[test]
fn test_boolean_set_expression() {
    check_expr_type("{1 > 0, 1 == 1}", type_set(type_bool()));
}

#[test]
fn test_nullable_boolean_assignment() {
    check_expr_type(
        "
let x bool? = true
x
",
        type_null(type_bool()),
    );
    check_expr_type(
        "
let y bool? = false
y
",
        type_null(type_bool()),
    );
    check_expr_type(
        "
let z bool? = None
z
",
        type_null(type_bool()),
    );
}

#[test]
fn test_nullable_boolean_mismatch() {
    check_error(
        "
let x bool? = true
let y bool = x
",
        "Type mismatch",
    );
}

#[test]
fn test_nullable_boolean_logic_error() {
    check_error(
        "
let x bool? = true
let y = x and true
",
        "Logical operations require booleans",
    );
}
