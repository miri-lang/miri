// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    binary, boolean_literal, expression_statement, identifier, int_literal_expression, logical,
};
use miri::ast::BinaryOp;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_equality_expression() {
    parser_test(
        "
x > 10 == false
",
        vec![expression_statement(binary(
            binary(
                identifier("x"),
                BinaryOp::GreaterThan,
                int_literal_expression(10),
            ),
            BinaryOp::Equal,
            boolean_literal(false),
        ))],
    );
}

#[test]
fn test_equality_expression_not_equal() {
    parser_test(
        "
x >= 8 != true
",
        vec![expression_statement(binary(
            binary(
                identifier("x"),
                BinaryOp::GreaterThanEqual,
                int_literal_expression(8),
            ),
            BinaryOp::NotEqual,
            boolean_literal(true),
        ))],
    );
}

#[test]
fn test_precedence_of_bitwise_and_equality() {
    // Equality (==) has lower precedence than bitwise AND (&).
    // This should parse as `(x & 10) == 10`.
    parser_test(
        "x & 10 == 10",
        vec![expression_statement(binary(
            binary(
                identifier("x"),
                BinaryOp::BitwiseAnd,
                int_literal_expression(10),
            ),
            BinaryOp::Equal,
            int_literal_expression(10),
        ))],
    );
}

#[test]
fn test_chained_equality_expression() {
    parser_test(
        "a == b == c",
        vec![expression_statement(binary(
            binary(identifier("a"), BinaryOp::Equal, identifier("b")),
            BinaryOp::Equal,
            identifier("c"),
        ))],
    );
}

#[test]
fn test_mixed_chained_equality_expression() {
    parser_test(
        "a == b != c",
        vec![expression_statement(binary(
            binary(identifier("a"), BinaryOp::Equal, identifier("b")),
            BinaryOp::NotEqual,
            identifier("c"),
        ))],
    );
}

#[test]
fn test_precedence_of_logical_and_equality() {
    parser_test(
        "a and b == c",
        vec![expression_statement(logical(
            identifier("a"),
            BinaryOp::And,
            binary(identifier("b"), BinaryOp::Equal, identifier("c")),
        ))],
    );
}

#[test]
fn test_error_on_consecutive_equality_operators() {
    parser_error_test(
        "a == == b",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "==".to_string(),
        },
    );
}
