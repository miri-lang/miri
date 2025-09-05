// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::opt_expr;
use miri::ast::MemberVisibility;
use miri::syntax_error::SyntaxErrorKind;

use super::ast_builder::*;
use super::utils::*;



#[test]
fn test_parse_symbol_literal() {
    literal_test(":my_fancy_symbol", symbol("my_fancy_symbol"));
}

#[test]
fn test_symbol_in_variable_declaration() {
    parser_test("let x = :my_symbol", vec![
        variable_statement(vec![
            let_variable(
                "x",
                None,
                opt_expr(symbol_literal("my_symbol"))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_symbol_as_map_key() {
    parser_test("let m = {:key1: 1, :key2: 'value'}", vec![
        variable_statement(vec![
            let_variable("m", None, opt_expr(map(vec![
                (symbol_literal("key1"), int_literal_expression(1)),
                (symbol_literal("key2"), string_literal("value")),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_symbol_as_function_argument() {
    parser_test("set_option(:enabled)", vec![
        expression_statement(
            call(
                identifier("set_option"),
                vec![symbol_literal("enabled")]
            )
        )
    ]);
}

#[test]
fn test_symbol_with_keyword_name() {
    literal_test(":if", symbol("if"));
    literal_test(":true", symbol("true"));
}

#[test]
fn test_error_on_standalone_colon() {
    parser_error_test(":", &SyntaxErrorKind::UnexpectedToken {
        expected: "an expression".to_string(),
        found: ":".to_string(),
    });
}
