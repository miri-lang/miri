// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_set_literal_assignment() {
    parse_test("let s = {1, 2, 3}", vec![
        variable_statement(vec![
            let_variable("s", None, opt_expr(set(vec![
                int_literal_expression(1),
                int_literal_expression(2),
                int_literal_expression(3),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_for_loop_over_set_literal() {
    parse_test("
for el in {1, 2, 3}
    print(el)
", vec![
        for_statement(
            vec![let_variable("el", None, None)],
            iter_obj(set(vec![
                int_literal_expression(1), int_literal_expression(2), int_literal_expression(3)
            ])),
            block(vec![expression_statement(call(identifier("print"), vec![identifier("el")]))])
        )
    ]);
}

#[test]
fn test_method_call_on_set_literal() {
    parse_test("{1, 2, 3}.len()", vec![
        expression_statement(
            call(
                member(
                    set(vec![int_literal_expression(1), int_literal_expression(2), int_literal_expression(3)]),
                    identifier("len")
                ),
                vec![]
            )
        )
    ]);
}

#[test]
fn test_set_of_lambdas() {
    parse_test("let funcs = {fn(): 1, fn(): 2}", vec![
        variable_statement(vec![
            let_variable("funcs", None, opt_expr(set(vec![
                lambda().build_lambda(expression_statement(int_literal_expression(1))),
                lambda().build_lambda(expression_statement(int_literal_expression(2))),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_map_of_values_similar_to_set() {
    parse_test("let funcs = {func1(): 1, func2(): 2}", vec![
        variable_statement(vec![
            let_variable("funcs", None, opt_expr(map(vec![
                (call(identifier("func1"), vec![]), int_literal_expression(1)),
                (call(identifier("func2"), vec![]), int_literal_expression(2)),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_set_with_trailing_comma() {
    parse_test("let s = {1, 2,}", vec![
        variable_statement(vec![
            let_variable("s", None, opt_expr(set(vec![
                int_literal_expression(1),
                int_literal_expression(2),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_nested_sets_and_maps_multiline() {
    parse_test("
let data = {
    'users',
    {'id': 1, 'name': 'John'},
    {'id': 2, 'name': 'Jane'},
}
", vec![
        variable_statement(vec![
            let_variable("data", None, opt_expr(set(vec![
                string_literal("users"),
                map(vec![
                    (string_literal("id"), int_literal_expression(1)),
                    (string_literal("name"), string_literal("John")),
                ]),
                map(vec![
                    (string_literal("id"), int_literal_expression(2)),
                    (string_literal("name"), string_literal("Jane")),
                ]),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_distinction_empty_is_map() {
    // An empty `{}` should be parsed as an empty map.
    parse_test("let empty = {}", vec![
        variable_statement(vec![
            let_variable("empty", None, opt_expr(map(vec![])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_distinction_single_item_is_set() {
    parse_test("let s = {'a'}", vec![
        variable_statement(vec![
            let_variable("s", None, opt_expr(set(vec![string_literal("a")])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_error_unclosed_set() {
    parse_error_test(
        "let s = {1, 2",
        SyntaxErrorKind::UnexpectedEOF
    );
}

#[test]
fn test_error_set_with_colon() {
    // This is invalid syntax. It's not a valid set (due to :) and not a valid map (due to missing value).
    parse_error_test(
        "let s = {1:}",
        SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "}".to_string(),
        }
    );
}
