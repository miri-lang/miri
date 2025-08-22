// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_equality_expression() {
    parse_test("
x > 10 == false
",
        vec![
            expression_statement(
                binary(
                    binary(
                        identifier("x".into()),
                        BinaryOp::GreaterThan,
                        int_literal_expression(10)
                    ),
                    BinaryOp::Equal,
                    boolean_literal(false)
                )
            )
        ]
    );
}

#[test]
fn test_equality_expression_not_equal() {
    parse_test("
x >= 8 != true
",
        vec![
            expression_statement(
                binary(
                    binary(
                        identifier("x".into()),
                        BinaryOp::GreaterThanEqual,
                        int_literal_expression(8)
                    ),
                    BinaryOp::NotEqual,
                    boolean_literal(true)
                )
            )
        ]
    );
}

#[test]
fn test_precedence_of_bitwise_and_equality() {
    // Equality (==) has lower precedence than bitwise AND (&).
    // This should parse as `(x & 10) == 10`.
    parse_test("x & 10 == 10", vec![
        expression_statement(
            binary(
                binary(
                    identifier("x".into()),
                    BinaryOp::BitwiseAnd,
                    int_literal_expression(10)
                ),
                BinaryOp::Equal,
                int_literal_expression(10)
            )
        )
    ]);
}
