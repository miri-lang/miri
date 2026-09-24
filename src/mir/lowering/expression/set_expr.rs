// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering of a set literal: the empty set its type names, then one
//! `miri_rt_set_add` per element, the way `set.add(e)` stores one.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::error::lowering::LoweringError;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::emit_set_add;
use crate::mir::lowering::expression::collection_literal::{deliver, literal_target};
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
    for element in elements {
        let set = Operand::Copy(target.place.clone());
        emit_set_add(ctx, set, &target.ty, element, None, &element.span)?;
    }
    Ok(deliver(ctx, target.place, dest, expr))
}
