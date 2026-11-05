// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Operands read the moment they are evaluated.
//!
//! Operands are evaluated left to right, and each yields its value at the
//! moment it is evaluated. An operand naming a place — a binding, or a field
//! or an element reached from one — lowers to that place, which the code
//! consuming the operands reads only once every operand is lowered: a call
//! when it is made, an operator when it applies. A later operand that writes
//! the place first would change the earlier value, so every operand-list
//! lowering lowers each operand through [`lower_operand`] (or, where another
//! path lowers it, hands the result to [`hold`]), which reads the place into
//! a temporary on the spot when the type checker found that a later operand
//! may write it (`type_checker::body_writes::HeldOperands`).
//!
//! The temporary is a copy, retained like any copy of a counted value, and is
//! declared after the watermark the lowering site took before the operand,
//! so the site releases it where it releases a call result standing in the
//! same position.
//!
//! The checker's verdicts are executed, not advised: once a body is lowered,
//! [`check_every_hold_lowered`] refuses it when an operand the checker held
//! in it never reached [`hold`], so an operand-list lowering that forgets the
//! entry point fails the build instead of reading late.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::ast::expression::Expression;
use crate::ast::statement::Statement;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::mir::{Local, Operand, Place, Rvalue, StatementKind};
use crate::type_checker::body_writes::unlabelled;

use super::dispatch::argument_type;
use super::expression::lower_expression;
use super::LoweringContext;

/// The held operands a body has lowered, and the temporaries they were read
/// into.
#[derive(Debug, Default)]
pub struct HeldReads {
    /// The id of each held operand lowered, any named-argument label taken
    /// off. A kernel cut out of the body (a `gpu forall`, a frame pass, a
    /// reduction) shares the body's set: the checker holds operands per
    /// function body, so every MIR body lowered from one function body
    /// records into one ledger.
    ids: Rc<RefCell<HashSet<usize>>>,
    /// The temporaries held operands were read into, locals of this MIR
    /// body alone.
    temps: HashSet<Local>,
}

impl HeldReads {
    /// The ledger of a kernel lowered out of the body `self` belongs to: the
    /// same held ids, and no temporaries yet.
    pub fn for_kernel(&self) -> Self {
        Self {
            ids: Rc::clone(&self.ids),
            temps: HashSet::new(),
        }
    }
}

/// `expression`, lowered as an operand of a sequence: read into a temporary
/// now when a later operand of the same sequence may write the place it
/// names.
pub(crate) fn lower_operand(
    ctx: &mut LoweringContext,
    expression: &Expression,
) -> Result<Operand, LoweringError> {
    let operand = lower_expression(ctx, expression, None)?;
    Ok(hold(ctx, operand, expression))
}

/// `operand`, lowered from `expression`, read into a temporary now when a
/// later operand of the same sequence may write the place it names;
/// otherwise `operand` itself.
pub(crate) fn hold(
    ctx: &mut LoweringContext,
    operand: Operand,
    expression: &Expression,
) -> Operand {
    if !ctx.type_checker.held_operands.holds(expression) {
        return operand;
    }
    (ctx.held_reads.ids.borrow_mut()).insert(unlabelled(expression).id);
    let (Operand::Copy(place) | Operand::Move(place)) = &operand else {
        return operand;
    };
    // A temporary a hold made is already the value as it was evaluated.
    let is_held_already = held_temporary(ctx, &operand).is_some();
    // A gpu-resident binding is a device buffer; reading it on the host
    // takes a readback this copy would skip.
    // TODO: hold a gpu-resident operand a later operand may write once a
    // copy of one can carry its device handle; it is read as it stands.
    if is_held_already || ctx.body.local_decls[place.local.0].device_handle.is_some() {
        return operand;
    }
    let ty = argument_type(ctx, &operand, expression);
    let temp = ctx.push_temp(ty, expression.span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(Operand::Copy(place.clone()))),
        span: expression.span,
    });
    ctx.held_reads.temps.insert(temp);
    Operand::Copy(Place::new(temp))
}

/// The temporary a hold read `operand` into, when it is one.
pub(crate) fn held_temporary(ctx: &LoweringContext, operand: &Operand) -> Option<Local> {
    let (Operand::Copy(place) | Operand::Move(place)) = operand else {
        return None;
    };
    (place.projection.is_empty() && ctx.held_reads.temps.contains(&place.local))
        .then_some(place.local)
}

/// Refuses the lowered function or closure body `body` when an operand the
/// type checker holds in it never went through [`hold`].
pub(crate) fn check_every_hold_lowered(
    ctx: &LoweringContext,
    body: &Statement,
) -> Result<(), LoweringError> {
    let held = ctx.type_checker.held_operands.held_in(body);
    let lowered = ctx.held_reads.ids.borrow();
    let Some(missed) = held.iter().find(|h| !lowered.contains(&h.id)) else {
        return Ok(());
    };
    Err(LoweringError::internal(
        DiagnosticCode::MirValidationFailed,
        "an operand a later operand may write was lowered without being read where it is \
         evaluated",
        missed.span,
    ))
}
