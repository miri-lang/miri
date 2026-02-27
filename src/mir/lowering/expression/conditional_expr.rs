// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    Discriminant, Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_conditional_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Conditional(then_expr, cond_expr, else_expr_opt, if_type) = &expr.node
    else {
        unreachable!()
    };
    // Inline if/unless expression: `value if condition else other`
    // then_expr is returned if condition is true (or false for unless)

    // Use dest if provided (DPS), otherwise create a temp
    let result_local = if let Some(ref dest_place) = dest {
        dest_place.local
    } else {
        let result_ty = resolve_type(ctx.type_checker, expr);
        ctx.push_temp(result_ty, expr.span)
    };

    // Evaluate condition first
    let cond_op = lower_expression(ctx, cond_expr, None)?;

    let then_bb = ctx.new_basic_block();
    let else_bb = ctx.new_basic_block();
    let join_bb = ctx.new_basic_block();

    // For `if`: true -> then, false -> else
    // For `unless`: true -> else, false -> then
    let (true_target, false_target) = match if_type {
        crate::ast::statement::IfStatementType::If => (then_bb, else_bb),
        crate::ast::statement::IfStatementType::Unless => (else_bb, then_bb),
    };

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: cond_op,
            targets: vec![(Discriminant::bool_true(), true_target)],
            otherwise: false_target,
        },
        cond_expr.span,
    ));

    // Then block
    ctx.set_current_block(then_bb);
    let then_op = lower_expression(ctx, then_expr, None)?;
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(then_op)),
        span: then_expr.span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        then_expr.span,
    ));

    // Else block
    ctx.set_current_block(else_bb);
    if let Some(else_expr) = else_expr_opt {
        let else_op = lower_expression(ctx, else_expr, None)?;
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(else_op)),
            span: else_expr.span,
        });
    }
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        expr.span,
    ));

    ctx.set_current_block(join_bb);
    Ok(Operand::Copy(Place::new(result_local)))
}
