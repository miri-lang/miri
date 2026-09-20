// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::Type;
use crate::error::lowering::LoweringError;
use crate::mir::{Operand, Place, Rvalue, StatementKind as MirStatementKind, UnOp};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

/// Put `value` where the caller asked for it, and name it the way the caller
/// will read it.
///
/// An operator that answers its operand unchanged still has to honour a
/// destination: a caller that supplied one reads that place afterwards, so
/// handing back the operand without writing it leaves the place holding
/// whatever it was initialised with. With no destination the value is already
/// the answer.
fn into_destination(
    ctx: &mut LoweringContext,
    value: Operand,
    dest: Option<Place>,
    expr: &Expression,
) -> Operand {
    let Some(place) = dest else {
        return value;
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(place.clone(), Rvalue::Use(value)),
        span: expr.span,
    });
    Operand::Copy(place)
}

/// The type an instantiated body gives `expr`, for the temp a unary operator
/// writes its result into.
///
/// The type checker records the type of `-a` in a generic body once, against
/// the parameter. Read raw inside a body instantiated at `float`, the temp
/// still carries the parameter, which code generation resolves to the
/// pointer-width integer fallback: the negated float is truncated on its way
/// into the temp, and a later read of that temp mixes widths. Outside an
/// instantiated body the substitution is empty and this is the recorded type
/// unchanged. `binary_expr::binary_result_type` keeps the same contract for
/// the binary operators.
fn instantiated_type(ctx: &LoweringContext, expr: &Expression) -> Type {
    crate::mir::lowering::apply_generic_sub(
        &resolve_type(ctx.type_checker, expr),
        &ctx.generic_subs,
    )
}

/// Lower `--x` as `-(-x)`. The type-checker's resolved type is used for the
/// temps so projected operands (e.g. `self.field`) keep their scalar width.
fn lower_double_negate(
    ctx: &mut LoweringContext,
    op_val: Operand,
    operand: &Expression,
    expr: &Expression,
) -> Operand {
    let first_neg_ty = instantiated_type(ctx, operand);
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

/// The constant a negated signed integer literal denotes, so the sign is
/// applied here rather than by an instruction at run time.
///
/// A literal's sign is known while compiling, and performing it later performs
/// it at the operand's width: the 128-bit widths have no negation instruction to
/// perform it with, and a narrower one would have to hold a magnitude one past
/// its own maximum to negate `MIN`. Folding avoids both.
///
/// `None` leaves the general path alone — for an unsigned target, whose wrapping
/// is the existing behaviour, and for a magnitude with no negation (`i128::MIN`
/// spelled positive).
fn fold_negated_int_literal(
    ctx: &mut LoweringContext,
    expr: &Expression,
    operand: &Expression,
) -> Option<Operand> {
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::ast::types::TypeKind;

    let ExpressionKind::Literal(Literal::Integer(int_lit)) = &operand.node else {
        return None;
    };
    let ty = resolve_type(ctx.type_checker, expr);
    if !matches!(
        ty.kind,
        TypeKind::Int | TypeKind::I8 | TypeKind::I16 | TypeKind::I32 | TypeKind::I64
    ) && ty.kind != TypeKind::I128
    {
        return None;
    }
    let negated = int_lit.to_i128().checked_neg()?;
    let literal = IntegerLiteral::from_type_kind(&ty.kind, negated)?;
    Some(Operand::Constant(Box::new(crate::mir::Constant {
        span: expr.span,
        ty,
        literal: Literal::Integer(literal),
    })))
}

pub(crate) fn lower_unary_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Unary(op, operand) = &expr.node else {
        unreachable!()
    };

    if matches!(op, crate::ast::operator::UnaryOp::Negate) {
        if let Some(folded) = fold_negated_int_literal(ctx, expr, operand) {
            return Ok(match dest {
                Some(d) => {
                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(folded)),
                        span: expr.span,
                    });
                    Operand::Copy(d)
                }
                None => folded,
            });
        }
    }

    let op_val = lower_expression(ctx, operand, None)?;
    let un_op = match op {
        crate::ast::operator::UnaryOp::Negate => UnOp::Neg,
        crate::ast::operator::UnaryOp::Not => UnOp::Not,
        crate::ast::operator::UnaryOp::Await => UnOp::Await,
        // Decrement (--x) is treated as double negation: -(-x) = x.
        crate::ast::operator::UnaryOp::Decrement => {
            let doubly_negated = lower_double_negate(ctx, op_val, operand, expr);
            return Ok(into_destination(ctx, doubly_negated, dest, expr));
        }
        // Increment (++x) is a no-op for value (not implemented as mutation)
        crate::ast::operator::UnaryOp::Increment => {
            return Ok(into_destination(ctx, op_val, dest, expr));
        }
        // Plus is identity
        crate::ast::operator::UnaryOp::Plus => {
            return Ok(into_destination(ctx, op_val, dest, expr));
        }
        crate::ast::operator::UnaryOp::BitwiseNot => UnOp::BitwiseNot,
    };

    // Use the type-checker's resolved type for the unary expression, read
    // through the active instantiation. Reading the base local's type would
    // lose projections (e.g. `-self.field` would yield the class type rather
    // than the field's scalar type), causing Perceus to mis-type the result
    // temp.
    let result_ty = instantiated_type(ctx, expr);

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
