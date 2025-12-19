// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_assignment_expression() {
    assignment_expression_test(
        "x = 123",
        lhs_identifier("x".into()),
        AssignmentOp::Assign,
        int_literal_expression(123),
    );
}

#[test]
fn test_parse_chained_assignment_expression() {
    assignment_expression_test(
        "x = y = 123",
        lhs_identifier("x".into()),
        AssignmentOp::Assign,
        assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            int_literal_expression(123),
        ),
    );
}

#[test]
fn test_parse_increment_assignment_expression() {
    assignment_expression_test(
        "x += 100",
        lhs_identifier("x".into()),
        AssignmentOp::AssignAdd,
        int_literal_expression(100),
    );
}

#[test]
fn test_parse_decrement_assignment_expression() {
    assignment_expression_test(
        "x -= 200",
        lhs_identifier("x".into()),
        AssignmentOp::AssignSub,
        int_literal_expression(200),
    );
}

#[test]
fn test_parse_multiplication_assignment_expression() {
    assignment_expression_test(
        "x *= 10",
        lhs_identifier("x".into()),
        AssignmentOp::AssignMul,
        int_literal_expression(10),
    );
}

#[test]
fn test_parse_division_assignment_expression() {
    assignment_expression_test(
        "x /= 10",
        lhs_identifier("x".into()),
        AssignmentOp::AssignDiv,
        int_literal_expression(10),
    );
}

#[test]
fn test_parse_modulo_assignment_expression() {
    assignment_expression_test(
        "x %= 10",
        lhs_identifier("x".into()),
        AssignmentOp::AssignMod,
        int_literal_expression(10),
    );
}

#[test]
fn test_parse_increment_chained_assignment_expression() {
    assignment_expression_test(
        "x = y = z += 100",
        lhs_identifier("x".into()),
        AssignmentOp::Assign,
        assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            assign(
                lhs_identifier("z".into()),
                AssignmentOp::AssignAdd,
                int_literal_expression(100),
            ),
        ),
    );
}

#[test]
fn test_parse_invalid_assignment_target() {
    run_parser_error_tests(
        vec![
            "x + 1 = 10",
            "get_x() = 10",
            "123 = 10",
            r#""hello" = 10"#,
            "await x = 5",
        ],
        &SyntaxErrorKind::InvalidLeftHandSideExpression,
    );
}

#[test]
fn test_assignment_precedence_with_binary_expression() {
    assignment_expression_test(
        "x = 1 + 2",
        lhs_identifier("x".into()),
        AssignmentOp::Assign,
        binary(
            int_literal_expression(1),
            BinaryOp::Add,
            int_literal_expression(2),
        ),
    );
}
