// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The collections a call to a runtime entry point writes in place.
//!
//! A runtime entry point a body calls by name — `miri_rt_list_clear(self)` in
//! the standard library, or one a program declares itself — writes the
//! collection it is handed at each position `runtime_fns::writes_in_place`
//! names. That call is a write like a writing method's, so the argument is
//! lowered the way a writing method's receiver is
//! (`method_dispatch::lower_mutated_receiver`): a whole binding is copied on
//! write and the copy written back into it, the body's own receiver, which its
//! caller made the body's own, is checked to have arrived unique instead, and
//! a field, an element or a map entry has every value on its path made the
//! program's own first.
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

use super::method_dispatch::{emit_cow_check, lower_mutated_receiver};
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

/// Lowers `value`, the argument at `position` of a call to `callee`: as the
/// collection the call writes when the entry point writes it in place, and
/// as any other expression otherwise.
///
/// The temp a written path starts at, when no binding holds its value, is
/// the root of the operand returned, so it is released with the call's other
/// argument temps once the call returns.
///
/// Arguments are lowered in the order they are written, so the written one
/// is made the call's own before any argument after it is lowered. A later
/// argument that captures it — a closure over the same list handed to
/// `miri_rt_list_sort_by` — retains it again, and the runtime then writes a
/// block that closure also holds and reads while the call runs: the sort
/// detaches the list's elements, so the closure finds it empty and an index
/// into it stops the program with an out-of-bounds error.
pub(super) fn lower_call_argument(
    ctx: &mut LoweringContext,
    callee: Option<RuntimeCallee<'_>>,
    position: usize,
    value: &Expression,
) -> Result<Operand, LoweringError> {
    let Some(written) = callee.and_then(|callee| callee.written_parameter(position)) else {
        return lower_expression(ctx, value, None);
    };
    let cow = written.kind.as_builtin_collection().and_then(cow_fn);
    let (operand, _released_with_the_arguments) =
        lower_mutated_receiver(ctx, value, cow.map(|cow| (written, cow)), value.span)?;
    Ok(operand)
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
