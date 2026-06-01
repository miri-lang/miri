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

    emit_conditional_switch(ctx, cond_op, if_type, then_bb, else_bb, cond_expr.span);

    ctx.set_current_block(then_bb);
    emit_conditional_branch_value(ctx, then_expr, result_local)?;
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        then_expr.span,
    ));

    ctx.set_current_block(else_bb);
    if let Some(else_expr) = else_expr_opt {
        emit_conditional_branch_value(ctx, else_expr, result_local)?;
    }
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        expr.span,
    ));

    ctx.set_current_block(join_bb);
    Ok(Operand::Copy(Place::new(result_local)))
}

/// Emit the `SwitchInt` selecting the then/else block. For `unless` the
/// true/false targets are swapped.
fn emit_conditional_switch(
    ctx: &mut LoweringContext,
    cond_op: Operand,
    if_type: &crate::ast::statement::IfStatementType,
    then_bb: crate::mir::BasicBlock,
    else_bb: crate::mir::BasicBlock,
    cond_span: crate::error::syntax::Span,
) {
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
        cond_span,
    ));
}

/// Lower a conditional branch value into `result_local`, dropping any fresh
/// managed temp created (e.g. an inline `List([..])`) to balance Perceus's
/// IncRef on the Copy. Does not emit the branch's terminator.
fn emit_conditional_branch_value(
    ctx: &mut LoweringContext,
    branch_expr: &Expression,
    result_local: crate::mir::Local,
) -> Result<(), LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let op = lower_expression(ctx, branch_expr, None)?;
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(op.clone())),
        span: branch_expr.span,
    });
    if let Operand::Copy(p) = &op {
        ctx.emit_temp_drop(p.local, watermark, branch_expr.span);
    }
    Ok(())
}
