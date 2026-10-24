// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The collections a call to a runtime entry point writes in place.
//!
//! A runtime entry point a body calls by name — `miri_rt_list_clear(self)` in
//! the standard library, or one a program declares itself — writes the
//! collection it is handed at each position `runtime_fns::writes_in_place`
//! names. That call is a write like a writing method's, so the argument is
//! lowered the way a writing method's receiver is
//! (`written_receiver::lower_pending_receiver`): a whole binding is copied on
//! write and the copy written back into it, the body's own receiver, which its
//! caller made the body's own, is checked to have arrived unique instead, and
//! a field, an element or a map entry has every value on its path made the
//! program's own first — each only after every argument of the call is
//! lowered, right before the call.
//!
//! A parameter whose value its caller still holds is never handed here: a
//! body that hands one to a runtime writer binds a copy of it on entry
//! (`lowering::written_parameters`), and the copy is what is written. The
//! thunk that forwards its parameters to a runtime entry point taken as a
//! value has no source to scan, so it binds the copy where it forwards the
//! parameter ([`copy_forwarded_argument`]).
//!
//! A GPU-resident binding is left alone, the way every write's copy-on-write
//! leaves it: its host block mirrors a device buffer and holds no count of
//! its own to ask about, and the uploads and readbacks that move it between
//! host and device are emitted by residency, never through a call a body
//! writes.

use crate::ast::expression::Expression;
use crate::ast::types::Type;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Local, Operand, Place, Rvalue, Statement, StatementKind};
use crate::runtime_fns::{cow_fn, writes_in_place};

use super::method_dispatch::emit_cow_check;
use super::written_receiver::{lower_pending_receiver, PendingReceiver};
use super::{lower_expression, LoweringContext};

/// The runtime entry point a call is made to, when it is one, with the type
/// of each parameter it declares.
#[derive(Clone, Copy)]
pub(crate) struct RuntimeCallee<'c> {
    name: &'c str,
    parameters: &'c [Type],
}

impl<'c> RuntimeCallee<'c> {
    /// The runtime entry point `func` names, declaring `parameters`; `None`
    /// when `func` names anything else.
    pub(crate) fn of(func: &'c Operand, parameters: &'c [Type]) -> Option<Self> {
        let name = func.called_runtime_name()?;
        Some(Self { name, parameters })
    }

    /// The collection type of the parameter at `position` when the entry
    /// point writes it in place.
    fn written_parameter(self, position: usize) -> Option<&'c Type> {
        let parameter = self.parameters.get(position)?;
        let is_collection = parameter.kind.as_builtin_collection().is_some();
        (is_collection && writes_in_place(self.name, position)).then_some(parameter)
    }
}

/// An argument of a call to a runtime entry point, lowered.
pub(super) enum CallArgument<'e> {
    /// Any argument the entry point does not write in place.
    Lowered(Operand),
    /// The collection the entry point writes in place, its path lowered and
    /// not yet made the call's own ([`PendingReceiver::make_own`]).
    Written(PendingReceiver<'e>),
}

/// Lowers `value`, the argument at `position` of a call to `callee`: as the
/// collection the call writes when the entry point writes it in place, and
/// as any other expression otherwise.
///
/// The written collection is made the call's own only once every argument
/// of the call is lowered, so an argument after it that hands it to another
/// name — a closure over the same list handed to `miri_rt_list_sort_by`
/// captures it — does so before the check, which then copies it: the runtime
/// writes the copy, and the closure reads the list as it was.
///
/// The temp a written path starts at, when no binding holds its value, is
/// the root of the operand the call is handed, so it is released with the
/// call's other argument temps once the call returns.
pub(super) fn lower_call_argument<'e>(
    ctx: &mut LoweringContext,
    callee: Option<RuntimeCallee<'_>>,
    position: usize,
    value: &'e Expression,
) -> Result<CallArgument<'e>, LoweringError> {
    let Some(written) = callee.and_then(|callee| callee.written_parameter(position)) else {
        return lower_expression(ctx, value, None).map(CallArgument::Lowered);
    };
    let cow = written.kind.as_builtin_collection().and_then(cow_fn);
    let (pending, _released_with_the_arguments) =
        lower_pending_receiver(ctx, value, cow.map(|cow| (written, cow)), value.span)?;
    Ok(CallArgument::Written(pending))
}

/// The operand a forwarding thunk hands `callee` at `position` for its own
/// parameter `parameter`, with the copy it bound for it, if any.
///
/// The parameter's value is its caller's, so when the entry point writes it
/// in place the thunk binds a copy, makes the copy the call's own the way any
/// binding's value is made its own before a write, and hands the copy on.
/// The caller releases the copy once the call returns.
pub(crate) fn copy_forwarded_argument(
    ctx: &mut LoweringContext,
    callee: Option<RuntimeCallee<'_>>,
    position: usize,
    parameter: Local,
    span: Span,
) -> Result<(Operand, Option<Local>), LoweringError> {
    let forwarded = Operand::Copy(Place::new(parameter));
    let Some(written) = callee.and_then(|callee| callee.written_parameter(position)) else {
        return Ok((forwarded, None));
    };
    let Some(cow) = written.kind.as_builtin_collection().and_then(cow_fn) else {
        return Ok((forwarded, None));
    };
    let copy = ctx.push_temp(written.clone(), span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(copy), Rvalue::Use(forwarded)),
        span,
    });
    let operand = emit_cow_check(ctx, Operand::Move(Place::new(copy)), written, cow, span)?;
    Ok((operand, Some(copy)))
}
