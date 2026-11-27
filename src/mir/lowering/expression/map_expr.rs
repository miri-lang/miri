// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of a map literal: the empty map its type names, then one
//! `miri_rt_map_set` per entry, the way `map.set(k, v)` stores one.
//!
//! Every entry is lowered before the first is stored, as a call's arguments
//! are: an entry may read a resource an earlier entry moves into the map. A
//! moved binding ends right after its own entry's store, which hands the
//! store the binding's own reference.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::{lower_map_entry, store_map_entry};
use crate::mir::lowering::expression::collection_literal::{deliver, literal_target};
use crate::mir::lowering::expression::literal_elements::end_moved_elements;
use crate::mir::lowering::written_receiver::Receiver;
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
    let lowered = pairs
        .iter()
        .map(|(key, value)| lower_map_entry(ctx, &target.ty, (key, value)))
        .collect::<Result<Vec<_>, _>>()?;
    for (entry, (key, value)) in lowered.into_iter().zip(pairs) {
        let map = Receiver::Ready(Operand::Copy(target.place.clone()));
        store_map_entry(ctx, map, entry, &key.span)?;
        end_moved_elements(ctx, [key, value], key.span)?;
    }
    Ok(deliver(ctx, target.place, dest, expr))
}
