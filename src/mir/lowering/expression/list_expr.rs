// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of a list literal: the empty list its type names, then one
//! `miri_rt_list_push` per element, the way `list.push(e)` stores one.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::lowering::constructors::emit_empty_list;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::emit_list_push;
use crate::mir::lowering::expression::collection_literal::{deliver, literal_target};
use crate::mir::{Operand, Place};

pub(crate) fn lower_list_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::List(elements) = &expr.node else {
        unreachable!()
    };
    let target = literal_target(ctx, expr, dest.as_ref());
    emit_empty_list(ctx, &expr.span, &target.ty, target.place.clone());
    for element in elements {
        let list = Operand::Copy(target.place.clone());
        emit_list_push(ctx, list, &target.ty, element, &element.span)?;
    }
    Ok(deliver(ctx, target.place, dest, expr))
}
