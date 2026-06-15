// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parse_program, parser_error_test};
use miri::ast::factory::{
    assign, empty_statement, expression_statement, forall_statement, identifier,
    int_literal_expression, let_variable, lhs_identifier, range, tuple_with_span,
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

#[test]
fn test_forall_2d_literal_ranges() {
    let program = parse_program(
        "
forall x, y in 0..2, 0..3: z = x
",
    );
    let expected = forall_statement(
        AcceleratorTarget::Inferred,
        vec![let_variable("x", None, None), let_variable("y", None, None)],
        tuple_with_span(
            vec![
                range(
                    int_literal_expression(0),
                    opt_expr(int_literal_expression(2)),
                    RangeExpressionType::Exclusive,
                ),
                range(
                    int_literal_expression(0),
                    opt_expr(int_literal_expression(3)),
                    RangeExpressionType::Exclusive,
                ),
            ],
            program.body[0].span,
        ),
        expression_statement(assign(
            lhs_identifier("z"),
            AssignmentOp::Assign,
            identifier("x"),
        )),
    );
    assert_eq!(program.body[0], expected);
}

#[test]
fn test_forall_3d_literal_ranges() {
    let program = parse_program(
        "
forall x, y, z in 0..2, 0..3, 0..4: a = x
",
    );
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Forall { vars, iterable, .. } = &program.body[0].node {
        assert_eq!(vars.len(), 3);
        if let miri::ast::ExpressionKind::Tuple(elements) = &iterable.node {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("Expected Tuple iterable for 3D forall");
        }
    } else {
        panic!("Expected Forall statement");
    }
}

#[test]
fn test_gpu_forall_3d_variable_bounds() {
    let program = parse_program(
        "
gpu forall x, y, z in 0..a, 0..b, 0..c: d = x
",
    );
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Forall {
        device: AcceleratorTarget::Gpu,
        vars,
        iterable,
        ..
    } = &program.body[0].node
    {
        assert_eq!(vars.len(), 3);
        if let miri::ast::ExpressionKind::Tuple(elements) = &iterable.node {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("Expected Tuple iterable for 3D gpu forall");
        }
    } else {
        panic!("Expected gpu Forall statement");
    }
}

#[test]
fn test_forall_mixed_variable_literal_bounds() {
    let program = parse_program(
        "
forall x, y in 0..n, lo..hi: z = x
",
    );
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Forall { vars, iterable, .. } = &program.body[0].node {
        assert_eq!(vars.len(), 2);
        if let miri::ast::ExpressionKind::Tuple(elements) = &iterable.node {
            assert_eq!(elements.len(), 2);
        } else {
            panic!("Expected Tuple iterable for mixed bounds");
        }
    } else {
        panic!("Expected Forall statement");
    }
}

#[test]
fn test_forall_4_vars_rejected() {
    parser_error_test(
        "
forall a, b, c, d in 0..1, 0..1, 0..1, 0..1: x = a
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "at most 3 loop variables".to_string(),
            found: "4 variables".to_string(),
        },
    );
}

#[test]
fn test_forall_3d_missing_third_range() {
    parser_error_test(
        "
forall x, y, z in 0..2, 0..3: w = x
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "3D forall requires 3 comma-separated ranges".to_string(),
            found: ":".to_string(),
        },
    );
}
