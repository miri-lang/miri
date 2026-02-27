// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    Constant, Discriminant, Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;

pub(crate) fn lower_logical_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Logical(lhs, op, rhs) = &expr.node else {
        unreachable!()
    };
    // Short-circuit evaluation for logical operators:
    // - and: if lhs is false, skip rhs and return false
    // - or: if lhs is true, skip rhs and return true

    let result_ty = Type::new(TypeKind::Boolean, expr.span);
    let result_local = ctx.push_temp(result_ty.clone(), expr.span);

    // Evaluate LHS
    let lhs_op = lower_expression(ctx, lhs, None)?;

    // Create blocks for short-circuit evaluation
    let rhs_bb = ctx.new_basic_block();
    let done_bb = ctx.new_basic_block();

    match op {
        crate::ast::operator::BinaryOp::And => {
            // and: if lhs is true, evaluate rhs; else return false
            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: lhs_op.clone(),
                    targets: vec![(Discriminant::bool_true(), rhs_bb)], // true -> evaluate rhs
                    otherwise: done_bb,                                 // false -> done with false
                },
                expr.span,
            ));

            // In done_bb after short-circuit (lhs was false), assign false
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
        crate::ast::operator::BinaryOp::Or => {
            // or: if lhs is false, evaluate rhs; else return true
            ctx.set_terminator(Terminator::new(
                TerminatorKind::SwitchInt {
                    discr: lhs_op.clone(),
                    targets: vec![(Discriminant::bool_false(), rhs_bb)], // false -> evaluate rhs
                    otherwise: done_bb,                                  // true -> done with true
                },
                expr.span,
            ));

            // In done_bb after short-circuit (lhs was true), assign true
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
        _ => {
            return Err(LoweringError::unsupported_operator(
                format!("{:?}", op),
                expr.span,
            ));
        }
    }

    // Create final join block
    let final_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: final_bb },
        expr.span,
    ));

    // Evaluate RHS in rhs_bb
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

    ctx.set_current_block(final_bb);

    // DPS: if a destination was provided (e.g. the variable being initialised in
    // `let var = a and b`), write the result into it so the caller's variable is
    // populated.  Without this the Logical arm ignores `dest` and the variable
    // stays at its zero-initialised default (false).
    if let Some(ref d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Use(Operand::Copy(Place::new(result_local))),
            ),
            span: expr.span,
        });
        return Ok(Operand::Copy(d.clone()));
    }

    Ok(Operand::Copy(Place::new(result_local)))
}
