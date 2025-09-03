// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_logical_expression() {
    parser_test("
x > 10 and y <= 8
",
        vec![
            expression_statement(
                logical(
                    binary(
                        identifier("x".into()),
                        BinaryOp::GreaterThan,
                        int_literal_expression(10)
                    ),
                    BinaryOp::And,
                    binary(
                        identifier("y".into()),
                        BinaryOp::LessThanEqual,
                        int_literal_expression(8)
                    )
                )
            )
        ]
    );
}

#[test]
fn test_logical_expression_and_precedence() {
    parser_test("
x > 1 and y <= 2 or y == 10
",
        vec![
            expression_statement(
                logical(
                    logical(
                        binary(
                            identifier("x".into()),
                            BinaryOp::GreaterThan,
                            int_literal_expression(1)
                        ),
                        BinaryOp::And,
                        binary(
                            identifier("y".into()),
                            BinaryOp::LessThanEqual,
                            int_literal_expression(2)
                        )
                    ),
                    BinaryOp::Or,
                    binary(
                        identifier("y".into()),
                        BinaryOp::Equal,
                        int_literal_expression(10)
                    )
                )
            )
        ]
    );
}

#[test]
fn test_precedence_of_logical_and_or() {
    // `and` has higher precedence than `or`.
    // This should parse as `(true and false) or true`.
    parser_test("true and false or true", vec![
        expression_statement(
            logical(
                logical(
                    boolean_literal(true),
                    BinaryOp::And,
                    boolean_literal(false)
                ),
                BinaryOp::Or,
                boolean_literal(true)
            )
        )
    ]);
}

#[test]
fn test_precedence_of_unary_not_and_logical_and() {
    // `not` should have higher precedence than `and`.
    // This should parse as `(not a) and b`.
    parser_test("not a and b", vec![
        expression_statement(
            logical(
                unary(UnaryOp::Not, identifier("a")),
                BinaryOp::And,
                identifier("b")
            )
        )
    ]);
}

#[test]
fn test_chained_logical_or_expression() {
    // This test verifies the left-associativity of the `or` operator.
    // It should parse as `(a or b) or c`.
    parser_test("a or b or c", vec![
        expression_statement(
            logical(
                logical(
                    identifier("a"),
                    BinaryOp::Or,
                    identifier("b")
                ),
                BinaryOp::Or,
                identifier("c")
            )
        )
    ]);
}

#[test]
fn test_precedence_of_unary_not_and_logical_or() {
    // `not` should have higher precedence than `or`.
    // This should parse as `(not a) or b`.
    parser_test("not a or b", vec![
        expression_statement(
            logical(
                unary(UnaryOp::Not, identifier("a")),
                BinaryOp::Or,
                identifier("b")
            )
        )
    ]);
}

#[test]
fn test_logical_expression_with_parentheses() {
    // Parentheses should override the default precedence of `and` over `or`.
    // This should parse as `a and (b or c)`.
    parser_test("a and (b or c)", vec![
        expression_statement(
            logical(
                identifier("a"),
                BinaryOp::And,
                logical(
                    identifier("b"),
                    BinaryOp::Or,
                    identifier("c")
                )
            )
        )
    ]);
}

#[test]
fn test_error_on_consecutive_logical_operators() {
    // The parser should fail if it encounters two logical operators in a row.
    parser_error_test("a and and b", &SyntaxErrorKind::UnexpectedToken {
        expected: "an expression".to_string(),
        found: "and".to_string(),
    });
}

#[test]
fn test_error_on_missing_rhs_logical() {
    // The parser should fail if a logical operator is not followed by an expression.
    parser_error_test("a or", &SyntaxErrorKind::UnexpectedEOF);
}

