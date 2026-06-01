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
use crate::runtime_fns::rt;

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

    // Map index access dispatches to a runtime call.
    if is_map_index(ctx, obj) {
        return lower_map_index_read(ctx, expr, obj, index_expr, dest);
    }

    // Watermark so we can release any managed temp created for `obj` (e.g. an
    // array literal). Only drop on Copy — Perceus IncRef's the source temp.
    let obj_watermark = ctx.body.local_decls.len();
    let obj_operand = lower_expression(ctx, obj, None)?;
    let obj_op_is_copy = matches!(&obj_operand, Operand::Copy(_));
    let obj_place = ensure_place(ctx, obj_operand, obj.span);

    let index_operand = lower_expression(ctx, index_expr, None)?;
    let index_local = ensure_index_local(ctx, index_expr, index_operand);

    let mut indexed_place = obj_place.clone();
    indexed_place.projection.push(PlaceElem::Index(index_local));

    finish_index_read(
        ctx,
        indexed_place,
        &obj_place,
        obj_op_is_copy,
        obj_watermark,
        expr,
        dest,
    )
}

/// True when `obj` is a `Map` (indexing it lowers to a runtime call).
fn is_map_index(ctx: &LoweringContext, obj: &Expression) -> bool {
    ctx.type_checker
        .get_type(obj.id)
        .map(|t| t.kind.as_builtin_collection() == Some(BuiltinCollectionKind::Map))
        .unwrap_or(false)
}

/// Materialize the index operand into a bare local (`PlaceElem::Index` requires
/// a `Local`), spilling to a temp when it is projected or a constant.
fn ensure_index_local(
    ctx: &mut LoweringContext,
    index_expr: &Expression,
    index_operand: Operand,
) -> crate::mir::Local {
    match index_operand {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => {
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
    }
}

/// Emit the indexed read into `dest` (or a temp), releasing the collection temp
/// after the element is read when `obj` was a Copy-returned temporary.
fn finish_index_read(
    ctx: &mut LoweringContext,
    indexed_place: Place,
    obj_place: &Place,
    obj_op_is_copy: bool,
    obj_watermark: usize,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(Operand::Copy(indexed_place))),
            span: expr.span,
        });
        if obj_op_is_copy {
            ctx.emit_temp_drop(obj_place.local, obj_watermark, expr.span);
        }
        Ok(Operand::Copy(d))
    } else if obj_op_is_copy {
        // Materialize the element first so the collection is freed AFTER the read.
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
        // obj accessed via Move — no IncRef, no drop. Return the projected place.
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
        literal: crate::ast::literal::Literal::Identifier(rt::MAP_GET_CHECKED.to_string()),
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
            out_args: Vec::new(),
            destination,
            target: Some(target_bb),
        },
        expr.span,
    ));
    ctx.set_current_block(target_bb);

    Ok(op)
}
