// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{AggregateKind, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::{coerce_arg_to_declared, collection_slot_type, ELEMENT_SLOT};
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;
use crate::mir::lowering::variable::canonical_declared_type;

pub(crate) fn lower_array_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Array(elements, _size) = &expr.node else {
        unreachable!()
    };
    // Record watermark before lowering elements so we can release any managed
    // temps created for sub-expressions (e.g. anonymous nested arrays).
    let elem_watermark = ctx.body.local_decls.len();
    let ty = resolve_type(ctx.type_checker, expr);
    // Each element is stored at the element type the literal was recorded at:
    // a plain `1` in an `Array<int?, 2>` is boxed as `Some(1)`, or the array
    // would hold a bare word where its readers expect an optional.
    let element_ty = collection_slot_type(ctx, &ty, ELEMENT_SLOT)
        .map(|element| canonical_declared_type(ctx.type_checker, &element));
    let mut ops: Vec<Operand> = Vec::with_capacity(elements.len());
    for element in elements {
        let watermark = ctx.body.local_decls.len();
        let op = lower_expression(ctx, element, None)?;
        ops.push(match &element_ty {
            Some(target) => coerce_arg_to_declared(ctx, op, element, target, watermark),
            None => op,
        });
    }

    let result = if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Aggregate(AggregateKind::Array, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(d)
    } else {
        let temp = ctx.push_temp(ty, expr.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(temp),
                Rvalue::Aggregate(AggregateKind::Array, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(Place::new(temp))
    };

    // Release managed element temps — the Aggregate IncRef transferred ownership
    // into the array, so the original temp references are no longer needed.
    for op in &ops {
        if let Operand::Copy(p) = op {
            ctx.emit_temp_drop(p.local, elem_watermark, expr.span);
        }
    }

    Ok(result)
}
