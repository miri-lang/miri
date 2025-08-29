// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_conditional_expression() {
    parse_test("
let x = 10 if y > 5 else 20
",
    vec![
        variable_statement(vec![
            let_variable(
                "x".into(),
                None,
                opt_expr(
                    if_conditional(
                        int_literal_expression(10),
                        binary(
                            identifier("y".into()),
                            BinaryOp::GreaterThan,
                            int_literal_expression(5)
                        ),
                        Some(int_literal_expression(20)),
                    )
                )
            )
        ], MemberVisibility::Public)
    ]
    )
}

#[test]
fn test_parse_conditional_expression_no_else() {
    parse_test("
var x = 100 if y % 2 == 0
",
    vec![
        variable_statement(vec![
            var(
                "x".into(),
                None,
                opt_expr(
                    if_conditional(
                        int_literal_expression(100),
                        binary(
                            binary(
                                identifier("y".into()),
                                BinaryOp::Mod,
                                int_literal_expression(2)
                            ),
                            BinaryOp::Equal,
                            int_literal_expression(0)
                        ),
                        None,
                    )
                ),
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_parse_conditional_expression_with_unless() {
    parse_test("
var x = 1 unless y
",
    vec![
        variable_statement(vec![
            var(
                "x".into(),
                None,
                opt_expr(
                    unless_conditional(
                        int_literal_expression(1),
                        identifier("y".into()),
                        None
                    )
                )
            )
        ], MemberVisibility::Public)
    ])
}

#[test]
fn test_conditional_expression_as_if_condition() {
    // Using a ternary-style if as the condition for a statement-style if.
    parse_if_statement_test("
if a if b else c
    x = 1
",
        if_conditional(
            identifier("a".into()),
            identifier("b".into()),
            Some(identifier("c".into())),
        ),
        block(vec![
            expression_statement(
                assign(
                    lhs_identifier("x"),
                    AssignmentOp::Assign,
                    int_literal_expression(1)
                )
            )
        ]),
        None,
        IfStatementType::If
    );
}

#[test]
fn test_precedence_of_assignment_and_conditional_expression() {
    // The conditional expression has higher precedence than assignment.
    // This should parse as `x = (1 if y else 2)`.
    parse_test("x = 1 if y else 2", vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::Assign,
                if_conditional(
                    int_literal_expression(1),
                    identifier("y"),
                    Some(int_literal_expression(2))
                )
            )
        )
    ]);
}
