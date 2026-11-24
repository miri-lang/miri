// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which releases in a body cannot be the last, worked out for the verifiers
//! on their own.
//!
//! A local whose every definition copies whole a parameter the body is lent
//! holds a reference beside the caller's, which outlives the body, so its
//! release never runs a drop hook. The rule reads the body alone, never the
//! analysis the passes ask the same question of. A parameter is lent here
//! when the body only ever reads it, retains it, or hands it to a call by
//! copy, never as an `out` place: a parameter the body takes over is released
//! or handed on somewhere in it, and every write, move, release or address
//! taken disqualifies it. The copy itself is never written but by those
//! definitions, by a call or through an `out` place, and its address is never
//! taken.

use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::{Body, Local, Operand, Place, Rvalue, StatementKind};
use std::collections::HashSet;

/// The locals of `body` whose release leaves a reference its caller holds.
pub(super) fn copies_of_lent_parameters(body: &Body) -> HashSet<Local> {
    let disturbed = disturbed_locals(body);
    let is_lent = |local: Local| {
        (1..=body.arg_count).contains(&local.0)
            && !body
                .signature
                .out_params
                .get(local.0 - 1)
                .copied()
                .unwrap_or(false)
            && !body.captures.env_capture_locals.contains(&local)
            && !disturbed.contains(&local)
    };
    let mut copies = HashSet::new();
    let mut other = HashSet::new();
    for statement in body.basic_blocks.iter().flat_map(|data| &data.statements) {
        let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
            &statement.kind
        else {
            continue;
        };
        if !dest.projection.is_empty() {
            continue;
        }
        match rvalue {
            Rvalue::Use(Operand::Copy(source))
                if source.projection.is_empty() && is_lent(source.local) =>
            {
                copies.insert(dest.local)
            }
            Rvalue::Use(_)
            | Rvalue::UnaryOp(..)
            | Rvalue::BinaryOp(..)
            | Rvalue::Cast(..)
            | Rvalue::Aggregate(..)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::Ref(_)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::AtomicOp { .. } => other.insert(dest.local),
        };
    }
    copies.retain(|local| {
        local.0 > body.arg_count
            && !other.contains(local)
            && !disturbed.contains(local)
            && !body.captures.env_capture_locals.contains(local)
    });
    copies
}

/// Every local `body` writes through, moves, takes the address of, writes
/// by a call or as an `out` place, or hands to a call other than by a
/// borrowed copy; and every parameter it defines or releases.
fn disturbed_locals(body: &Body) -> HashSet<Local> {
    let mut disturbed = HashSet::new();
    let is_parameter = |place: &Place| (1..=body.arg_count).contains(&place.local.0);
    for data in &body.basic_blocks {
        for statement in &data.statements {
            match &statement.kind {
                StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                    if !dest.projection.is_empty() || is_parameter(dest) {
                        disturbed.insert(dest.local);
                    }
                    disturbed.extend(moved_or_referenced(rvalue));
                }
                StatementKind::DecRef(place) | StatementKind::Dealloc(place)
                    if is_parameter(place) =>
                {
                    disturbed.insert(place.local);
                }
                StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::StorageLive(_)
                | StatementKind::StorageDead(..)
                | StatementKind::IncRef(_)
                | StatementKind::Nop
                | StatementKind::ReadHeader(_) => {}
            }
        }
        if let Some(terminator) = &data.terminator {
            disturbed.extend(handed_by_call(&terminator.kind));
        }
    }
    disturbed
}

/// The locals `rvalue` moves out of or takes the address of.
fn moved_or_referenced(rvalue: &Rvalue) -> Vec<Local> {
    let moved = |operand: &Operand| match operand {
        Operand::Move(place) => Some(place.local),
        Operand::Copy(_) | Operand::Constant(_) | Operand::Function(_) => None,
    };
    match rvalue {
        Rvalue::Use(operand) => moved(operand).into_iter().collect(),
        Rvalue::UnaryOp(_, operand) | Rvalue::Cast(operand, _) => {
            moved(operand).into_iter().collect()
        }
        Rvalue::BinaryOp(_, left, right) => moved(left).into_iter().chain(moved(right)).collect(),
        Rvalue::Aggregate(_, operands) | Rvalue::MathIntrinsic(_, operands) => {
            operands.iter().filter_map(moved).collect()
        }
        Rvalue::Ref(place) => vec![place.local],
        Rvalue::Len(_) | Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => Vec::new(),
    }
}

/// The locals a call writes, as its result or an `out` place, or is handed
/// other than as a borrowed copy, its callee included.
fn handed_by_call(kind: &TerminatorKind) -> Vec<Local> {
    let (func, args, arg_modes, destination) = match kind {
        TerminatorKind::Call {
            func,
            args,
            arg_modes,
            destination,
            ..
        } => (Some(func), args, arg_modes, destination),
        TerminatorKind::VirtualCall {
            args,
            arg_modes,
            destination,
            ..
        } => (None, args, arg_modes, destination),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::GpuLaunch { .. } => return Vec::new(),
    };
    let moved_callee = match func {
        Some(Operand::Move(place)) => Some(place.local),
        Some(Operand::Copy(_) | Operand::Constant(_) | Operand::Function(_)) | None => None,
    };
    let handed_args = args.iter().enumerate().filter_map(|(position, arg)| {
        let lent =
            matches!(arg, Operand::Copy(_)) && arg_mode(arg_modes, position) == ArgMode::Borrowed;
        (!lent)
            .then(|| arg.place().map(|place| place.local))
            .flatten()
    });
    std::iter::once(destination.local)
        .chain(moved_callee)
        .chain(handed_args)
        .collect()
}
