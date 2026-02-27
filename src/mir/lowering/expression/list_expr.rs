// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{AggregateKind, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_list_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::List(elements) = &expr.node else {
        unreachable!()
    };
    let ops: Vec<Operand> = elements
        .iter()
        .map(|e| lower_expression(ctx, e, None))
        .collect::<Result<_, _>>()?;
    let ty = resolve_type(ctx.type_checker, expr);
    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Aggregate(AggregateKind::List, ops)),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(ty, expr.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(temp),
                Rvalue::Aggregate(AggregateKind::List, ops),
            ),
            span: expr.span,
        });
        Ok(Operand::Copy(Place::new(temp)))
    }
}
