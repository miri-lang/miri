// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    block, boolean_literal, call, expression_statement, for_statement, identifier,
    int_literal_expression, iter_obj, lambda, let_variable, logical, map, member, set,
    string_literal_expression, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_set_literal_assignment() {
    parser_test(
        "let s = {1, 2, 3}",
        vec![variable_statement(
            vec![let_variable(
                "s",
                None,
                opt_expr(set(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_for_loop_over_set_literal() {
    parser_test(
        "
for el in {1, 2, 3}
    print(el)
",
        vec![for_statement(
            vec![let_variable("el", None, None)],
            iter_obj(set(vec![
                int_literal_expression(1),
                int_literal_expression(2),
                int_literal_expression(3),
            ])),
            block(vec![expression_statement(call(
                identifier("print"),
                vec![identifier("el")],
            ))]),
        )],
    );
}

#[test]
fn test_method_call_on_set_literal() {
    parser_test(
        "{1, 2, 3}.len()",
        vec![expression_statement(call(
            member(
                set(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                ]),
                identifier("len"),
            ),
            vec![],
        ))],
    );
}

#[test]
fn test_set_of_lambdas() {
    parser_test(
        "let funcs = {fn(): 1, fn(): 2}",
        vec![variable_statement(
            vec![let_variable(
                "funcs",
                None,
                opt_expr(set(vec![
                    lambda().build_lambda(expression_statement(int_literal_expression(1))),
                    lambda().build_lambda(expression_statement(int_literal_expression(2))),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_map_of_values_similar_to_set() {
    parser_test(
        "let funcs = {func1(): 1, func2(): 2}",
        vec![variable_statement(
            vec![let_variable(
                "funcs",
                None,
                opt_expr(map(vec![
                    (call(identifier("func1"), vec![]), int_literal_expression(1)),
                    (call(identifier("func2"), vec![]), int_literal_expression(2)),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_set_with_trailing_comma() {
    parser_test(
        "let s = {1, 2,}",
        vec![variable_statement(
            vec![let_variable(
                "s",
                None,
                opt_expr(set(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_multiline_set() {
    parser_test(
        "
let s = {1, 
2, 
    3, 
            4, 
5, 
    6, 
7}",
        vec![variable_statement(
            vec![let_variable(
                "s",
                None,
                opt_expr(set(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                    int_literal_expression(4),
                    int_literal_expression(5),
                    int_literal_expression(6),
                    int_literal_expression(7),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_nested_sets_and_maps_multiline() {
    parser_test(
        "
let data = {
    'users',
    {'id': 1, 'name': 'John'},
    {'id': 2, 'name': 'Jane'},
}
",
        vec![variable_statement(
            vec![let_variable(
                "data",
                None,
                opt_expr(set(vec![
                    string_literal_expression("users"),
                    map(vec![
                        (string_literal_expression("id"), int_literal_expression(1)),
                        (
                            string_literal_expression("name"),
                            string_literal_expression("John"),
                        ),
                    ]),
                    map(vec![
                        (string_literal_expression("id"), int_literal_expression(2)),
                        (
                            string_literal_expression("name"),
                            string_literal_expression("Jane"),
                        ),
                    ]),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_distinction_empty_is_map() {
    // An empty `{}` should be parsed as an empty map.
    parser_test(
        "let empty = {}",
        vec![variable_statement(
            vec![let_variable("empty", None, opt_expr(map(vec![])))],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_distinction_single_item_is_set() {
    parser_test(
        "let s = {'a'}",
        vec![variable_statement(
            vec![let_variable(
                "s",
                None,
                opt_expr(set(vec![string_literal_expression("a")])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_unclosed_set() {
    parser_error_test("let s = {1, 2", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_set_with_colon() {
    // This is invalid syntax. It's not a valid set (due to :) and not a valid map (due to missing value).
    parser_error_test(
        "let s = {1:}",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "}".to_string(),
        },
    );
}

#[test]
fn test_set_literal_precedence() {
    parser_test(
        "{1, 2}.contains(1) and true",
        vec![expression_statement(logical(
            call(
                member(
                    set(vec![int_literal_expression(1), int_literal_expression(2)]),
                    identifier("contains"),
                ),
                vec![int_literal_expression(1)],
            ),
            BinaryOp::And,
            boolean_literal(true),
        ))],
    );
}
