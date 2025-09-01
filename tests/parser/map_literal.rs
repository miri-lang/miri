// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_map_literal_assignment() {
    parser_test("let m = {'a': 1, 'b': 2}", vec![
        variable_statement(vec![
            let_variable("m", None, opt_expr(map(vec![
                (string_literal("a"), int_literal_expression(1)),
                (string_literal("b"), int_literal_expression(2)),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_map_index_access() {
    // This relies on the existing index expression parsing
    parser_test("print(my_map['key'])", vec![
        expression_statement(
            call(
                identifier("print"),
                vec![index(identifier("my_map"), string_literal("key"))]
            )
        )
    ]);
}

#[test]
fn test_for_loop_over_map_literal() {
    parser_test("
for k, v in {'a': 1, 'b': 2}
    print(k, v)
", vec![
        for_statement(
            vec![
                let_variable("k", None, None),
                let_variable("v", None, None)
            ],
            iter_obj(map(vec![
                (string_literal("a"), int_literal_expression(1)),
                (string_literal("b"), int_literal_expression(2)),
            ])),
            block(vec![
                expression_statement(
                    call(identifier("print"), vec![identifier("k"), identifier("v")])
                )
            ])
        )
    ]);
}

#[test]
fn test_method_call_on_map_literal() {
    parser_test("{'a': 1}.keys()", vec![
        expression_statement(
            call(
                member(
                    map(vec![(string_literal("a"), int_literal_expression(1))]),
                    identifier("keys")
                ),
                vec![]
            )
        )
    ]);
}

#[test]
fn test_map_of_lambdas() {
    parser_test("let funcs = {'a': fn(): 1, 'b': fn(): 2}", vec![
        variable_statement(vec![
            let_variable("funcs", None, opt_expr(map(vec![
                (string_literal("a"), lambda().build_lambda(expression_statement(int_literal_expression(1)))),
                (string_literal("b"), lambda().build_lambda(expression_statement(int_literal_expression(2)))),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_empty_map() {
    parser_test("let empty = {}", vec![
        variable_statement(vec![
            let_variable("empty", None, opt_expr(map(vec![])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_map_with_trailing_comma() {
    parser_test("let m = {'a': 1,}", vec![
        variable_statement(vec![
            let_variable("m", None, opt_expr(map(vec![
                (string_literal("a"), int_literal_expression(1)),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_map_multiline() {
    parser_test("
let m = {
    'a'
        :
            1,
    'b'
        :
 2
}", vec![
        variable_statement(vec![
            let_variable("m", None, opt_expr(map(vec![
                (string_literal("a"), int_literal_expression(1)),
                (string_literal("b"), int_literal_expression(2)),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_nested_maps_multiline() {
    parser_test("
let config = {
    'user': {
        'name': 'John',
        'id': 123,
    },
    'settings': {}
}
", vec![
        variable_statement(vec![
            let_variable("config", None, opt_expr(map(vec![
                (string_literal("user"), map(vec![
                    (string_literal("name"), string_literal("John")),
                    (string_literal("id"), int_literal_expression(123)),
                ])),
                (string_literal("settings"), map(vec![])),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_error_unclosed_map() {
    parser_error_test(
        "let m = {'a': 1",
        &SyntaxErrorKind::UnexpectedEOF
    );
}

#[test]
fn test_error_map_missing_colon() {
    // This is invalid syntax. It's not a valid map (due to :) and not a valid set (due to missing value).
    parser_error_test(
        "let m = {'a' 1}",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "}".to_string(),
            found: "int".to_string(),
        }
    );
}

#[test]
fn test_error_map_missing_comma() {
    parser_error_test(
        "let m = {'a': 1 'b': 2}",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "}".to_string(),
            found: "string".to_string(),
        }
    );
}
