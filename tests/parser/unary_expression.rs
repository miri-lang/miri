// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_unary_expression_negate() {
    unary_expression_test("-x", UnaryOp::Negate, identifier("x".into()));
}

#[test]
fn test_unary_expression_plus() {
    unary_expression_test("+x", UnaryOp::Plus, identifier("x".into()));
}

#[test]
fn test_unary_expression_not() {
    unary_expression_test("not x", UnaryOp::Not, identifier("x".into()));
}

#[test]
fn test_unary_expression_bitwise_not() {
    unary_expression_test("~x", UnaryOp::BitwiseNot, identifier("x".into()));
}

#[test]
fn test_unary_expression_increment() {
    unary_expression_test("++x", UnaryOp::Increment, identifier("x".into()));
}

#[test]
fn test_unary_expression_decrement() {
    unary_expression_test("--x", UnaryOp::Decrement, identifier("x".into()));
}

#[test]
fn test_unary_expression_precedence() {
    parser_test("-x * -2", vec![
        expression_statement(
            binary(
                unary(UnaryOp::Negate, identifier("x".into())),
                BinaryOp::Mul,
                unary(UnaryOp::Negate, int_literal_expression(2))
            )
        )
    ]);
}

#[test]
fn test_precedence_of_member_access_and_unary_negation() {
    // Member access `.` has higher precedence than unary `-`.
    // This should parse as `-(a.b)`.
    parser_test("-a.b", vec![
        expression_statement(
            unary(
                UnaryOp::Negate,
                member(identifier("a"), identifier("b"))
            )
        )
    ]);
}
