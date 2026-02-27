// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    AggregateKind, Constant, Operand, Place, Rvalue, StatementKind as MirStatementKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_range_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Range(start_expr, end_expr_opt, range_type) = &expr.node else {
        unreachable!()
    };
    // Lower range expression to a tuple aggregate (start, end, is_inclusive)
    // This provides enough info for backends to iterate the range

    let start_op = lower_expression(ctx, start_expr, None)?;

    // End value - if not provided, create a "max" sentinel or just use start
    let end_op = if let Some(end_expr) = end_expr_opt {
        lower_expression(ctx, end_expr, None)?
    } else {
        // Range with no end (used for iterable objects) - use start as placeholder
        start_op.clone()
    };

    // is_inclusive flag
    let is_inclusive = matches!(
        range_type,
        crate::ast::expression::RangeExpressionType::Inclusive
    );
    let inclusive_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Boolean, expr.span),
        literal: crate::ast::literal::Literal::Boolean(is_inclusive),
    }));

    // Create tuple aggregate (start, end, is_inclusive)
    let (target, ret_op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let ty = resolve_type(ctx.type_checker, expr);
        let temp = ctx.push_temp(ty, expr.span);
        (Place::new(temp), Operand::Copy(Place::new(temp)))
    };
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target,
            Rvalue::Aggregate(AggregateKind::Tuple, vec![start_op, end_op, inclusive_op]),
        ),
        span: expr.span,
    });
    Ok(ret_op)
}
