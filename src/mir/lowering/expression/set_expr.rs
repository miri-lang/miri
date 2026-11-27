// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of a set literal: the empty set its type names, then one
//! `miri_rt_set_add` per element, the way `set.add(e)` stores one.
//!
//! Every element is lowered before the first is stored, as a call's
//! arguments are: an element may read a resource an earlier element moves
//! into the set. A moved binding ends right after its own store, which
//! hands the store the binding's own reference.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::{store_set_element, ELEMENT_SLOT};
use crate::mir::lowering::expression::collection_literal::{deliver, literal_target};
use crate::mir::lowering::expression::literal_elements::end_moved_elements;
use crate::mir::lowering::stored_values::lower_kept_value;
use crate::mir::lowering::written_receiver::Receiver;
use crate::mir::{AggregateKind, Operand, Place, Rvalue, Statement, StatementKind};

pub(crate) fn lower_set_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Set(elements) = &expr.node else {
        unreachable!()
    };
    let target = literal_target(ctx, expr, dest.as_ref());
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            target.place.clone(),
            Rvalue::Aggregate(AggregateKind::Set, Vec::new()),
        ),
        span: expr.span,
    });
    let lowered = elements
        .iter()
        .map(|element| lower_kept_value(ctx, element, &target.ty, ELEMENT_SLOT))
        .collect::<Result<Vec<_>, _>>()?;
    for (value, element) in lowered.into_iter().zip(elements) {
        let set = Receiver::Ready(Operand::Copy(target.place.clone()));
        store_set_element(ctx, set, (value, element), None, &element.span)?;
        end_moved_elements(ctx, [element], element.span)?;
    }
    Ok(deliver(ctx, target.place, dest, expr))
}
