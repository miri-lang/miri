// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::{
    Constant, Operand, Place, PlaceElem, Rvalue, StatementKind as MirStatementKind, Terminator,
    TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::lowering::helpers::ensure_place;

pub(crate) fn lower_index_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Index(obj, index_expr) = &expr.node else {
        unreachable!()
    };

    // Check if this is a map index access — dispatch to runtime call
    if let Some(obj_ty) = ctx.type_checker.get_type(obj.id) {
        if obj_ty.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map) {
            return lower_map_index_read(ctx, expr, obj, index_expr, dest);
        }
    }

    // Lower object to get a place. Record a watermark so we can release any
    // managed temp created just for this subexpression (e.g. an array literal).
    let obj_watermark = ctx.body.local_decls.len();
    let obj_operand = lower_expression(ctx, obj, None)?;
    // Only drop if we got a Copy (Perceus will have IncRef'd the source temp).
    let obj_op_is_copy = matches!(&obj_operand, Operand::Copy(_));

    let obj_place = ensure_place(ctx, obj_operand, obj.span);

    // Lower index expression
    let index_operand = lower_expression(ctx, index_expr, None)?;

    // Ensure index is in a local (PlaceElem::Index requires Local)
    let index_local = match index_operand {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => {
            // Store in temp - use inferred type from type checker or default to Int
            let ty = ctx
                .type_checker
                .get_type(index_expr.id)
                .cloned()
                .unwrap_or_else(|| Type::new(TypeKind::Int, index_expr.span));
            let temp = ctx.push_temp(ty, index_expr.span);
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(index_operand)),
                span: index_expr.span,
            });
            temp
        }
    };

    // Create indexed place with projection
    let mut indexed_place = obj_place.clone();
    indexed_place.projection.push(PlaceElem::Index(index_local));

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(Operand::Copy(indexed_place))),
            span: expr.span,
        });
        // Release the collection temp only if it was a Copy (Perceus IncRef'd it).
        if obj_op_is_copy {
            ctx.emit_temp_drop(obj_place.local, obj_watermark, expr.span);
        }
        Ok(Operand::Copy(d))
    } else if obj_op_is_copy {
        // obj was a temporary (Copy-returned) that we IncRef'd. Materialize the
        // element value into its own temp first, then release the collection temp.
        // This ensures the collection is freed AFTER the element has been read.
        let elem_ty = ctx
            .type_checker
            .get_type(expr.id)
            .cloned()
            .unwrap_or_else(|| Type::new(TypeKind::Int, expr.span));
        let elem_temp = ctx.push_temp(elem_ty, expr.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(elem_temp),
                Rvalue::Use(Operand::Copy(indexed_place)),
            ),
            span: expr.span,
        });
        ctx.emit_temp_drop(obj_place.local, obj_watermark, expr.span);
        Ok(Operand::Copy(Place::new(elem_temp)))
    } else {
        // obj was accessed via Move — no IncRef, no drop needed. Return the projected place.
        Ok(Operand::Copy(indexed_place))
    }
}

/// Lowers `map[key]` to a `miri_rt_map_get_checked(map, key)` runtime call.
///
/// Uses the checked variant that aborts on missing key, consistent with
/// array out-of-bounds behavior. For safe access, use `m.get(key)` which
/// returns `V?`.
fn lower_map_index_read(
    ctx: &mut LoweringContext,
    expr: &Expression,
    obj: &Expression,
    key_expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_op = lower_expression(ctx, obj, None)?;
    let key_op = lower_expression(ctx, key_expr, None)?;

    let func_op = Operand::Constant(Box::new(Constant {
        span: expr.span,
        ty: Type::new(TypeKind::Identifier, expr.span),
        literal: crate::ast::literal::Literal::Identifier("miri_rt_map_get_checked".to_string()),
    }));

    let result_ty = if let Some(t) = ctx.type_checker.get_type(expr.id) {
        t.clone()
    } else {
        Type::new(TypeKind::Int, expr.span)
    };

    let (destination, op) = if let Some(d) = dest {
        (d.clone(), Operand::Copy(d))
    } else {
        let temp = ctx.push_temp(result_ty, expr.span);
        let p = Place::new(temp);
        (p.clone(), Operand::Copy(p))
    };

    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, key_op],
            destination,
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);

    Ok(op)
}
