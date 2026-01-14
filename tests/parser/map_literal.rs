// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    binary, block, call, expression_statement, for_statement, identifier, index,
    int_literal_expression, iter_obj, lambda, let_variable, map, member, string_literal_expression,
    symbol_literal, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_map_literal_assignment() {
    parser_test(
        "let m = {'a': 1, 'b': 2}",
        vec![variable_statement(
            vec![let_variable(
                "m",
                None,
                opt_expr(map(vec![
                    (string_literal_expression("a"), int_literal_expression(1)),
                    (string_literal_expression("b"), int_literal_expression(2)),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_map_index_access() {
    // This relies on the existing index expression parsing
    parser_test(
        "print(my_map['key'])",
        vec![expression_statement(call(
            identifier("print"),
            vec![index(
                identifier("my_map"),
                string_literal_expression("key"),
            )],
        ))],
    );
}

#[test]
fn test_for_loop_over_map_literal() {
    parser_test(
        "
for k, v in {'a': 1, 'b': 2}
    print(k, v)
",
        vec![for_statement(
            vec![let_variable("k", None, None), let_variable("v", None, None)],
            iter_obj(map(vec![
                (string_literal_expression("a"), int_literal_expression(1)),
                (string_literal_expression("b"), int_literal_expression(2)),
            ])),
            block(vec![expression_statement(call(
                identifier("print"),
                vec![identifier("k"), identifier("v")],
            ))]),
        )],
    );
}

#[test]
fn test_method_call_on_map_literal() {
    parser_test(
        "{'a': 1}.keys()",
        vec![expression_statement(call(
            member(
                map(vec![(
                    string_literal_expression("a"),
                    int_literal_expression(1),
                )]),
                identifier("keys"),
            ),
            vec![],
        ))],
    );
}

#[test]
fn test_map_of_lambdas() {
    parser_test(
        "let funcs = {'a': fn(): 1, 'b': fn(): 2}",
        vec![variable_statement(
            vec![let_variable(
                "funcs",
                None,
                opt_expr(map(vec![
                    (
                        string_literal_expression("a"),
                        lambda().build_lambda(expression_statement(int_literal_expression(1))),
                    ),
                    (
                        string_literal_expression("b"),
                        lambda().build_lambda(expression_statement(int_literal_expression(2))),
                    ),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_empty_map() {
    parser_test(
        "let empty = {}",
        vec![variable_statement(
            vec![let_variable("empty", None, opt_expr(map(vec![])))],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_map_with_trailing_comma() {
    parser_test(
        "let m = {'a': 1,}",
        vec![variable_statement(
            vec![let_variable(
                "m",
                None,
                opt_expr(map(vec![(
                    string_literal_expression("a"),
                    int_literal_expression(1),
                )])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_map_multiline() {
    parser_test(
        "
let m = {
    'a'
        :
            1,
    'b'
        :
 2
}",
        vec![variable_statement(
            vec![let_variable(
                "m",
                None,
                opt_expr(map(vec![
                    (string_literal_expression("a"), int_literal_expression(1)),
                    (string_literal_expression("b"), int_literal_expression(2)),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_nested_maps_multiline() {
    parser_test(
        "
let config = {
    'user': {
        'name': 'John',
        'id': 123,
    },
    'settings': {}
}
",
        vec![variable_statement(
            vec![let_variable(
                "config",
                None,
                opt_expr(map(vec![
                    (
                        string_literal_expression("user"),
                        map(vec![
                            (
                                string_literal_expression("name"),
                                string_literal_expression("John"),
                            ),
                            (string_literal_expression("id"), int_literal_expression(123)),
                        ]),
                    ),
                    (string_literal_expression("settings"), map(vec![])),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_unclosed_map() {
    parser_error_test("let m = {'a': 1", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_map_missing_colon() {
    // This is invalid syntax. It's not a valid map (due to :) and not a valid set (due to missing value).
    parser_error_test(
        "let m = {'a' 1}",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "}".to_string(),
            found: "int".to_string(),
        },
    );
}

#[test]
fn test_error_map_missing_comma() {
    parser_error_test(
        "let m = {'a': 1 'b': 2}",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "}".to_string(),
            found: "string".to_string(),
        },
    );
}

#[test]
fn test_map_with_complex_keys() {
    // Map keys can be complex expressions, not just literals.
    // NOTE: semantically this code isn't correct, because keys must have the same type.
    parser_test(
        "let m = {1 + 1: 'a', my_func(): 'b'}",
        vec![variable_statement(
            vec![let_variable(
                "m",
                None,
                opt_expr(map(vec![
                    (
                        binary(
                            int_literal_expression(1),
                            BinaryOp::Add,
                            int_literal_expression(1),
                        ),
                        string_literal_expression("a"),
                    ),
                    (
                        call(identifier("my_func"), vec![]),
                        string_literal_expression("b"),
                    ),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_map_with_symbol_keys() {
    // A common pattern is to use symbols as keys.
    parser_test(
        "let m = {:a: 1, :b: 2}",
        vec![variable_statement(
            vec![let_variable(
                "m",
                None,
                opt_expr(map(vec![
                    (symbol_literal("a"), int_literal_expression(1)),
                    (symbol_literal("b"), int_literal_expression(2)),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_map_index_precedence() {
    // Index access on a literal has higher precedence than binary operators.
    // This should parse as `({'a': 10}['a']) + 1`.
    parser_test(
        "{'a': 10}['a'] + 1",
        vec![expression_statement(binary(
            index(
                map(vec![(
                    string_literal_expression("a"),
                    int_literal_expression(10),
                )]),
                string_literal_expression("a"),
            ),
            BinaryOp::Add,
            int_literal_expression(1),
        ))],
    );
}
