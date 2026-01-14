// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_while_loop() {
    combined_while_until_test(
        "
while x > 0
    x -= 1
",
        binary(
            identifier("x".into()),
            BinaryOp::GreaterThan,
            int_literal_expression(0),
        ),
        block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::AssignSub,
            int_literal_expression(1),
        ))]),
    );
}

#[test]
fn test_while_loop_empty() {
    combined_while_until_test(
        "
while x > 0
    // TODO
",
        binary(
            identifier("x".into()),
            BinaryOp::GreaterThan,
            int_literal_expression(0),
        ),
        empty_statement(),
    );
}

#[test]
fn test_while_loop_nested() {
    while_expression_test(
        "
while x > 0
    while y < 5
        y += 1
",
        binary(
            identifier("x".into()),
            BinaryOp::GreaterThan,
            int_literal_expression(0),
        ),
        block(vec![while_statement(
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal_expression(5),
            ),
            block(vec![expression_statement(assign(
                lhs_identifier("y"),
                AssignmentOp::AssignAdd,
                int_literal_expression(1),
            ))]),
        )]),
        WhileStatementType::While,
    );
}

#[test]
fn test_while_loop_nested_empty() {
    while_expression_test(
        "
while x > 0
    while y < 5
        // TODO
",
        binary(
            identifier("x".into()),
            BinaryOp::GreaterThan,
            int_literal_expression(0),
        ),
        block(vec![while_statement(
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal_expression(5),
            ),
            empty_statement(),
        )]),
        WhileStatementType::While,
    );
}

#[test]
fn test_while_loop_inline() {
    combined_while_until_test(
        "
while x > 0: x -= 1
",
        binary(
            identifier("x".into()),
            BinaryOp::GreaterThan,
            int_literal_expression(0),
        ),
        expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::AssignSub,
            int_literal_expression(1),
        )),
    );
}

#[test]
fn test_while_loop_containing_if_statement() {
    combined_while_until_test(
        "
while x < 10
    if x % 2 == 0
        x += 1
    else
        x += 2
",
        binary(
            identifier("x".into()),
            BinaryOp::LessThan,
            int_literal_expression(10),
        ),
        block(vec![if_statement(
            binary(
                binary(
                    identifier("x".into()),
                    BinaryOp::Mod,
                    int_literal_expression(2),
                ),
                BinaryOp::Equal,
                int_literal_expression(0),
            ),
            block(vec![expression_statement(assign(
                lhs_identifier("x"),
                AssignmentOp::AssignAdd,
                int_literal_expression(1),
            ))]),
            Some(block(vec![expression_statement(assign(
                lhs_identifier("x"),
                AssignmentOp::AssignAdd,
                int_literal_expression(2),
            ))])),
        )]),
    );
}

#[test]
fn test_while_loop_with_complex_multiline_condition() {
    combined_while_until_test(
        "
while (
    x > 0
    and (y < 5)
)
    x -= 1
",
        logical(
            binary(
                identifier("x".into()),
                BinaryOp::GreaterThan,
                int_literal_expression(0),
            ),
            BinaryOp::And,
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal_expression(5),
            ),
        ),
        block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::AssignSub,
            int_literal_expression(1),
        ))]),
    );
}

#[test]
fn test_while_loop_with_break_and_continue() {
    combined_while_until_test(
        "
while true
    if condition()
        continue
    else
        break
",
        boolean_literal(true),
        block(vec![if_statement(
            call(identifier("condition"), vec![]),
            block(vec![continue_statement()]),
            Some(block(vec![break_statement()])),
        )]),
    );
}

#[test]
fn test_error_on_missing_loop_condition() {
    parser_error_test("while", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_on_missing_loop_body() {
    parser_error_test(
        "while x > 0",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a colon or an expression end".to_string(),
            found: "end of file".to_string(),
        },
    );
}
