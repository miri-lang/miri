// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a literal does with a resource written as one of its elements.
//!
//! An element of an array, tuple, set or map literal is a place, so a
//! resource binding written there moves into the literal: the type checker
//! records the move, and the literal ends the binding once it is built
//! ([`end_moved_elements`]). A payload binding matched out of an optional
//! the literal ends hands the literal the optional's own reference
//! ([`lower_aggregate_elements`]) rather than a retained copy beside it.

use crate::ast::expression::Expression;
use crate::ast::types::Type;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::dispatch::coerce_arg_to_declared;
use crate::mir::lowering::kept_parameters::hand_over_stored_payload;
use crate::mir::lowering::sequencing::lower_operand;
use crate::mir::lowering::variable::{consumed_operand_locals, end_consumed_binding};
use crate::mir::{Local, Operand, Place, Rvalue, Statement, StatementKind};

/// An aggregate literal's elements, lowered: the operand of each, and the
/// local each took over in place of a matched payload binding, which
/// [`crate::mir::lowering::kept_parameters::end_stored_payloads`] ends,
/// moved, once the literal is built.
pub(crate) struct LoweredElements {
    pub(crate) ops: Vec<Operand>,
    pub(crate) handed: Vec<Option<Local>>,
}

/// Lowers each element of an aggregate literal, converted to the type its
/// slot declares (`slots`, by position; `None` stores it as it is), handing
/// each element that reads a payload binding matched out of an optional the
/// literal ends the optional's own reference ([`hand_over_stored_payload`]).
/// Every element is lowered first, so a later element may still read a
/// payload binding an earlier one stores. The reference is moved into an
/// element temp of its own: the literal funds each element it stores and
/// its temps are released after it, so the element keeps the one reference
/// the optional held. Such an element's conversion to its slot waits for the
/// hand-over, so an optional slot wraps the reference the literal takes
/// over, `Some(move o.0)`, rather than a copy beside it.
pub(crate) fn lower_aggregate_elements(
    ctx: &mut LoweringContext,
    elements: &[Expression],
    slots: &[Option<Type>],
) -> Result<LoweredElements, LoweringError> {
    let mut lowered = Vec::with_capacity(elements.len());
    for (element, slot) in elements.iter().zip(slots) {
        let watermark = ctx.body.local_decls.len();
        let op = lower_operand(ctx, element)?;
        let is_handed_over = ctx.type_checker.ended_scrutinee(element).is_some();
        lowered.push(match slot {
            Some(target) if !is_handed_over => (
                coerce_arg_to_declared(ctx, op, element, target, watermark),
                None,
            ),
            Some(target) => (op, Some((target, watermark))),
            None => (op, None),
        });
    }
    let mut ops = Vec::with_capacity(elements.len());
    let mut handed = Vec::with_capacity(elements.len());
    for (element, (mut op, owed_slot)) in elements.iter().zip(lowered) {
        // The binding's own read, before the hand-over re-spells it as a
        // projection of the optional, carries the element's type.
        let ty = op.ty(&ctx.body).clone();
        let taken = hand_over_stored_payload(ctx, element, &mut op, element.span);
        if taken.is_some() {
            op = spill_to_element_temp(ctx, (op, ty), element.span);
        }
        if let Some((target, watermark)) = owed_slot {
            op = coerce_arg_to_declared(ctx, op, element, target, watermark);
        }
        ops.push(op);
        handed.push(taken);
    }
    Ok(LoweredElements { ops, handed })
}

/// Moves `op`, of type `ty`, into a temp and answers the temp's read.
pub(crate) fn spill_to_element_temp(
    ctx: &mut LoweringContext,
    (op, ty): (Operand, Type),
    span: Span,
) -> Operand {
    let temp = ctx.push_temp(ty, span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(op)),
        span,
    });
    Operand::Copy(Place::new(temp))
}

/// Ends, once the literal is built, each resource binding one of its
/// `elements` (a map literal's keys and values) moved into it.
pub(crate) fn end_moved_elements<'e>(
    ctx: &mut LoweringContext,
    elements: impl IntoIterator<Item = &'e Expression>,
    span: Span,
) -> Result<(), LoweringError> {
    for element in elements {
        for (_, binding) in consumed_operand_locals(ctx, None, std::slice::from_ref(element))? {
            end_consumed_binding(ctx, binding, span);
        }
    }
    Ok(())
}
