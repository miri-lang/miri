// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that a body never writes a parameter whose value its caller still
//! holds.
//!
//! A collection, a struct or a tuple a caller passes is a value the caller
//! keeps: a body that writes one binds a copy of it on entry and writes the
//! copy (`lowering::written_parameters`). The parameter's own count holds only
//! the caller's references, so a uniqueness check or a copy-on-write call on
//! it answers for the caller, and a store into it lands in the caller's value.
//! Each of those is a finding here: assigning to it or through it, or handing
//! it, whole or through a projection, to a runtime entry point at a position
//! the entry point does not only read (`runtime_fns::only_reads_argument`) —
//! asking whether it is shared, copying it on write, or any write the runtime
//! makes in place, which a write lowered with no copy-on-write check at all
//! would be. An element a collection stores or looks up is not written.
//!
//! A method's receiver is written in place, since the caller made it the
//! body's own before calling a method that writes it; so is an `out`
//! parameter, the caller's binding itself, and a parameter the body takes
//! over from its callers. A GPU body holds no count and is skipped.

use super::{local_display_name, VerificationViolation};
use crate::mir::block::BasicBlockData;
use crate::mir::terminator::TerminatorKind;
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::{Body, Local, Operand, StatementKind};
use crate::runtime_fns::{element_positions, only_reads_argument};
use std::collections::BTreeSet;

/// Every parameter of `body` whose value its caller still holds that the
/// body writes, or hands a runtime entry point that may write it.
pub fn verify_parameter_writes(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let written: BTreeSet<usize> = body
        .basic_blocks
        .iter()
        .flat_map(written_locals)
        .filter(|&local| is_callers_value(body, local))
        .map(|local| local.0)
        .collect();
    written
        .into_iter()
        .map(|index| violation(body, Local(index)))
        .collect()
}

/// The locals `block` assigns to or through, and those its call hands a
/// runtime entry point at a position it does not only read.
fn written_locals(block: &BasicBlockData) -> Vec<Local> {
    let stored = block
        .statements
        .iter()
        .filter_map(|statement| stored_local(&statement.kind));
    let handed = block
        .terminator
        .as_ref()
        .map(|terminator| written_arguments(&terminator.kind))
        .unwrap_or_default();
    stored.chain(handed).collect()
}

/// The local a statement of `kind` stores into, or through.
fn stored_local(kind: &StatementKind) -> Option<Local> {
    match kind {
        StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => Some(place.local),
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => None,
    }
}

/// The locals a call to a runtime entry point is handed, whole or through a
/// projection, at a position the entry point does not only read and that is
/// not an element it stores or looks up: asking whether a value is shared,
/// copying it on write, and every write the runtime makes in place. A call to
/// a Miri function writes its own copy of what it is handed, so it hands
/// nothing over here.
fn written_arguments(kind: &TerminatorKind) -> Vec<Local> {
    let (func, args) = match kind {
        TerminatorKind::Call { func, args, .. } => (func, args),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::VirtualCall { .. } => return Vec::new(),
    };
    let Some(name) = func.called_runtime_name() else {
        return Vec::new();
    };
    let elements = element_positions(name);
    args.iter()
        .enumerate()
        .filter(|(position, _)| {
            !only_reads_argument(name, *position) && !elements.contains(position)
        })
        .filter_map(|(_, arg)| match arg {
            Operand::Copy(place) | Operand::Move(place) => Some(place.local),
            Operand::Constant(_) | Operand::Function(_) => None,
        })
        .collect()
}

/// Whether `local` is a parameter holding a value its caller still holds and
/// a write would copy.
fn is_callers_value(body: &Body, local: Local) -> bool {
    let decl = &body.local_decls[local.0];
    body.is_lent_param(local)
        && !body.is_owned_param(local)
        && decl.device_handle.is_none()
        && is_copied_on_write(body, &decl.ty)
}

fn violation(body: &Body, local: Local) -> VerificationViolation {
    VerificationViolation {
        local,
        local_name: local_display_name(body, local),
        message: "a parameter whose value its caller still holds is written, or checked or \
                  copied for a write, in place of the copy the body binds on entry"
            .to_string(),
    }
}
