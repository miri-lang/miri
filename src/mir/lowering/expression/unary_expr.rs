// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{Operand, Place, Rvalue, StatementKind as MirStatementKind, UnOp};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

/// Lower `--x` as `-(-x)`. The type-checker's resolved type is used for the
/// temps so projected operands (e.g. `self.field`) keep their scalar width.
fn lower_double_negate(
    ctx: &mut LoweringContext,
    op_val: Operand,
    operand: &Expression,
    expr: &Expression,
) -> Operand {
    let first_neg_ty = resolve_type(ctx.type_checker, operand);
    let first_neg = ctx.push_temp(first_neg_ty.clone(), expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(first_neg),
            Rvalue::UnaryOp(UnOp::Neg, Box::new(op_val)),
        ),
        span: expr.span,
    });

    let second_neg = ctx.push_temp(first_neg_ty, expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(second_neg),
            Rvalue::UnaryOp(UnOp::Neg, Box::new(Operand::Copy(Place::new(first_neg)))),
        ),
        span: expr.span,
    });
    Operand::Copy(Place::new(second_neg))
}

pub(crate) fn lower_unary_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Unary(op, operand) = &expr.node else {
        unreachable!()
    };
    let op_val = lower_expression(ctx, operand, None)?;
    let un_op = match op {
        crate::ast::operator::UnaryOp::Negate => UnOp::Neg,
        crate::ast::operator::UnaryOp::Not => UnOp::Not,
        crate::ast::operator::UnaryOp::Await => UnOp::Await,
        // Decrement (--x) is treated as double negation: -(-x) = x.
        crate::ast::operator::UnaryOp::Decrement => {
            return Ok(lower_double_negate(ctx, op_val, operand, expr));
        }
        // Increment (++x) is a no-op for value (not implemented as mutation)
        crate::ast::operator::UnaryOp::Increment => {
            return Ok(op_val);
        }
        // Plus is identity
        crate::ast::operator::UnaryOp::Plus => {
            return Ok(op_val);
        }
        // BitwiseNot - similar to Not
        crate::ast::operator::UnaryOp::BitwiseNot => UnOp::Not,
    };

    // Use the type-checker's resolved type for the unary expression.
    // Reading the base local's type would lose projections (e.g. `-self.field`
    // would yield the class type rather than the field's scalar type),
    // causing Perceus to mis-type the result temp.
    let result_ty = resolve_type(ctx.type_checker, expr);

    let (target, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(result_ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(target, Rvalue::UnaryOp(un_op, Box::new(op_val))),
        span: expr.span,
    });

    Ok(ret_op)
}
