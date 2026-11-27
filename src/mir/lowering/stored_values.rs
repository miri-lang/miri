// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The value a store keeps in a collection's slot, when the store may take
//! over a matched payload's one reference.
//!
//! A bare value headed for an optional slot is wrapped as `Some(value)`
//! ([`wrap_for_optional_slot`]). Wrapping a payload binding's read as it is
//! lowered builds `Some(copy r)`, which RC insertion funds with a retain
//! while the optional the binding was matched out of keeps its own: two
//! holders. A store that ends the optional ([`TypeChecker::ended_scrutinee`])
//! is handed the optional's reference instead
//! ([`hand_over_stored_payload`]), but only once every other operand of the
//! store is lowered, since a sibling operand may still read the binding. So
//! the value's conversion to its slot waits for the hand-over and wraps the
//! reference the store takes over, `Some(move o.0)`, rather than a copy.
//!
//! [`TypeChecker::ended_scrutinee`]: crate::type_checker::TypeChecker::ended_scrutinee

use crate::ast::expression::Expression;
use crate::ast::Type;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Local, Operand};

use super::dispatch::{collection_slot_type, fit_to_slot, lower_stored_value};
use super::expression::literal_elements::spill_to_element_temp;
use super::helpers::{coercion_wraps_in_some, resolve_arg_type};
use super::kept_parameters::hand_over_stored_payload;
use super::sequencing::lower_operand;
use super::variable;
use super::LoweringContext;

/// A value lowered for the slot a collection's type argument declares, by a
/// store that may hand it a matched payload's reference ([`lower_kept_value`]).
pub(crate) struct StoredValue {
    op: Operand,
    ty: Type,
    /// The slot the value is still to be converted to, when the conversion
    /// waits for the hand-over; `None` once the value fits its slot.
    owed_slot: Option<Type>,
    /// The local count before the value was lowered, below which its
    /// conversion releases nothing.
    watermark: usize,
}

/// The operand a store is handed for a [`StoredValue`].
pub(crate) struct StoreOperand {
    pub(crate) op: Operand,
    /// The local the store takes over in place of a matched payload
    /// binding, which `end_stored_payloads` ends, moved, after the store.
    pub(crate) handed: Option<Local>,
    /// The local the stored value was read from, which the store releases
    /// once it keeps the value when the store's own lowering created it.
    pub(crate) source: Option<Local>,
    /// The local count before the value was lowered.
    pub(crate) watermark: usize,
}

impl StoredValue {
    /// A value lowered on its own, already of the type its slot stores.
    pub(crate) fn fitted(op: Operand, ty: Type, watermark: usize) -> Self {
        StoredValue {
            op,
            ty,
            owed_slot: None,
            watermark,
        }
    }

    /// A value lowered on its own whose conversion to `slot_ty`, the type of
    /// the variable or field a store writes it to, waits for the hand-over.
    pub(crate) fn owing(op: Operand, ty: Type, slot_ty: Type, watermark: usize) -> Self {
        StoredValue {
            op,
            ty,
            owed_slot: Some(slot_ty),
            watermark,
        }
    }

    /// The operand the store is handed, converted to its slot: the matched
    /// payload's one reference in place of the payload binding the value
    /// reads, when the store ends the optional it was matched out of, and
    /// the value's own read otherwise.
    pub(crate) fn hand_over(
        self,
        ctx: &mut LoweringContext,
        value: &Expression,
        span: Span,
    ) -> StoreOperand {
        let StoredValue {
            mut op,
            ty,
            owed_slot,
            watermark,
        } = self;
        let read = op.place().map(|place| place.local);
        let handed = hand_over_stored_payload(ctx, value, &mut op, span);
        // A handed-over payload is counted, so a slot that does not wrap it
        // takes it as it is, and it ends with the store rather than as the
        // store's temp.
        let owed_slot =
            owed_slot.filter(|slot_ty| handed.is_none() || coercion_wraps_in_some(&ty, slot_ty));
        let Some(slot_ty) = owed_slot else {
            return StoreOperand {
                op,
                handed,
                source: read,
                watermark,
            };
        };
        // The wrap retains what it reads and releases a temp of the store's
        // own, so the handed-over reference moves into one first and the
        // wrap is the only holder left once the temp is released.
        if handed.is_some() {
            op = spill_to_element_temp(ctx, (op, ty.clone()), span);
        }
        let (op, _) = fit_to_slot(ctx, (op, ty), &slot_ty, watermark, span);
        StoreOperand {
            source: op.place().map(|place| place.local),
            op,
            handed,
            watermark,
        }
    }

    /// The operand a store that hands nothing over is handed: the value
    /// converted to its slot.
    pub(crate) fn into_store_operand(self, ctx: &mut LoweringContext, span: Span) -> StoreOperand {
        let op = match self.owed_slot {
            Some(slot_ty) => fit_to_slot(ctx, (self.op, self.ty), &slot_ty, self.watermark, span).0,
            None => self.op,
        };
        StoreOperand {
            source: op.place().map(|place| place.local),
            op,
            handed: None,
            watermark: self.watermark,
        }
    }
}

impl StoreOperand {
    /// Releases the temp the store read the value from once the store keeps
    /// it, when the store's own lowering created it: the `Some(..)` a
    /// handed-over payload was wrapped in, which no one reads after.
    pub(crate) fn release_source(&self, ctx: &mut LoweringContext, span: Span) {
        if let Some(source) = self.source {
            ctx.emit_temp_drop(source, self.watermark, span);
        }
    }
}

/// Lower a value a store keeps in the slot `collection_ty`'s `slot`-th type
/// argument declares, as [`lower_stored_value`] does, except that the
/// conversion of a payload binding matched out of an optional the store
/// ends waits for [`StoredValue::hand_over`], which wraps the one reference
/// the optional held rather than a copy of the binding.
pub(crate) fn lower_kept_value(
    ctx: &mut LoweringContext,
    value_arg: &Expression,
    collection_ty: &Type,
    slot: usize,
) -> Result<StoredValue, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let value = variable::argument_value(value_arg);
    if ctx.type_checker.ended_scrutinee(value).is_none() {
        let (op, ty) = lower_stored_value(ctx, value_arg, collection_ty, slot)?;
        return Ok(StoredValue::fitted(op, ty, watermark));
    }
    let op = lower_operand(ctx, value_arg)?;
    let ty = resolve_arg_type(ctx, value_arg, &op);
    Ok(StoredValue {
        op,
        ty,
        owed_slot: collection_slot_type(ctx, collection_ty, slot),
        watermark,
    })
}
