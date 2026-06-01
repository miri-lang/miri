// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    BinOp, Constant, Discriminant, Operand, Place, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;

/// Emit short-circuit AND branching and short-circuit assignment.
fn emit_and_short_circuit(
    ctx: &mut LoweringContext,
    result_local: crate::mir::Local,
    result_ty: &Type,
    rhs_bb: crate::mir::BasicBlock,
    done_bb: crate::mir::BasicBlock,
    expr: &Expression,
) {
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(result_local)),
            targets: vec![(Discriminant::bool_true(), rhs_bb)],
            otherwise: done_bb,
        },
        expr.span,
    ));

    ctx.set_current_block(done_bb);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty: result_ty.clone(),
                literal: crate::ast::literal::Literal::Boolean(false),
            }))),
        ),
        span: expr.span,
    });
}

/// Emit short-circuit OR branching and short-circuit assignment.
fn emit_or_short_circuit(
    ctx: &mut LoweringContext,
    result_local: crate::mir::Local,
    result_ty: &Type,
    rhs_bb: crate::mir::BasicBlock,
    done_bb: crate::mir::BasicBlock,
    expr: &Expression,
) {
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(result_local)),
            targets: vec![(Discriminant::bool_false(), rhs_bb)],
            otherwise: done_bb,
        },
        expr.span,
    ));

    ctx.set_current_block(done_bb);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: expr.span,
                ty: result_ty.clone(),
                literal: crate::ast::literal::Literal::Boolean(true),
            }))),
        ),
        span: expr.span,
    });
}

pub(crate) fn lower_logical_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Logical(lhs, op, rhs) = &expr.node else {
        unreachable!()
    };
    if matches!(op, crate::ast::operator::BinaryOp::NullCoalesce) {
        return lower_null_coalesce(ctx, expr, lhs, rhs, dest);
    }

    // Short-circuit: `and` skips rhs when lhs is false; `or` when lhs is true.
    let result_ty = Type::new(TypeKind::Boolean, expr.span);
    let result_local = ctx.push_temp(result_ty.clone(), expr.span);
    let lhs_op = lower_expression(ctx, lhs, None)?;

    let rhs_bb = ctx.new_basic_block();
    let done_bb = ctx.new_basic_block();
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(lhs_op)),
        span: expr.span,
    });
    emit_logical_short_circuit(ctx, op, result_local, &result_ty, rhs_bb, done_bb, expr)?;

    let final_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        expr.span,
    ));
    emit_logical_rhs(ctx, rhs, result_local, rhs_bb, final_bb, expr)?;

    ctx.set_current_block(final_bb);
    Ok(finish_logical_result(ctx, result_local, dest, expr))
}

/// Dispatch to the `and`/`or` short-circuit emitter for `op`.
fn emit_logical_short_circuit(
    ctx: &mut LoweringContext,
    op: &crate::ast::operator::BinaryOp,
    result_local: crate::mir::Local,
    result_ty: &Type,
    rhs_bb: crate::mir::BasicBlock,
    done_bb: crate::mir::BasicBlock,
    expr: &Expression,
) -> Result<(), LoweringError> {
    match op {
        crate::ast::operator::BinaryOp::And => {
            emit_and_short_circuit(ctx, result_local, result_ty, rhs_bb, done_bb, expr)
        }
        crate::ast::operator::BinaryOp::Or => {
            emit_or_short_circuit(ctx, result_local, result_ty, rhs_bb, done_bb, expr)
        }
        _ => {
            return Err(LoweringError::unsupported_operator(
                format!("{:?}", op),
                expr.span,
            ))
        }
    }
    Ok(())
}

/// Evaluate the RHS in `rhs_bb`, store it into `result_local`, and goto join.
fn emit_logical_rhs(
    ctx: &mut LoweringContext,
    rhs: &Expression,
    result_local: crate::mir::Local,
    rhs_bb: crate::mir::BasicBlock,
    final_bb: crate::mir::BasicBlock,
    expr: &Expression,
) -> Result<(), LoweringError> {
    ctx.set_current_block(rhs_bb);
    let rhs_op = lower_expression(ctx, rhs, None)?;
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(result_local), Rvalue::Use(rhs_op)),
        span: expr.span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        expr.span,
    ));
    Ok(())
}

/// Return the boolean result, copying into `dest` (DPS) when provided so that
/// `let v = a and b` populates `v` rather than leaving its zero default.
fn finish_logical_result(
    ctx: &mut LoweringContext,
    result_local: crate::mir::Local,
    dest: Option<Place>,
    expr: &Expression,
) -> Operand {
    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(result_local))),
            ),
            span: expr.span,
        });
        return Operand::Copy(d);
    }
    Operand::Copy(Place::new(result_local))
}

/// Lowers the `??` (null coalescing) operator.
///
/// Pattern: evaluate LHS, compare with None. If None → evaluate RHS.
/// If Some → use LHS directly (Option<T> == T at runtime).
fn lower_null_coalesce(
    ctx: &mut LoweringContext,
    expr: &Expression,
    lhs: &Expression,
    rhs: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let inner_ty = coalesce_inner_type(ctx, lhs, expr);
    let result_local = ctx.push_temp(inner_ty.clone(), expr.span);
    let lhs_op = lower_expression(ctx, lhs, None)?;
    let is_none_local = emit_none_comparison(ctx, &lhs_op, &inner_ty, expr);

    let rhs_bb = ctx.new_basic_block();
    let some_bb = ctx.new_basic_block();
    let final_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(is_none_local)),
            targets: vec![(Discriminant::bool_true(), rhs_bb)], // None → rhs
            otherwise: some_bb,                                 // Some → use lhs
        },
        expr.span,
    ));

    emit_coalesce_some_branch(ctx, lhs_op, result_local, some_bb, final_bb, expr);
    emit_logical_rhs(ctx, rhs, result_local, rhs_bb, final_bb, expr)?;

    ctx.set_current_block(final_bb);
    Ok(finish_logical_result(ctx, result_local, dest, expr))
}

/// The inner type `T` of `Option<T>` for the coalesce result temp; the lhs type
/// itself if it is not an Option, else `Int` when untyped.
fn coalesce_inner_type(ctx: &LoweringContext, lhs: &Expression, expr: &Expression) -> Type {
    let Some(ty) = ctx.type_checker.get_type(lhs.id).cloned() else {
        return Type::new(TypeKind::Int, expr.span);
    };
    match &ty.kind {
        TypeKind::Option(inner) => inner.as_ref().clone(),
        _ => ty,
    }
}

/// Emit `lhs == None` and return the boolean result local.
fn emit_none_comparison(
    ctx: &mut LoweringContext,
    lhs_op: &Operand,
    inner_ty: &Type,
    expr: &Expression,
) -> crate::mir::Local {
    let none_val = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: inner_ty.clone(),
        literal: crate::ast::literal::Literal::None,
    }));
    let is_none_local = ctx.push_temp(Type::new(TypeKind::Boolean, expr.span), expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(is_none_local),
            Rvalue::BinaryOp(BinOp::Eq, Box::new(lhs_op.clone()), Box::new(none_val)),
        ),
        span: expr.span,
    });
    is_none_local
}

/// Some branch: project the Option payload (`Field(0)`) into `result_local`.
fn emit_coalesce_some_branch(
    ctx: &mut LoweringContext,
    lhs_op: Operand,
    result_local: crate::mir::Local,
    some_bb: crate::mir::BasicBlock,
    final_bb: crate::mir::BasicBlock,
    expr: &Expression,
) {
    ctx.set_current_block(some_bb);
    let mut payload_place = crate::mir::lowering::helpers::ensure_place(ctx, lhs_op, expr.span);
    payload_place
        .projection
        .push(crate::mir::PlaceElem::Field(0));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(result_local),
            Rvalue::Use(Operand::Copy(payload_place)),
        ),
        span: expr.span,
    });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        expr.span,
    ));
}
