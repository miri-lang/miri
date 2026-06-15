// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parse_program, parser_error_test};
use miri::ast::factory::{
    assign, empty_statement, expression_statement, forall_statement, identifier,
    int_literal_expression, let_variable, lhs_identifier, range,
};
use miri::ast::statement::AcceleratorTarget;
use miri::ast::{opt_expr, AssignmentOp, RangeExpressionType, StatementKind};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_forall_inferred_device_block_body() {
    let program = parse_program(
        "
forall i in 0..16
    x = i
",
    );
    assert_eq!(program.body.len(), 1);
    assert!(matches!(program.body[0].node, StatementKind::Forall { .. }));
}

#[test]
fn test_forall_inferred_device_inline_body() {
    let program = parse_program(
        "
forall i in 0..16: x = i
",
    );
    let expected = forall_statement(
        AcceleratorTarget::Inferred,
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
fn test_forall_inferred_device_empty_body() {
    let program = parse_program(
        "
forall i in 0..4: // nothing
",
    );
    let expected = forall_statement(
        AcceleratorTarget::Inferred,
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
fn test_gpu_forall_explicit_device_block_body() {
    let program = parse_program(
        "
gpu forall i in 0..16
    x = i
",
    );
    assert_eq!(program.body.len(), 1);
    assert!(matches!(
        program.body[0].node,
        StatementKind::Forall {
            device: AcceleratorTarget::Gpu,
            ..
        }
    ));
}

#[test]
fn test_gpu_forall_explicit_device_inline_body() {
    let program = parse_program(
        "
gpu forall i in 0..16: x = i
",
    );
    let expected = forall_statement(
        AcceleratorTarget::Gpu,
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
fn test_gpu_forall_explicit_device_empty_body() {
    let program = parse_program(
        "
gpu forall i in 0..4: // nothing
",
    );
    let expected = forall_statement(
        AcceleratorTarget::Gpu,
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
fn test_gpu_for_is_rejected() {
    parser_error_test(
        "
gpu for i in 0..3
    x = i
",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "gpu for".to_string(),
            reason: "unexpected 'for' after 'gpu'; use 'forall' or 'gpu forall'".to_string(),
        },
    );
}

#[test]
fn test_forall_requires_in_keyword() {
    parser_error_test(
        "
forall i 0..3
    x = i
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "int".to_string(),
        },
    );
}

#[test]
fn test_forall_still_compatible_with_gpu_fn() {
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
