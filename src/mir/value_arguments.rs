// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every value held as its bytes that a call takes is a whole local.
//!
//! A value argument travels as the address of its bytes, which the callee only
//! reads. An argument read through a projection — `f(c, c.p)`, `c.take(c.p)`
//! — would hand over an address inside its holder, and the callee could then
//! see a write it makes through another argument, or read the holder after
//! replacing and freeing it. So each such argument is copied into a fresh
//! value local of its own just before the call, and the call takes that.
//!
//! A runtime entry point is spelled by the element ABI instead, and an `out`
//! argument names the place the callee writes, so neither is copied.

use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::{Body, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind};

/// Copy every projected value argument of every call in `body` into a whole
/// local ahead of the call.
pub fn copy_projected_value_arguments(body: &mut Body) {
    if body.is_gpu() {
        return;
    }
    for block in 0..body.basic_blocks.len() {
        for position in projected_value_arguments(body, block) {
            copy_argument(body, block, position);
        }
    }
}

/// The positions of the arguments of the call ending `block` that read a
/// value held as its bytes through a projection, and are neither handed to a
/// runtime entry point nor written back as `out`.
pub fn projected_value_arguments(body: &Body, block: usize) -> Vec<usize> {
    let Some(TerminatorKind::Call {
        func,
        args,
        arg_modes,
        ..
    }) = body.basic_blocks[block]
        .terminator
        .as_ref()
        .map(|terminator| &terminator.kind)
    else {
        return Vec::new();
    };
    if func.called_runtime_name().is_some() {
        return Vec::new();
    }
    let types = PlaceTypes::of(body);
    args.iter()
        .enumerate()
        .filter(|(position, _)| arg_mode(arg_modes, *position) != ArgMode::Out)
        .filter_map(|(position, arg)| {
            let (Operand::Copy(place) | Operand::Move(place)) = arg else {
                return None;
            };
            let declared = types.declared(place)?;
            (!place.projection.is_empty() && body.value_layout(&declared.kind).is_some())
                .then_some(position)
        })
        .collect()
}

/// Copy argument `position` of the call ending `block` into a new value local
/// and pass that instead.
fn copy_argument(body: &mut Body, block: usize, position: usize) {
    let Some(terminator) = body.basic_blocks[block].terminator.as_ref() else {
        return;
    };
    let span = terminator.span;
    let TerminatorKind::Call { args, .. } = &terminator.kind else {
        return;
    };
    let Some(Operand::Copy(place) | Operand::Move(place)) = args.get(position).cloned() else {
        return;
    };
    let Some(declared) = PlaceTypes::of(body).declared(&place) else {
        return;
    };
    let copy = Place::new(body.new_local(LocalDecl::new(declared, span)));
    body.basic_blocks[block].statements.push(Statement {
        kind: StatementKind::Assign(copy.clone(), Rvalue::Use(Operand::Copy(place))),
        span,
    });
    if let Some(TerminatorKind::Call { args, .. }) = body.basic_blocks[block]
        .terminator
        .as_mut()
        .map(|terminator| &mut terminator.kind)
    {
        args[position] = Operand::Copy(copy);
    }
}
