// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of a map literal: the empty map its type names, then one
//! `miri_rt_map_set` per entry, the way `map.set(k, v)` stores one.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::emit_map_set;
use crate::mir::lowering::expression::collection_literal::{deliver, literal_target};
use crate::mir::{AggregateKind, Operand, Place, Rvalue, Statement, StatementKind};

pub(crate) fn lower_map_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Map(pairs) = &expr.node else {
        unreachable!()
    };
    let target = literal_target(ctx, expr, dest.as_ref());
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            target.place.clone(),
            Rvalue::Aggregate(AggregateKind::Map, Vec::new()),
        ),
        span: expr.span,
    });
    for (key, value) in pairs {
        let map = Operand::Copy(target.place.clone());
        emit_map_set(ctx, map, &target.ty, key, value, &key.span)?;
    }
    Ok(deliver(ctx, target.place, dest, expr))
}
