// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{AggregateKind, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;

pub(crate) fn lower_map_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Map(pairs) = &expr.node else {
        unreachable!()
    };
    // Record watermark before lowering elements so we can release managed temps
    // created for sub-expressions (e.g. nested map or list values).
    let elem_watermark = ctx.body.local_decls.len();
    // Flatten pairs into [key1, val1, key2, val2, ...]
    let mut ops: Vec<Operand> = Vec::with_capacity(pairs.len() * 2);
    for (key, val) in pairs {
        ops.push(lower_expression(ctx, key, None)?);
        ops.push(lower_expression(ctx, val, None)?);
    }
    let ty = resolve_type(ctx.type_checker, expr);
    let result = if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Aggregate(AggregateKind::Map, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(d)
    } else {
        let temp = ctx.push_temp(ty, expr.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(temp),
                Rvalue::Aggregate(AggregateKind::Map, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(Place::new(temp))
    };

    // Release managed element temps — the Aggregate IncRef transferred ownership
    // into the map, so the original temp references are no longer needed.
    for op in &ops {
        if let Operand::Copy(p) = op {
            ctx.emit_temp_drop(p.local, elem_watermark, expr.span);
        }
    }

    Ok(result)
}
