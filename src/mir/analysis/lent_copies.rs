// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The locals of a body holding nothing but a copy of a parameter its caller
//! lends it.
//!
//! A body is lent a parameter: its caller holds the value for the whole call.
//! A local whose every definition copies such a parameter whole holds a
//! second reference to the caller's value, or none at all where it borrows.
//! Releasing it therefore never releases the last reference, and runs no
//! drop hook, while the parameter stays as it was: never defined again,
//! written through, moved, released, taken the address of, written by a call
//! or handed to one other than lent. The copy itself may be released, which
//! is the release this answers for, but nothing else may write it. A match
//! lowered over `self` reads its subject this way.

use crate::mir::facts::OwnershipFacts;
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::{Body, Local, Operand, Rvalue, Statement, StatementKind};
use std::collections::HashSet;

/// The locals of `body` whose every definition copies whole a parameter the
/// body is lent and leaves as it was, and that nothing else writes.
pub fn lent_copies(body: &Body, ownership: &OwnershipFacts) -> HashSet<Local> {
    let touched = touched_locals(body);
    let is_lent_parameter = |local: Local| {
        (1..=body.arg_count).contains(&local.0)
            && !ownership.is_owned_param(local)
            && !is_out_parameter(body, local)
            && !body.captures.env_capture_locals.contains(&local)
            && !touched.contains(&local)
    };
    let mut copies = HashSet::new();
    let mut refused = HashSet::new();
    for statement in body.basic_blocks.iter().flat_map(|data| &data.statements) {
        let Some(written) = statement.written_place() else {
            continue;
        };
        match copied_whole(statement) {
            Some(source) if is_lent_parameter(source) => copies.insert(written.local),
            Some(_) | None => refused.insert(written.local),
        };
    }
    copies
        .into_iter()
        .filter(|local| {
            local.0 > body.arg_count
                && !refused.contains(local)
                && !touched.contains(local)
                && !body.captures.env_capture_locals.contains(local)
        })
        .collect()
}

/// The source of `statement` when it copies one whole local into another.
fn copied_whole(statement: &Statement) -> Option<Local> {
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
        &statement.kind
    else {
        return None;
    };
    let Rvalue::Use(Operand::Copy(source)) = rvalue else {
        return None;
    };
    (dest.projection.is_empty() && source.projection.is_empty()).then_some(source.local)
}

/// Whether `local` is a parameter of `body` declared `out`.
fn is_out_parameter(body: &Body, local: Local) -> bool {
    (local.0.checked_sub(1))
        .and_then(|index| body.signature.out_params.get(index))
        .copied()
        .unwrap_or(false)
}

/// The locals of `body` written other than by a whole definition, moved,
/// taken the address of, written by a call or handed to one other than
/// lent; and every parameter defined, released or deallocated.
fn touched_locals(body: &Body) -> HashSet<Local> {
    let mut touched = HashSet::new();
    for data in &body.basic_blocks {
        for statement in &data.statements {
            touched_by_statement(body, statement, &mut touched);
        }
        if let Some(terminator) = &data.terminator {
            touched_by_terminator(&terminator.kind, &mut touched);
        }
    }
    touched
}

fn touched_by_statement(body: &Body, statement: &Statement, touched: &mut HashSet<Local>) {
    let is_parameter = |local: Local| (1..=body.arg_count).contains(&local.0);
    match &statement.kind {
        StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
            if !dest.projection.is_empty() || is_parameter(dest.local) {
                touched.insert(dest.local);
            }
            touched_by_rvalue(rvalue, touched);
        }
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
            if is_parameter(place.local) {
                touched.insert(place.local);
            }
        }
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_, _)
        | StatementKind::IncRef(_)
        | StatementKind::Nop
        | StatementKind::ReadHeader(_) => {}
    }
}

fn touched_by_rvalue(rvalue: &Rvalue, touched: &mut HashSet<Local>) {
    let operands: Vec<&Operand> = match rvalue {
        Rvalue::Use(operand) => vec![operand],
        Rvalue::UnaryOp(_, operand) | Rvalue::Cast(operand, _) => vec![operand.as_ref()],
        Rvalue::BinaryOp(_, left, right) => vec![left.as_ref(), right.as_ref()],
        Rvalue::Aggregate(_, items) | Rvalue::MathIntrinsic(_, items) => items.iter().collect(),
        Rvalue::Ref(place) => {
            touched.insert(place.local);
            Vec::new()
        }
        Rvalue::Len(_) | Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => Vec::new(),
    };
    for operand in operands {
        if let Operand::Move(place) = operand {
            touched.insert(place.local);
        }
    }
}

fn touched_by_terminator(kind: &TerminatorKind, touched: &mut HashSet<Local>) {
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
        | TerminatorKind::GpuLaunch { .. } => return,
    };
    touched.insert(destination.local);
    if let Some(Operand::Move(place)) = func {
        touched.insert(place.local);
    }
    for (position, arg) in args.iter().enumerate() {
        let Some(place) = arg.place() else {
            continue;
        };
        let is_lent =
            matches!(arg, Operand::Copy(_)) && arg_mode(arg_modes, position) == ArgMode::Borrowed;
        if !is_lent {
            touched.insert(place.local);
        }
    }
}
