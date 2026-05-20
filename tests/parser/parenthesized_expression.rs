// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::common::MemberVisibility;
use miri::ast::factory::{
    binary, block, boolean_literal, expression_statement, func, int_literal_expression,
    let_variable, string_literal_expression, tuple, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parentheses_override_precedence() {
    // The primary use of parentheses: overriding default operator precedence.
    // This should parse as `(1 + 2) * 3`, not `1 + (2 * 3)`.
    parser_test(
        "(1 + 2) * 3",
        vec![expression_statement(binary(
            binary(
                int_literal_expression(1),
                BinaryOp::Add,
                int_literal_expression(2),
            ),
            BinaryOp::Mul,
            int_literal_expression(3),
        ))],
    );
}

#[test]
fn test_nested_parenthesized_expression() {
    parser_test(
        "((1))",
        vec![expression_statement(int_literal_expression(1))],
    );
}

#[test]
fn test_empty_tuple_literal() {
    // `()` should parse as an empty tuple.
    parser_test("()", vec![expression_statement(tuple(vec![]))]);
}

#[test]
fn test_single_element_tuple_literal() {
    // A single element followed by a comma `(1,)` is a tuple.
    parser_test(
        "(1,)",
        vec![expression_statement(tuple(vec![int_literal_expression(1)]))],
    );
}

#[test]
fn test_multi_element_tuple_literal() {
    // Multiple comma-separated elements are a tuple.
    parser_test(
        "(1, 'a', true)",
        vec![expression_statement(tuple(vec![
            int_literal_expression(1),
            string_literal_expression("a"),
            boolean_literal(true),
        ]))],
    );
}

#[test]
fn test_error_on_unclosed_parenthesis() {
    parser_error_test("(5 + 2", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_parse_mismatched_parentheses() {
    // Mismatched brackets should be a syntax error.
    parser_error_test(
        "(5 + 2]",
        &SyntaxErrorKind::UnexpectedToken {
            expected: ")".into(),
            found: "]".into(),
        },
    );
}

/// An expression inside parentheses can span multiple lines: newlines between
/// `(` and `)` do not terminate the statement.
#[test]
fn test_parenthesized_expression_spans_multiple_lines() {
    parser_test(
        "
(1 +
    2)
",
        vec![expression_statement(binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2),
        ))],
    );
}

/// Same rule applies when the parenthesised expression is nested inside an
/// indented code block (function body, if body, etc.) — historically the
/// indent-aware lexer emitted a stray `ExpressionStatementEnd` inside the
/// parens, breaking the expression.
#[test]
fn test_parenthesized_expression_multiline_inside_indented_block() {
    parser_test(
        "
fn main()
    let x = (1 +
        2)
",
        vec![func("main").build(block(vec![variable_statement(
            vec![let_variable(
                "x",
                None,
                opt_expr(binary(
                    int_literal_expression(1),
                    BinaryOp::Add,
                    int_literal_expression(2),
                )),
            )],
            MemberVisibility::Public,
        )]))],
    );
}
