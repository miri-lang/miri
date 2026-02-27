// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{BinOp, Operand, Place, Rvalue, StatementKind as MirStatementKind, UnOp};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;

pub(crate) fn lower_guard_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Guard(guard_op, guard_expr) = &expr.node else {
        unreachable!()
    };
    // Guard expressions are used in function parameter validation
    // e.g., fn divide(a int, b int > 0) - the `> 0` is a guard
    // We lower guards to comparison operations that return bool

    let operand = lower_expression(ctx, guard_expr, None)?;

    // Convert GuardOp to BinOp
    let _bin_op = match guard_op {
        crate::ast::operator::GuardOp::GreaterThan => BinOp::Gt,
        crate::ast::operator::GuardOp::GreaterThanEqual => BinOp::Ge,
        crate::ast::operator::GuardOp::LessThan => BinOp::Lt,
        crate::ast::operator::GuardOp::LessThanEqual => BinOp::Le,
        crate::ast::operator::GuardOp::NotEqual => BinOp::Ne,
        crate::ast::operator::GuardOp::Not => {
            // Not is a unary op, apply directly
            let result_ty = Type::new(TypeKind::Boolean, expr.span);
            let temp = ctx.push_temp(result_ty, expr.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(
                    Place::new(temp),
                    Rvalue::UnaryOp(UnOp::Not, Box::new(operand)),
                ),
                span: expr.span,
            });
            return Ok(Operand::Copy(Place::new(temp)));
        }
        crate::ast::operator::GuardOp::In | crate::ast::operator::GuardOp::NotIn => {
            // In/NotIn guards require membership test - for now create placeholder
            return Ok(operand);
        }
    };

    // Guards already have their RHS value baked in from parsing
    // The operand IS the guard expression (e.g., the `0` in `> 0`)
    // The LHS (the parameter) would need to be provided by the caller
    // For now, just return the guard expression value
    Ok(operand)
}
