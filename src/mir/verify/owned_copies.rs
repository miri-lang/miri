// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every argument a call hands over by copy is funded.
//!
//! A call or a virtual call passing an argument [`ArgMode::Owned`] gives the
//! callee a reference it releases; one passed [`ArgMode::Donated`] gives the
//! container the callee stores it in a reference it keeps. An argument
//! spelled `move` is the caller's own reference. One spelled as a copy — a
//! field, an element, a lent parameter, a binding the caller keeps — is a
//! reference the caller still holds, so each such copy is funded by a retain
//! of that place in the block the call ends, after the last write to the
//! place's local there: a place handed over twice needs two, and a retain
//! followed by a write to the place funds the value the write replaced, not
//! the one the call reads. Without one, the callee's release frees a value
//! its holder still reads.
//!
//! The rule reads the body alone. A constant needs no retain (every constant
//! reaching a counted parameter is inert to counting), a donated value that
//! is not counted needs none, and a GPU body holds no counts.

use super::{local_display_name, VerificationViolation};
use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::arg_mode;
use crate::mir::{ArgMode, BasicBlockData, Body, Local, Operand, Place, Statement, StatementKind};

/// Every argument a call or virtual call in `body` hands over by copy with
/// fewer retains of it in the block the call ends than the call hands it over.
pub fn verify_owned_copies(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    (body.basic_blocks.iter().enumerate())
        .flat_map(|(block, data)| {
            (unfunded_copies(body, data).into_iter()).map(move |found| (block, found))
        })
        .map(|(block, (position, place))| unfunded_copy(body, place, block, position))
        .collect()
}

/// Each place the call ending `data` hands over by copy more often than
/// retains in `data` fund it, with the first argument position it is passed
/// at.
fn unfunded_copies<'d>(body: &Body, data: &'d BasicBlockData) -> Vec<(usize, &'d Place)> {
    let copies = funded_copies(body, data);
    (copies.iter().enumerate())
        .filter(|&(index, &(_, place))| copies[..index].iter().all(|&(_, seen)| seen != place))
        .map(|(_, &copy)| copy)
        .filter(|&(_, place)| {
            let handed = copies.iter().filter(|&&(_, other)| other == place).count();
            funding_retains(data, place) < handed
        })
        .collect()
}

/// Each argument the call ending `data` hands over by copy that the caller
/// pays a reference for, with its position: every one passed owned, and
/// every counted one passed donated.
fn funded_copies<'d>(body: &Body, data: &'d BasicBlockData) -> Vec<(usize, &'d Place)> {
    let passed = (data.terminator.as_ref()).and_then(|terminator| terminator.passed_arguments());
    let (args, modes) = passed.unwrap_or((&[], &[]));
    (args.iter().enumerate())
        .filter_map(|(position, arg)| match arg {
            Operand::Copy(place) => Some((position, place)),
            Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => None,
        })
        .filter(|&(position, place)| match arg_mode(modes, position) {
            ArgMode::Owned => true,
            ArgMode::Donated => is_counted_place(body, place),
            ArgMode::Borrowed | ArgMode::Out => false,
        })
        .collect()
}

/// Whether the value at `place` is counted; a place whose type cannot be
/// read is not checked here.
fn is_counted_place(body: &Body, place: &Place) -> bool {
    (PlaceTypes::of(body).declared(place)).is_some_and(|ty| body.is_counted(&ty.kind))
}

/// The retains of `place` in `data` after the last statement there writing
/// its local.
fn funding_retains(data: &BasicBlockData, place: &Place) -> usize {
    (data.statements.iter().rev())
        .take_while(|statement| !writes_local(statement, place.local))
        .filter(|statement| matches!(&statement.kind, StatementKind::IncRef(retained) if retained == place))
        .count()
}

/// Whether `statement` stores into, or starts or ends the storage of, `local`.
fn writes_local(statement: &Statement, local: Local) -> bool {
    match &statement.kind {
        StatementKind::Assign(place, _)
        | StatementKind::Reassign(place, _)
        | StatementKind::StorageLive(place)
        | StatementKind::StorageDead(place, _) => place.local == local,
        StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::ReadHeader(_)
        | StatementKind::Nop => false,
    }
}

fn unfunded_copy(
    body: &Body,
    place: &Place,
    block: usize,
    position: usize,
) -> VerificationViolation {
    VerificationViolation {
        local: place.local,
        local_name: local_display_name(body, place.local),
        message: format!(
            "handed over by copy, as `{place}`, at argument {position} of the call ending \
             bb{block} with no retain funding it, so the callee releases a reference its \
             holder still reads"
        ),
    }
}
