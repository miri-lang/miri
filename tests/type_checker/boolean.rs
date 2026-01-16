// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_boolean_literals() {
    type_checker_exprs_type_test(vec![("true", type_bool()), ("false", type_bool())]);
}

#[test]
fn test_boolean_expressions() {
    type_checker_exprs_type_test(vec![
        ("true and false", type_bool()),
        ("true or false", type_bool()),
        ("not true", type_bool()),
        ("true and (false or true)", type_bool()),
    ]);
}

#[test]
fn test_boolean_logic() {
    type_checker_vars_type_test(
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
    type_checker_exprs_type_test(vec![
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
    type_checker_vars_type_test(
        "
let x = 1 > 2
let y = 1.5 <= 2.5
",
        vec![("x", type_bool()), ("y", type_bool())],
    );
}

#[test]
fn test_explicit_type() {
    type_checker_vars_type_test(
        "
let x bool = true
let y bool = false
",
        vec![("x", type_bool()), ("y", type_bool())],
    );
}

#[test]
fn test_invalid_boolean_logic_and() {
    type_checker_error_test(
        "
let x = true and 1
",
        "Logical operations require booleans",
    );
}

#[test]
fn test_invalid_boolean_logic_or() {
    type_checker_error_test(
        "
let x = 1 or false
",
        "Logical operations require booleans",
    );
}

#[test]
fn test_invalid_boolean_logic_not() {
    type_checker_error_test(
        "
let x = not 1
",
        "Logical NOT requires boolean",
    );
}

#[test]
fn test_invalid_equality_types() {
    type_checker_error_test(
        "
let x = 1 == true
",
        "Type mismatch",
    );
}

#[test]
fn test_boolean_comparison() {
    // Boolean comparison is valid (e.g. true > false)
    type_checker_vars_type_test(
        "
let x = true > false
",
        vec![("x", type_bool())],
    );
}

#[test]
fn test_if_condition_type_mismatch() {
    type_checker_error_test(
        "
if 1
    let x = 1
",
        "If condition must be a boolean",
    );
}

#[test]
fn test_while_condition_type_mismatch() {
    type_checker_error_test(
        "
while 1
    let x = 1
",
        "While condition must be a boolean",
    );
}

#[test]
fn test_conditional_expression_type_mismatch() {
    type_checker_error_test(
        "
let x = 10 if 1 else 20
",
        "Conditional condition must be a boolean",
    );
}

#[test]
fn test_bitwise_operations_invalid() {
    type_checker_error_test(
        "let x = true & false",
        "Invalid types for bitwise operation",
    );
    type_checker_error_test(
        "let x = true | false",
        "Invalid types for bitwise operation",
    );
    type_checker_error_test(
        "let x = true ^ false",
        "Invalid types for bitwise operation",
    );
}

#[test]
fn test_unary_operations_invalid() {
    type_checker_error_test("let x = -true", "Unary operator requires numeric type");
    type_checker_error_test("let x = +true", "Unary operator requires numeric type");
}

#[test]
fn test_assignment_mismatch() {
    type_checker_error_test("let x bool = 1", "Type mismatch for variable 'x'");
    type_checker_error_test(
        "
let x = true
x = 1
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_function_mismatch() {
    type_checker_error_test(
        "
fn f(b bool) bool
    return 1
",
        "Invalid return type",
    );
    type_checker_error_test(
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
    type_checker_exprs_type_test(vec![
        ("true < false", type_bool()),
        ("true <= false", type_bool()),
        ("true > false", type_bool()),
        ("true >= false", type_bool()),
    ]);
}

#[test]
fn test_match_boolean() {
    type_checker_expr_type_test(
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
    type_checker_error_test(
        "
for i in true
    1
",
        "Type boolean is not iterable",
    );
}

#[test]
fn test_boolean_map_key() {
    type_checker_expr_type_test(
        "
{true: 1, false: 0}
",
        type_map(type_bool(), type_int()),
    );
}

#[test]
fn test_boolean_map_key_expression() {
    type_checker_expr_type_test(
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
    type_checker_expr_type_test("[true, false, true]", type_list(type_bool()));
}

#[test]
fn test_boolean_list_expression() {
    type_checker_expr_type_test("[1 > 0, 1 == 1, true or false]", type_list(type_bool()));
}

#[test]
fn test_boolean_tuple() {
    type_checker_expr_type_test("(true, false)", type_tuple(vec![type_bool(), type_bool()]));
}

#[test]
fn test_boolean_tuple_mixed() {
    type_checker_expr_type_test(
        "(true, 1, \"s\")",
        type_tuple(vec![type_bool(), type_int(), type_string()]),
    );
}

#[test]
fn test_boolean_set() {
    type_checker_expr_type_test("{true, false}", type_set(type_bool()));
}

#[test]
fn test_boolean_set_expression() {
    type_checker_expr_type_test("{1 > 0, 1 == 1}", type_set(type_bool()));
}

#[test]
fn test_nullable_boolean_assignment() {
    type_checker_expr_type_test(
        "
let x bool? = true
x
",
        type_null(type_bool()),
    );
    type_checker_expr_type_test(
        "
let y bool? = false
y
",
        type_null(type_bool()),
    );
    type_checker_expr_type_test(
        "
let z bool? = None
z
",
        type_null(type_bool()),
    );
}

#[test]
fn test_nullable_boolean_mismatch() {
    type_checker_error_test(
        "
let x bool? = true
let y bool = x
",
        "Type mismatch",
    );
}

#[test]
fn test_nullable_boolean_logic_error() {
    type_checker_error_test(
        "
let x bool? = true
let y = x and true
",
        "Logical operations require booleans",
    );
}

#[test]
fn test_boolean_deeply_nested_logic() {
    type_checker_exprs_type_test(vec![
        ("not not not not true", type_bool()),
        ("((true and false) or (true and true))", type_bool()),
        ("(((true or false) and (not true)) or false)", type_bool()),
    ]);
}

#[test]
fn test_boolean_long_chain_and() {
    type_checker_expr_type_test(
        "true and true and true and true and true and true and true and true and true and true",
        type_bool(),
    );
}

#[test]
fn test_boolean_long_chain_or() {
    type_checker_expr_type_test(
        "false or false or false or false or false or false or false or false or true",
        type_bool(),
    );
}

#[test]
fn test_boolean_mixed_logic_chain() {
    type_checker_exprs_type_test(vec![
        ("true and false or true and false or true", type_bool()),
        ("not true or not false and not true", type_bool()),
    ]);
}

#[test]
fn test_boolean_complex_comparisons() {
    type_checker_exprs_type_test(vec![
        ("(1 < 2) and (3 > 4) or (5 == 5)", type_bool()),
        ("(1 <= 2) == (3 >= 4)", type_bool()),
        ("not (1 != 2)", type_bool()),
    ]);
}

#[test]
fn test_boolean_compact_formatting() {
    type_checker_exprs_type_test(vec![
        ("true and false", type_bool()),
        ("not true", type_bool()),
    ]);
}

#[test]
fn test_boolean_in_nested_conditionals() {
    type_checker_test(
        "
if true and false
    if true or false
        if not true
            1
",
    );
}

#[test]
fn test_boolean_in_while_nested() {
    type_checker_test(
        "
while true and (1 < 2)
    while false or (3 > 4)
        break
    break
",
    );
}

#[test]
fn test_boolean_many_variables() {
    type_checker_test(
        "
let a = true
let b = a and false
let c = b or true
let d = not c
let e = (a and b) or (c and d)
let f = not (e and a) or (b and c)
",
    );
}

#[test]
fn test_boolean_equality_chain() {
    type_checker_exprs_type_test(vec![
        ("true == true", type_bool()),
        ("false != true", type_bool()),
        ("(true == false) == (false == true)", type_bool()),
    ]);
}

#[test]
fn test_boolean_invalid_operand_types() {
    type_checker_error_test("true and 1", "Logical operations require booleans");
    type_checker_error_test("\"string\" or false", "Logical operations require booleans");
    type_checker_error_test("not 42", "Logical NOT requires boolean");
}
