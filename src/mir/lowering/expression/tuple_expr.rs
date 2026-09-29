// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::{AggregateKind, Operand, Place, Rvalue, StatementKind as MirStatementKind};

use crate::ast::types::{Type, TypeKind};
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::coerce_arg_to_declared;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::resolve_type;
use crate::mir::lowering::variable::canonical_declared_type;

pub(crate) fn lower_tuple_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Tuple(elements) = &expr.node else {
        unreachable!()
    };
    // Record watermark before lowering elements so we can release any managed
    // temps created for sub-expressions (e.g. anonymous nested collections).
    let elem_watermark = ctx.body.local_decls.len();
    let ty = resolve_type(ctx.type_checker, expr);
    // Each element is stored at the element type the literal was recorded at:
    // a plain `7` in a tuple built as `(int?, String)` is boxed as `Some(7)`.
    let element_types: Vec<Type> = if let TypeKind::Tuple(declared) = &ty.kind {
        declared
            .iter()
            .map(|element| canonical_declared_type(ctx.type_checker, &ctx.resolved_type(element)))
            .collect()
    } else {
        Vec::new()
    };
    let mut ops: Vec<Operand> = Vec::with_capacity(elements.len());
    for (position, element) in elements.iter().enumerate() {
        let watermark = ctx.body.local_decls.len();
        let op = lower_expression(ctx, element, None)?;
        ops.push(match element_types.get(position) {
            Some(target) => coerce_arg_to_declared(ctx, op, element, target, watermark),
            None => op,
        });
    }

    let result = if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                d.clone(),
                Rvalue::Aggregate(AggregateKind::Tuple, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(d)
    } else {
        let temp = ctx.push_temp(ty, expr.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(temp),
                Rvalue::Aggregate(AggregateKind::Tuple, ops.clone()),
            ),
            span: expr.span,
        });
        Operand::Copy(Place::new(temp))
    };

    // Release managed element temps — the Aggregate IncRef transferred ownership
    // into the tuple, so the original temp references are no longer needed.
    for op in &ops {
        if let Operand::Copy(p) = op {
            ctx.emit_temp_drop(p.local, elem_watermark, expr.span);
        }
    }

    Ok(result)
}
