// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{if_statement_test, parser_error_test, parser_test};
use miri::ast::factory::{
    assign, binary, block, expression_statement, identifier, if_conditional,
    int_literal_expression, let_variable, lhs_identifier, logical, unless_conditional, var,
    variable_statement,
};
use miri::ast::{opt_expr, AssignmentOp, BinaryOp, IfStatementType, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_conditional_expression() {
    parser_test(
        "
let x = 10 if y > 5 else 20
",
        vec![variable_statement(
            vec![let_variable(
                "x",
                None,
                opt_expr(if_conditional(
                    int_literal_expression(10),
                    binary(
                        identifier("y"),
                        BinaryOp::GreaterThan,
                        int_literal_expression(5),
                    ),
                    Some(int_literal_expression(20)),
                )),
            )],
            MemberVisibility::Public,
        )],
    )
}

#[test]
fn test_parse_conditional_expression_no_else() {
    parser_test(
        "
var x = 100 if y % 2 == 0
",
        vec![variable_statement(
            vec![var(
                "x",
                None,
                opt_expr(if_conditional(
                    int_literal_expression(100),
                    binary(
                        binary(identifier("y"), BinaryOp::Mod, int_literal_expression(2)),
                        BinaryOp::Equal,
                        int_literal_expression(0),
                    ),
                    None,
                )),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_parse_conditional_expression_with_unless() {
    parser_test(
        "
var x = 1 unless y
",
        vec![variable_statement(
            vec![var(
                "x",
                None,
                opt_expr(unless_conditional(
                    int_literal_expression(1),
                    identifier("y"),
                    None,
                )),
            )],
            MemberVisibility::Public,
        )],
    )
}

#[test]
fn test_conditional_expression_as_if_condition() {
    // Using a ternary-style if as the condition for a statement-style if.
    if_statement_test(
        "
if a if b else c
    x = 1
",
        if_conditional(identifier("a"), identifier("b"), Some(identifier("c"))),
        block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))]),
        None,
        IfStatementType::If,
    );
}

#[test]
fn test_precedence_of_assignment_and_conditional_expression() {
    // The conditional expression has higher precedence than assignment.
    // This should parse as `x = (1 if y else 2)`.
    parser_test(
        "x = 1 if y else 2",
        vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            if_conditional(
                int_literal_expression(1),
                identifier("y"),
                Some(int_literal_expression(2)),
            ),
        ))],
    );
}

#[test]
fn test_chained_conditional_expression() {
    // This test verifies right-associativity.
    // It should parse as `1 if a else (2 if b else 3)`.
    parser_test(
        "1 if a else 2 if b else 3",
        vec![expression_statement(if_conditional(
            int_literal_expression(1),
            identifier("a"),
            Some(if_conditional(
                int_literal_expression(2),
                identifier("b"),
                Some(int_literal_expression(3)),
            )),
        ))],
    );
}

#[test]
fn test_precedence_with_logical_operators() {
    // The logical `and` operator has higher precedence than the conditional expression.
    // This should parse as `(a and b) if c else d`.
    parser_test(
        "a and b if c else d",
        vec![expression_statement(if_conditional(
            logical(identifier("a"), BinaryOp::And, identifier("b")),
            identifier("c"),
            Some(identifier("d")),
        ))],
    );
}

#[test]
fn test_nested_conditional_in_condition() {
    // The condition of a conditional can itself be a conditional.
    // This should parse as `a if (b if c else d) else e`.
    parser_test(
        "a if b if c else d else e",
        vec![expression_statement(if_conditional(
            identifier("a"),
            if_conditional(identifier("b"), identifier("c"), Some(identifier("d"))),
            Some(identifier("e")),
        ))],
    );
}

#[test]
fn test_error_on_incomplete_conditional() {
    // A conditional with `else` must be followed by an expression.
    parser_error_test("x if y else", &SyntaxErrorKind::UnexpectedEOF);
}
