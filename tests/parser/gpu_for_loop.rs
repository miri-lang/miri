// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parse_program, parser_error_test};
use miri::ast::factory::{
    assign, empty_statement, expression_statement, gpu_for_statement, identifier,
    int_literal_expression, let_variable, lhs_identifier, range,
};
use miri::ast::{opt_expr, AssignmentOp, RangeExpressionType, StatementKind};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_gpu_for_loop_block_body() {
    let program = parse_program(
        "
gpu for i in 0..16
    x = i
",
    );
    assert_eq!(program.body.len(), 1);
    assert!(matches!(
        program.body[0].node,
        StatementKind::GpuFor(_, _, _)
    ));
}

#[test]
fn test_gpu_for_loop_inline_body() {
    let program = parse_program(
        "
gpu for i in 0..16: x = i
",
    );
    let expected = gpu_for_statement(
        vec![let_variable("i", None, None)],
        range(
            int_literal_expression(0),
            opt_expr(int_literal_expression(16)),
            RangeExpressionType::Exclusive,
        ),
        expression_statement(assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            identifier("i"),
        )),
    );
    assert_eq!(program.body[0], expected);
}

#[test]
fn test_gpu_for_loop_empty_body() {
    let program = parse_program(
        "
gpu for i in 0..4: // nothing
",
    );
    let expected = gpu_for_statement(
        vec![let_variable("i", None, None)],
        range(
            int_literal_expression(0),
            opt_expr(int_literal_expression(4)),
            RangeExpressionType::Exclusive,
        ),
        empty_statement(),
    );
    assert_eq!(program.body[0], expected);
}

#[test]
fn test_gpu_fn_still_parses_after_gpu_for_dispatch() {
    let program = parse_program(
        "
gpu fn kernel(x int)
    let y = x + 1
",
    );
    assert!(matches!(
        program.body[0].node,
        StatementKind::FunctionDeclaration(_)
    ));
}

#[test]
fn test_gpu_for_requires_in_keyword() {
    parser_error_test(
        "
gpu for i 0..3
    x = i
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "int".to_string(),
        },
    );
}
