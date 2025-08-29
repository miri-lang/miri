// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_logical_expression() {
    parse_test("
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
    parse_test("
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
    parse_test("true and false or true", vec![
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
    parse_test("not a and b", vec![
        expression_statement(
            logical(
                unary(UnaryOp::Not, identifier("a")),
                BinaryOp::And,
                identifier("b")
            )
        )
    ]);
}
