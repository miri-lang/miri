// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression lowering - converts AST expressions to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
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
        if matches!(&obj_ty.kind, TypeKind::Map(_, _))
            || matches!(&obj_ty.kind, TypeKind::Custom(name, _) if name == "Map")
        {
            return lower_map_index_read(ctx, expr, obj, index_expr, dest);
        }
    }

    // Lower object to get a place
    let obj_operand = lower_expression(ctx, obj, None)?;

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
    let mut indexed_place = obj_place;
    indexed_place.projection.push(PlaceElem::Index(index_local));

    if let Some(d) = dest {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(d.clone(), Rvalue::Use(Operand::Copy(indexed_place))),
            span: expr.span,
        });
        Ok(Operand::Copy(d))
    } else {
        Ok(Operand::Copy(indexed_place))
    }
}

/// Lowers `map[key]` to a `miri_rt_map_get(map, key)` runtime call.
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
        literal: crate::ast::literal::Literal::Identifier("miri_rt_map_get".to_string()),
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
