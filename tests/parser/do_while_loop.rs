// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_do_while_loop() {
    combined_do_while_until_test("
do
    x -= 1
while x > 0
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal_expression(0)
    ),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::AssignSub,
                int_literal_expression(1)
            )
        )
    ])
    );
}

#[test]
fn test_do_while_loop_empty() {
    combined_do_while_until_test("
do
    // TODO
while x > 0
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal_expression(0)
    ),
    empty_statement()
    );
}

#[test]
fn test_do_while_loop_nested() {
    while_expression_test("
do
    do
        y += 1
    while y < 5
while x > 0
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal_expression(0)
    ),
    block(vec![
        do_while_statement(
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal_expression(5)
            ),
            block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("y"),
                        AssignmentOp::AssignAdd,
                        int_literal_expression(1)
                    )
                )
            ])
        )
    ]),
    WhileStatementType::DoWhile
    );
}


#[test]
fn test_do_while_loop_nested_empty() {
    while_expression_test("
do
    do
        // TODO
    while y < 5
while x > 0
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal_expression(0)
    ),
    block(vec![
        do_while_statement(
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal_expression(5)
            ),
            empty_statement()
        )
    ]),
    WhileStatementType::DoWhile
    );
}

#[test]
fn test_do_while_loop_inline() {
    combined_do_while_until_test("
do: x -= 1 while x > 0
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal_expression(0)
    ),
    expression_statement(
        assign(
            lhs_identifier("x"),
            AssignmentOp::AssignSub,
            int_literal_expression(1)
            )
        )
    );
}

#[test]
fn test_do_while_loop_containing_if_statement() {
    combined_do_while_until_test("
do
    if x % 2 == 0
        x += 1
    else
        x += 2
while x < 10
",
    binary(
        identifier("x".into()),
        BinaryOp::LessThan,
        int_literal_expression(10)
    ),
    block(vec![
        if_statement(
            binary(
                binary(
                    identifier("x".into()),
                    BinaryOp::Mod,
                    int_literal_expression(2)
                ),
                BinaryOp::Equal,
                int_literal_expression(0)
            ),
            block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("x"),
                        AssignmentOp::AssignAdd,
                        int_literal_expression(1)
                    )
                )
            ]),
            Some(
                block(vec![
                    expression_statement(
                        assign(
                            lhs_identifier("x"),
                            AssignmentOp::AssignAdd,
                            int_literal_expression(2)
                        )
                    )
                ])
            )
        )
    ])
    );
}

#[test]
fn test_do_until_loop() {
    while_expression_test("
do
    x -= 1
until x == 0
",
    binary(
        identifier("x".into()),
        BinaryOp::Equal,
        int_literal_expression(0)
    ),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::AssignSub,
                int_literal_expression(1)
            )
        )
    ]),
    WhileStatementType::DoUntil
    );
}

#[test]
fn test_do_while_with_break_and_continue() {
    while_expression_test("
do
    if x > 10: continue
    if x == 0: break
    x -= 1
while x > 0
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal_expression(0)
    ),
    block(vec![
        if_statement(
            binary(identifier("x".into()), BinaryOp::GreaterThan, int_literal_expression(10)),
            continue_statement(),
            None
        ),
        if_statement(
            binary(identifier("x".into()), BinaryOp::Equal, int_literal_expression(0)),
            break_statement(),
            None
        ),
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::AssignSub,
                int_literal_expression(1)
            )
        )
    ]),
    WhileStatementType::DoWhile
    );
}

#[test]
fn test_error_on_do_without_while_or_until() {
    parser_error_test("
do
    x = 1
", &SyntaxErrorKind::UnexpectedToken { expected: "while or until".to_string(), found: "end of file".to_string() });
}

#[test]
fn test_error_on_missing_do_while_condition() {
    parser_error_test(
        "do: x += 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "while or until".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_error_on_do_with_wrong_keyword() {
    parser_error_test("
do
    x = 1
if x > 0
", &SyntaxErrorKind::UnexpectedToken { expected: "while or until".to_string(), found: "if".to_string() });
}
