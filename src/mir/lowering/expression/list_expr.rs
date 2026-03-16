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
    // Record watermark before lowering elements so we can release any managed
    // temps created for sub-expressions (e.g. anonymous nested lists/arrays).
    let elem_watermark = ctx.body.local_decls.len();
    let ops: Vec<Operand> = elements
        .iter()
        .map(|e| lower_expression(ctx, e, None))
        .collect::<Result<_, _>>()?;
    let ty = resolve_type(ctx.type_checker, expr);

    let result = if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Aggregate(AggregateKind::List, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(d)
    } else {
        let temp = ctx.push_temp(ty, expr.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(temp),
                Rvalue::Aggregate(AggregateKind::List, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(Place::new(temp))
    };

    // Release managed element temps — the Aggregate IncRef transferred ownership
    // into the list, so the original temp references are no longer needed.
    for op in &ops {
        if let Operand::Copy(p) = op {
            ctx.emit_temp_drop(p.local, elem_watermark, expr.span);
        }
    }

    Ok(result)
}
