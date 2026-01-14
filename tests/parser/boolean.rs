// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_test, run_literal_tests};
use miri::ast::factory::{
    boolean, boolean_literal, call, expression_statement, identifier, let_variable, logical,
    member, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility};

#[test]
fn test_parse_boolean_literal() {
    run_literal_tests(vec![("true", boolean(true)), ("false", boolean(false))]);
}

#[test]
fn test_boolean_in_variable_declaration() {
    parser_test(
        "let x = true",
        vec![variable_statement(
            vec![let_variable("x", None, opt_expr(boolean_literal(true)))],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_boolean_in_binary_expression() {
    parser_test(
        "x and true",
        vec![expression_statement(logical(
            identifier("x"),
            BinaryOp::And,
            boolean_literal(true),
        ))],
    );
}

#[test]
fn test_boolean_in_function_call() {
    parser_test(
        "my_func(true, false)",
        vec![expression_statement(call(
            identifier("my_func"),
            vec![boolean_literal(true), boolean_literal(false)],
        ))],
    );
}

#[test]
fn test_boolean_as_method_call_target() {
    // Booleans, like other literals, can be the target of a method call.
    parser_test(
        "false.to_string()",
        vec![expression_statement(call(
            member(boolean_literal(false), identifier("to_string")),
            vec![],
        ))],
    );
}

#[test]
fn test_case_sensitive_booleans() {
    // "True" and "False" are not boolean literals; they should be parsed as identifiers.
    parser_test(
        "let is_ok = True",
        vec![variable_statement(
            vec![let_variable("is_ok", None, opt_expr(identifier("True")))],
            MemberVisibility::Public,
        )],
    );
}
