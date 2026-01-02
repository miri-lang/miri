// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_binary_expression() {
    binary_expression_test(
        "123 + 456",
        int_literal_expression(123),
        BinaryOp::Add,
        int_literal_expression(456),
    );
}

#[test]
fn test_parse_chained_binary_expression() {
    binary_expression_test(
        "123 + 456 - 789",
        binary(
            int_literal_expression(123),
            BinaryOp::Add,
            int_literal_expression(456),
        ),
        BinaryOp::Sub,
        int_literal_expression(789),
    );
}

#[test]
fn test_parse_chained_multiply_expression() {
    parser_test(
        "2 + 2 * 2",
        vec![expression_statement(binary(
            int_literal_expression(2),
            BinaryOp::Add,
            binary(
                int_literal_expression(2),
                BinaryOp::Mul,
                int_literal_expression(2),
            ),
        ))],
    );
}

#[test]
fn test_parse_bitwise_and_expression() {
    binary_expression_test(
        "1 + 2 & 2",
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2),
        ),
        BinaryOp::BitwiseAnd,
        int_literal_expression(2),
    );
}

#[test]
fn test_parse_bitwise_or_expression() {
    binary_expression_test(
        "1 + 2 | 2",
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2),
        ),
        BinaryOp::BitwiseOr,
        int_literal_expression(2),
    );
}

#[test]
fn test_parse_bitwise_xor_expression() {
    binary_expression_test(
        "1 + 2 ^ 2",
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2),
        ),
        BinaryOp::BitwiseXor,
        int_literal_expression(2),
    );
}

#[test]
fn test_parse_multiply_with_parentheses_expression() {
    binary_expression_test(
        "(2 + 2) * 2",
        binary(
            int_literal_expression(2),
            BinaryOp::Add,
            int_literal_expression(2),
        ),
        BinaryOp::Mul,
        int_literal_expression(2),
    );
}

#[test]
fn test_parse_consecutive_operators() {
    // Two binary operators in a row is invalid.
    parser_error_test(
        "5 + * 2",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".into(),
            found: "*".into(),
        },
    );
}

#[test]
fn test_parse_incomplete_expression() {
    // The parser should error on an incomplete binary expression.
    parser_error_test("5 +", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_very_long_chain_of_binary_operators() {
    // Stress test the loop-based expression parsing to ensure it doesn't have performance issues
    // or stack overflows (which it shouldn't, but this is a good sanity check).

    // We don't need to build the full AST here, just confirm it parses without crashing.
    // A more dedicated test could build the deeply nested tree if desired.
    // For now, we just check that `parser.parse()` returns Ok.
    let long_expr = "1 + ".repeat(500) + "1";
    parse_program(&long_expr);
}

#[test]
fn test_precedence_of_equality_and_relational_operators() {
    // Relational operators (`<`, `>`) have higher precedence than equality (`==`, `!=`).
    // This should parse as `(a < b) == (c > d)`.
    parser_test(
        "a < b == c > d",
        vec![expression_statement(binary(
            binary(identifier("a"), BinaryOp::LessThan, identifier("b")),
            BinaryOp::Equal,
            binary(identifier("c"), BinaryOp::GreaterThan, identifier("d")),
        ))],
    );
}

#[test]
fn test_precedence_of_additive_and_relational_operators() {
    // Additive operators (`+`, `-`) have higher precedence than relational operators.
    // This should parse as `(a + b) > (c - d)`.
    parser_test(
        "a + b > c - d",
        vec![expression_statement(binary(
            binary(identifier("a"), BinaryOp::Add, identifier("b")),
            BinaryOp::GreaterThan,
            binary(identifier("c"), BinaryOp::Sub, identifier("d")),
        ))],
    );
}

#[test]
fn test_precedence_of_logical_and_equality_operators() {
    // Equality operators have higher precedence than `and`.
    // This should parse as `(a == b) and (c != d)`.
    parser_test(
        "a == b and c != d",
        vec![expression_statement(logical(
            binary(identifier("a"), BinaryOp::Equal, identifier("b")),
            BinaryOp::And,
            binary(identifier("c"), BinaryOp::NotEqual, identifier("d")),
        ))],
    );
}

#[test]
fn test_all_binary_operators() {
    let test_cases = vec![
        ("a + b", identifier("a"), BinaryOp::Add, identifier("b")),
        ("a - b", identifier("a"), BinaryOp::Sub, identifier("b")),
        ("a * b", identifier("a"), BinaryOp::Mul, identifier("b")),
        ("a / b", identifier("a"), BinaryOp::Div, identifier("b")),
        ("a % b", identifier("a"), BinaryOp::Mod, identifier("b")),
        ("a == b", identifier("a"), BinaryOp::Equal, identifier("b")),
        (
            "a != b",
            identifier("a"),
            BinaryOp::NotEqual,
            identifier("b"),
        ),
        (
            "a < b",
            identifier("a"),
            BinaryOp::LessThan,
            identifier("b"),
        ),
        (
            "a <= b",
            identifier("a"),
            BinaryOp::LessThanEqual,
            identifier("b"),
        ),
        (
            "a > b",
            identifier("a"),
            BinaryOp::GreaterThan,
            identifier("b"),
        ),
        (
            "a >= b",
            identifier("a"),
            BinaryOp::GreaterThanEqual,
            identifier("b"),
        ),
        (
            "a & b",
            identifier("a"),
            BinaryOp::BitwiseAnd,
            identifier("b"),
        ),
        (
            "a | b",
            identifier("a"),
            BinaryOp::BitwiseOr,
            identifier("b"),
        ),
        (
            "a ^ b",
            identifier("a"),
            BinaryOp::BitwiseXor,
            identifier("b"),
        ),
    ];

    for (input, left, op, right) in test_cases {
        binary_expression_test(input, left, op, right);
    }
}
