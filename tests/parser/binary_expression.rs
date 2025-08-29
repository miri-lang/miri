// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_binary_expression() {
    parse_binary_expression_test(
        "123 + 456",
        int_literal_expression(123),
        BinaryOp::Add,
        int_literal_expression(456)
    );
}

#[test]
fn test_parse_chained_binary_expression() {
    parse_binary_expression_test(
        "123 + 456 - 789",
        binary(
            int_literal_expression(123),
            BinaryOp::Add,
            int_literal_expression(456)
        ),
        BinaryOp::Sub,
        int_literal_expression(789)
    );
}

#[test]
fn test_parse_chained_multiply_expression() {
    parse_test("2 + 2 * 2", vec![
        expression_statement(
            binary(
                int_literal_expression(2),
                BinaryOp::Add,
                binary(int_literal_expression(2), BinaryOp::Mul, int_literal_expression(2))
            )
        )
    ]);
}

#[test]
fn test_parse_bitwise_and_expression() {
    parse_binary_expression_test(
        "1 + 2 & 2",
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2)
        ),
        BinaryOp::BitwiseAnd,
        int_literal_expression(2)
    );
}

#[test]
fn test_parse_bitwise_or_expression() {
    parse_binary_expression_test(
        "1 + 2 | 2",
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2)
        ),
        BinaryOp::BitwiseOr,
        int_literal_expression(2)
    );
}

#[test]
fn test_parse_bitwise_xor_expression() {
    parse_binary_expression_test(
        "1 + 2 ^ 2",
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2)
        ),
        BinaryOp::BitwiseXor,
        int_literal_expression(2)
    );
}


#[test]
fn test_parse_multiply_with_parentheses_expression() {
    parse_binary_expression_test(
        "(2 + 2) * 2",
        binary(
            int_literal_expression(2),
            BinaryOp::Add,
            int_literal_expression(2)
        ),
        BinaryOp::Mul,
        int_literal_expression(2)
    );
}

#[test]
fn test_parse_consecutive_operators() {
    // Two binary operators in a row is invalid.
    parse_error_test(
        "5 + * 2", 
        SyntaxErrorKind::UnexpectedToken { 
            expected: "literal, parenthesized expression, identifier, lambda, list, map or set".into(), 
            found: "*".into() 
        }
    );
}

#[test]
fn test_parse_incomplete_expression() {
    // The parser should error on an incomplete binary expression.
    parse_error_test(
        "5 +", 
        SyntaxErrorKind::UnexpectedEOF
    );
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
