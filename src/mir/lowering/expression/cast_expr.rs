// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::ExpressionKind;
use crate::ast::Expression;
use crate::error::lowering::LoweringError;
use crate::mir::{Operand, Place, Rvalue};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::resolve_type;

pub(super) fn lower_cast_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let ExpressionKind::Cast(value_expr, _target_type_expr) = &expr.node {
        let value_operand = super::lower_expression(ctx, value_expr, None)?;

        let target_type = resolve_type(ctx.type_checker, expr);

        let (target, ret_op) = if let Some(d) = dest {
            (d.clone(), Operand::Copy(d))
        } else {
            let temp = ctx.push_temp(target_type.clone(), expr.span);
            (Place::new(temp), Operand::Copy(Place::new(temp)))
        };

        ctx.push_statement(crate::mir::Statement {
            kind: crate::mir::StatementKind::Assign(
                target,
                Rvalue::Cast(Box::new(value_operand), target_type),
            ),
            span: expr.span,
        });

        Ok(ret_op)
    } else {
        Err(LoweringError::unsupported_expression(
            "expected cast expression".to_string(),
            expr.span,
        ))
    }
}
