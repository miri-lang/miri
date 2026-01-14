// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

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
    parser_test(
        "-x * -2",
        vec![expression_statement(binary(
            unary(UnaryOp::Negate, identifier("x".into())),
            BinaryOp::Mul,
            unary(UnaryOp::Negate, int_literal_expression(2)),
        ))],
    );
}

#[test]
fn test_precedence_of_member_access_and_unary_negation() {
    // Member access `.` has higher precedence than unary `-`.
    // This should parse as `-(a.b)`.
    parser_test(
        "-a.b",
        vec![expression_statement(unary(
            UnaryOp::Negate,
            member(identifier("a"), identifier("b")),
        ))],
    );
}

#[test]
fn test_precedence_of_index_and_unary_negation() {
    // Index access `[]` has higher precedence than unary `-`.
    // This should parse as `-(a[0])`.
    parser_test(
        "-a[0]",
        vec![expression_statement(unary(
            UnaryOp::Negate,
            index(identifier("a"), int_literal_expression(0)),
        ))],
    );
}

#[test]
fn test_precedence_of_call_and_unary_negation() {
    // Function calls `()` have higher precedence than unary `-`.
    // This should parse as `-(a())`.
    parser_test(
        "-a()",
        vec![expression_statement(unary(
            UnaryOp::Negate,
            call(identifier("a"), vec![]),
        ))],
    );
}

#[test]
fn test_chained_unary_operator() {
    parser_test(
        "not not y",
        vec![expression_statement(unary(
            UnaryOp::Not,
            unary(UnaryOp::Not, identifier("y")),
        ))],
    );
}

#[test]
fn test_error_on_dangling_unary_operator() {
    // A unary operator must be followed by an expression.
    parser_error_test("not", &SyntaxErrorKind::UnexpectedEOF);
    parser_error_test("let x = -", &SyntaxErrorKind::UnexpectedEOF);
}
