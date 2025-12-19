// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_parse_if_statement() {
    combined_if_unless_test(
        "
if x
    x = 10
else
    x = 20
",
        identifier("x".into()),
        block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(10),
        ))]),
        Some(block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(20),
        ))])),
    );
}

#[test]
fn test_parse_if_statement_with_condition() {
    combined_if_unless_test(
        "
if x > 5
    x = 10
else
    x = 20
",
        binary(
            identifier("x"),
            BinaryOp::GreaterThan,
            int_literal_expression(5),
        ),
        block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(10),
        ))]),
        Some(block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(20),
        ))])),
    );
}

#[test]
fn test_parse_if_block_else_inline() {
    combined_if_unless_test(
        "
if x
    x = 10
else: x = 20
",
        identifier("x".into()),
        block(vec![expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(10),
        ))]),
        Some(expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(20),
        ))),
    );
}

#[test]
fn test_parse_if_inline_else_block() {
    combined_if_unless_test(
        "
if x: x = 10
else
    x = 20
",
        identifier("x".into()),
        expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal_expression(10),
        )),
        Some(block(vec![expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(20),
        ))])),
    );
}

#[test]
fn test_parse_if_statement_no_else() {
    combined_if_unless_test(
        "
if x
    x = 10
",
        identifier("x".into()),
        Statement::Block(vec![expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(10),
        ))]),
        None,
    );
}

#[test]
fn test_parse_if_statement_nested() {
    if_statement_test(
        "
if x
    if y
        x = 10
    else
        x = 20
else
    if z
        if w
            x = 30
    else
        x = 40
",
        identifier("x".into()),
        block(vec![if_statement(
            identifier("y".into()),
            block(vec![expression_statement(assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal_expression(10),
            ))]),
            Some(block(vec![expression_statement(assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal_expression(20),
            ))])),
        )]),
        Some(block(vec![if_statement(
            identifier("z".into()),
            block(vec![if_statement(
                identifier("w".into()),
                block(vec![expression_statement(assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal_expression(30),
                ))]),
                None,
            )]),
            Some(block(vec![expression_statement(assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal_expression(40),
            ))])),
        )])),
        IfStatementType::If,
    );
}

#[test]
fn test_parse_if_statement_inline() {
    combined_if_unless_test(
        "
if x: x = 10 else: x = 20
",
        identifier("x".into()),
        expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(10),
        )),
        Some(expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(20),
        ))),
    );
}

#[test]
fn test_parse_if_mixed_inline() {
    combined_if_unless_test(
        "
if x: x = 10
else: x = 20
",
        identifier("x".into()),
        expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(10),
        )),
        Some(expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(20),
        ))),
    );
}

#[test]
fn test_parse_if_statement_inline_nested() {
    if_statement_test(
        "
// This is crazy, but should work
if x: if y: x = 10 else: if z: x = 20 else: x = 30
",
        identifier("x".into()),
        if_statement(
            identifier("y".into()),
            expression_statement(assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal_expression(10),
            )),
            Some(if_statement(
                identifier("z".into()),
                expression_statement(assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal_expression(20),
                )),
                Some(expression_statement(assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal_expression(30),
                ))),
            )),
        ),
        None,
        IfStatementType::If,
    );
}

#[test]
fn test_parse_if_statement_inline_no_else() {
    combined_if_unless_test(
        "
if x: x = 10
",
        identifier("x".into()),
        expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(10),
        )),
        None,
    );
}

#[test]
fn test_parse_if_statement_precedence() {
    combined_if_unless_test(
        "
if x + 10 <= 20: x = 10
",
        binary(
            binary(
                identifier("x".into()),
                BinaryOp::Add,
                int_literal_expression(10),
            ),
            BinaryOp::LessThanEqual,
            int_literal_expression(20),
        ),
        expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(10),
        )),
        None,
    );
}

#[test]
fn test_parse_if_else_if_chain() {
    if_statement_test(
        "
if x > 10
    y = 1
else if x > 5
    y = 2
else
    y = 3
",
        binary(
            identifier("x".into()),
            BinaryOp::GreaterThan,
            int_literal_expression(10),
        ),
        block(vec![expression_statement(assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))]),
        Some(if_statement(
            binary(
                identifier("x".into()),
                BinaryOp::GreaterThan,
                int_literal_expression(5),
            ),
            block(vec![expression_statement(assign(
                lhs_identifier("y".into()),
                AssignmentOp::Assign,
                int_literal_expression(2),
            ))]),
            Some(block(vec![expression_statement(assign(
                lhs_identifier("y".into()),
                AssignmentOp::Assign,
                int_literal_expression(3),
            ))])),
        )),
        IfStatementType::If,
    );
}

#[test]
fn test_parse_if_with_variable_declaration() {
    combined_if_unless_test(
        "
if x
    let y = 10
else
    var z = 20
",
        identifier("x".into()),
        block(vec![variable_statement(
            vec![let_variable(
                "y".into(),
                None,
                opt_expr(int_literal_expression(10)),
            )],
            MemberVisibility::Public,
        )]),
        Some(block(vec![variable_statement(
            vec![var("z".into(), None, opt_expr(int_literal_expression(20)))],
            MemberVisibility::Public,
        )])),
    );
}

#[test]
fn test_parse_if_with_complex_logical_condition() {
    combined_if_unless_test(
        "
if (x > 10 and y < 5) or z == 1
    x = 1
",
        logical(
            logical(
                binary(
                    identifier("x".into()),
                    BinaryOp::GreaterThan,
                    int_literal_expression(10),
                ),
                BinaryOp::And,
                binary(
                    identifier("y".into()),
                    BinaryOp::LessThan,
                    int_literal_expression(5),
                ),
            ),
            BinaryOp::Or,
            binary(
                identifier("z".into()),
                BinaryOp::Equal,
                int_literal_expression(1),
            ),
        ),
        block(vec![expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))]),
        None,
    );
}

#[test]
fn test_parse_if_with_empty_block() {
    combined_if_unless_test(
        "
if x
    // empty then
else
    x = 1
",
        identifier("x".into()),
        empty_statement(),
        Some(block(vec![expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))])),
    );
}

#[test]
fn test_parse_if_with_empty_block_no_else() {
    combined_if_unless_test(
        "
if x
    // TODO
",
        identifier("x".into()),
        empty_statement(),
        None,
    );
}

#[test]
fn test_comment_in_empty_block() {
    // An indented block containing only a comment should parse as an empty block.
    parser_test(
        "
if x
    // This block is empty
let y = 1
",
        vec![
            if_statement(identifier("x"), empty_statement(), None),
            variable_statement(
                vec![let_variable("y", None, opt_expr(int_literal_expression(1)))],
                MemberVisibility::Public,
            ),
        ],
    );
}

#[test]
fn test_error_if_statement_as_condition() {
    // An `if` statement is not an expression and cannot be a condition.
    // The parser should expect an expression and fail on the block/inline body.
    parser_error_test(
        "if if x: 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "if".to_string(),
        },
    );
}

#[test]
fn test_parse_if_nested_empty() {
    if_statement_test(
        "
if x
    if y
        // TODO
",
        identifier("x".into()),
        block(vec![if_statement(
            identifier("y".into()),
            empty_statement(),
            None,
        )]),
        None,
        IfStatementType::If,
    );
}

#[test]
fn test_parse_if_with_empty_else_block() {
    combined_if_unless_test(
        "
if x
    x = 1
else
    // empty else
",
        identifier("x".into()),
        block(vec![expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal_expression(1),
        ))]),
        Some(empty_statement()),
    );
}

#[test]
fn test_parse_if_with_empty_else_block_with_followup() {
    parser_test(
        "
if x
    x = 1
else
    // empty else
x = 2
",
        vec![
            if_statement(
                identifier("x".into()),
                block(vec![expression_statement(assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal_expression(1),
                ))]),
                Some(empty_statement()),
            ),
            expression_statement(assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal_expression(2),
            )),
        ],
    );
}

#[test]
fn test_parse_if_with_empty_inline_else_block_with_followup() {
    parser_test(
        "
if x
    x = 1
else: // empty else
x = 2
",
        vec![
            if_statement(
                identifier("x".into()),
                block(vec![expression_statement(assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal_expression(1),
                ))]),
                Some(empty_statement()),
            ),
            expression_statement(assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal_expression(2),
            )),
        ],
    );
}

#[test]
fn test_error_dangling_else() {
    // An `else` without a preceding `if` is a syntax error.
    parser_error_test(
        "else: print('error')",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(), // Or a more specific expectation
            found: "else".to_string(),
        },
    );
}

#[test]
fn test_parse_unless_else_if_chain() {
    parser_test(
        "
unless x > 10
    y = 1
else if x > 5
    y = 2
else
    y = 3
",
        vec![unless_statement(
            binary(
                identifier("x"),
                BinaryOp::GreaterThan,
                int_literal_expression(10),
            ),
            block(vec![expression_statement(assign(
                lhs_identifier("y"),
                AssignmentOp::Assign,
                int_literal_expression(1),
            ))]),
            Some(if_statement(
                binary(
                    identifier("x"),
                    BinaryOp::GreaterThan,
                    int_literal_expression(5),
                ),
                block(vec![expression_statement(assign(
                    lhs_identifier("y"),
                    AssignmentOp::Assign,
                    int_literal_expression(2),
                ))]),
                Some(block(vec![expression_statement(assign(
                    lhs_identifier("y"),
                    AssignmentOp::Assign,
                    int_literal_expression(3),
                ))])),
            )),
        )],
    );
}
