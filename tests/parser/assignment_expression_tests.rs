// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_assignment_expression() {
    parse_assignment_expression_test(
        "x = 123", 
        lhs_identifier("x".into()), 
        AssignmentOp::Assign, 
        int_literal_expression(123)
    );
}

#[test]
fn test_parse_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = 123", 
        lhs_identifier("x".into()), 
        AssignmentOp::Assign, 
        assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            int_literal_expression(123)
        )
    );
}

#[test]
fn test_parse_increment_assignment_expression() {
    parse_assignment_expression_test(
        "x += 100", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignAdd,
        int_literal_expression(100)
    );
}

#[test]
fn test_parse_decrement_assignment_expression() {
    parse_assignment_expression_test(
        "x -= 200", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignSub,
        int_literal_expression(200)
    );
}

#[test]
fn test_parse_multiplication_assignment_expression() {
    parse_assignment_expression_test(
        "x *= 10", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignMul,
        int_literal_expression(10)
    );
}

#[test]
fn test_parse_division_assignment_expression() {
    parse_assignment_expression_test(
        "x /= 10", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignDiv,
        int_literal_expression(10)
    );
}

#[test]
fn test_parse_modulo_assignment_expression() {
    parse_assignment_expression_test(
        "x %= 10", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignMod,
        int_literal_expression(10)
    );
}

#[test]
fn test_parse_increment_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = z += 100",
        lhs_identifier("x".into()),
        AssignmentOp::Assign,
        assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            assign(
                lhs_identifier("z".into()),
                AssignmentOp::AssignAdd,
                int_literal_expression(100)
            )
        )
    );
}

#[test]
fn test_parse_invalid_assignment_target() {
    // The left-hand side of an assignment must be a valid target (e.g., identifier).
    // An expression like `x + 1` is not a valid target.
    parse_error_test("x + 1 = 10", SyntaxErrorKind::InvalidLeftHandSideExpression);
}