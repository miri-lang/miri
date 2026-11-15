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
//! of that place reaching the call on every path, after the last write to
//! the place's local: a place handed over twice needs two, a retain followed
//! by a write to the place funds the value the write replaced, not the one
//! the call reads, a release of the place gives a retain back, and a retain
//! an earlier call handed over funds that call alone. Reference counting
//! places the retain in the block the call ends, and a pass splitting that
//! block keeps it on every path to the call, so the rule follows the paths
//! rather than the block. Without one, the callee's release frees a value
//! its holder still reads.
//!
//! The rule reads the body alone. A constant needs no retain (every constant
//! reaching a counted parameter is inert to counting), a donated value that
//! is not counted needs none, and a GPU body holds no counts.

use std::collections::HashMap;

use super::{local_display_name, VerificationViolation};
use crate::mir::analysis::traversal::successors_of;
use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::arg_mode;
use crate::mir::{ArgMode, BasicBlockData, Body, Local, Operand, Place, Statement, StatementKind};

/// Every argument a call or virtual call in `body` hands over by copy with
/// fewer retains of it reaching the call than the call hands it over. A
/// call no path reaches never runs and is not checked.
pub fn verify_owned_copies(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let mut reaching: HashMap<&Place, Vec<Option<usize>>> = HashMap::new();
    let mut violations = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        let copies = funded_copies(body, data);
        let first_copies = (copies.iter().enumerate())
            .filter(|&(index, &(_, place))| copies[..index].iter().all(|&(_, seen)| seen != place));
        for (_, &(position, place)) in first_copies {
            let handed = copies.iter().filter(|&&(_, other)| other == place).count();
            let on_entry = reaching
                .entry(place)
                .or_insert_with(|| retains_on_entry(body, place));
            let Some(entering) = on_entry.get(block).copied().flatten() else {
                continue;
            };
            if retains_at_terminator(data, place, entering) < handed {
                violations.push(unfunded_copy(body, place, block, position));
            }
        }
    }
    violations
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

/// For each block of `body`, how many retains of `place` reach its entry on
/// every path: the fewest any path leaves standing, `None` for a block no
/// path reaches. A release of the place gives one retain back, a write to
/// the place's local takes every standing retain back to none, and a call
/// handing the place over by copy spends one for each time it hands it
/// over, so a retain funds one hand-over, made after it and before the next
/// write or release.
fn retains_on_entry(body: &Body, place: &Place) -> Vec<Option<usize>> {
    let mut on_entry: Vec<Option<usize>> = vec![None; body.basic_blocks.len()];
    let Some(entry) = on_entry.first_mut() else {
        return on_entry;
    };
    *entry = Some(0);
    let is_counted = is_counted_place(body, place);
    let mut changed = true;
    while changed {
        changed = false;
        for (block, data) in body.basic_blocks.iter().enumerate() {
            let Some(entering) = on_entry[block] else {
                continue;
            };
            let leaving = retains_leaving(data, (place, is_counted), entering);
            for successor in successors_of(body, block) {
                let met = on_entry[successor].map_or(leaving, |known| known.min(leaving));
                if on_entry[successor] != Some(met) {
                    on_entry[successor] = Some(met);
                    changed = true;
                }
            }
        }
    }
    on_entry
}

/// The retains of `place` standing once `data` ends, `entering` standing at
/// its entry: those standing at its terminator, less one for each time the
/// terminator hands the place over by copy as [`funded_copies`] counts it
/// (`is_counted` says whether the place's value is counted), and none once
/// it writes the place's local.
fn retains_leaving(
    data: &BasicBlockData,
    (place, is_counted): (&Place, bool),
    entering: usize,
) -> usize {
    let Some(terminator) = data.terminator.as_ref() else {
        return retains_at_terminator(data, place, entering);
    };
    if (terminator.written_places()).any(|written| written.local == place.local) {
        return 0;
    }
    let (args, modes) = terminator.passed_arguments().unwrap_or((&[], &[]));
    let handed = (args.iter().enumerate())
        .filter(|&(_, arg)| matches!(arg, Operand::Copy(copied) if copied == place))
        .filter(|&(position, _)| match arg_mode(modes, position) {
            ArgMode::Owned => true,
            ArgMode::Donated => is_counted,
            ArgMode::Borrowed | ArgMode::Out => false,
        })
        .count();
    retains_at_terminator(data, place, entering).saturating_sub(handed)
}

/// The retains of `place` standing at `data`'s terminator, `entering`
/// standing at its entry: each retain of the place adds one, each release
/// of it gives one back, and a write to its local takes them all back.
fn retains_at_terminator(data: &BasicBlockData, place: &Place, entering: usize) -> usize {
    (data.statements.iter()).fold(entering, |standing, statement| {
        if writes_local(statement, place.local) {
            return 0;
        }
        match &statement.kind {
            StatementKind::IncRef(retained) if retained == place => standing + 1,
            StatementKind::DecRef(released) if released == place => standing.saturating_sub(1),
            StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(..)
            | StatementKind::Dealloc(_)
            | StatementKind::ReadHeader(_)
            | StatementKind::Nop => standing,
        }
    })
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
