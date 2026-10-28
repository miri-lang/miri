// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A lent argument read through a class field holds a reference of its own
//! for a call that may release it.
//!
//! A call lends its arguments, so the callee reads whatever reference holds
//! the argument in the caller. When that holder is a class field, or a place
//! reached through a dereference, the callee can reach it another way and
//! replace or empty it while it still reads the argument. Unless the call is
//! one [`crate::mir::analysis::heap_effects`] found quiet, each such argument
//! is read into a temporary before the call, which Perceus retains like any
//! copy of a counted field, and the temporary's storage ends after the call,
//! where Perceus releases it.
//!
//! An entry a runtime getter hands back as a borrow of a collection reached
//! that way (`m[k]` on a map a field holds) is held the same way.
//!
//! A receiver the callee writes in place as a value its caller copies on
//! write is left as it is: the caller made it unique for the call, and a
//! retain would make the callee find it shared. A receiver the callee only
//! reads — `map`, `filter`, `reduce` on a list a field holds — is held like
//! any other argument.

use crate::mir::analysis::heap_effects::HeapLends;
use crate::mir::block::{BasicBlock, BasicBlockData};
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{arg_mode, Terminator, TerminatorKind};
use crate::mir::{Body, LocalDecl, Operand, Place, Rvalue};

/// One argument read into a temporary for the call ending `block`.
struct HeldArgument {
    block: usize,
    position: usize,
    place: Place,
    decl: LocalDecl,
}

/// Read each lent argument a loud call is handed through a class field into
/// a temporary that lives across the call. Reports whether any was.
pub(super) fn hold_lent_field_arguments(body: &mut Body) -> bool {
    let held = held_arguments(body);
    if held.is_empty() {
        return false;
    }
    let predecessors = body.predecessor_counts();
    let mut temporaries: Vec<(usize, Vec<Place>)> = Vec::new();
    for argument in held {
        let span = argument.decl.span;
        let temporary = Place::new(body.new_local(argument.decl));
        let Some(block) = body.basic_blocks.get_mut(argument.block) else {
            continue;
        };
        block.statements.extend([
            statement(StatementKind::StorageLive(temporary.clone()), span),
            statement(
                StatementKind::Assign(
                    temporary.clone(),
                    Rvalue::Use(Operand::Copy(argument.place)),
                ),
                span,
            ),
        ]);
        if let Some(arg) = call_args(block.terminator.as_mut()).get_mut(argument.position) {
            *arg = Operand::Copy(temporary.clone());
        }
        match temporaries.last_mut() {
            Some((last, places)) if *last == argument.block => places.push(temporary),
            Some(_) | None => temporaries.push((argument.block, vec![temporary])),
        }
    }
    for (block, places) in temporaries {
        end_after_call(body, block, places, &predecessors);
    }
    true
}

/// Every argument of a call `body` makes that a loud callee may release
/// while it reads it, in block order.
fn held_arguments(body: &Body) -> Vec<HeldArgument> {
    let lends = HeapLends::of(body);
    let mut held = Vec::new();
    for (index, data) in body.basic_blocks.iter().enumerate() {
        let Some(terminator) = &data.terminator else {
            continue;
        };
        if body.quiet_calls.contains(&BasicBlock(index)) || call_target(terminator).is_none() {
            continue;
        }
        let Some((args, modes)) = lent_call_arguments(terminator) else {
            continue;
        };
        // TODO: a receiver kept here is lent unretained even when its callee
        // also runs code of the program that can free it — see
        // `heap_effects::writes_in_place_through` for why a retain cannot
        // close that.
        let keeps_receiver = body.unique_receiver_calls.contains(&BasicBlock(index));
        for (position, arg) in args.iter().enumerate() {
            if position == 0 && keeps_receiver {
                continue;
            }
            let mode = arg_mode(modes, position);
            let Some(place) = lends.lent_place(mode, arg) else {
                continue;
            };
            let Some(ty) = lends.declared(place) else {
                continue;
            };
            held.push(HeldArgument {
                block: index,
                position,
                place: place.clone(),
                decl: LocalDecl::new(ty, terminator.span),
            });
        }
    }
    held
}

/// The arguments and their modes of a call that lends them to code of the
/// program; `None` for a call into the runtime and any other terminator.
fn lent_call_arguments(terminator: &Terminator) -> Option<(&[Operand], &[crate::mir::ArgMode])> {
    match &terminator.kind {
        TerminatorKind::Call {
            func,
            args,
            arg_modes,
            ..
        } => (func.called_runtime_name().is_none()).then_some((args, arg_modes)),
        TerminatorKind::VirtualCall {
            args, arg_modes, ..
        } => Some((args, arg_modes)),
        TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => None,
    }
}

/// The arguments of the call `terminator` ends with, to rewrite.
fn call_args(terminator: Option<&mut Terminator>) -> &mut [Operand] {
    match terminator.map(|t| &mut t.kind) {
        Some(TerminatorKind::Call { args, .. } | TerminatorKind::VirtualCall { args, .. }) => args,
        Some(
            TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable,
        )
        | None => &mut [],
    }
}

/// The block a call returns to; `None` for a call that never returns and for
/// any other terminator.
fn call_target(terminator: &Terminator) -> Option<BasicBlock> {
    match &terminator.kind {
        TerminatorKind::Call { target, .. } | TerminatorKind::VirtualCall { target, .. } => *target,
        TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => None,
    }
}

/// End the storage of `places` where the call ending `block` returns to: at
/// the top of its target when the call is the target's only way in, and in a
/// block of their own between the two otherwise.
fn end_after_call(body: &mut Body, block: usize, places: Vec<Place>, predecessors: &[usize]) {
    let Some(terminator) = body
        .basic_blocks
        .get(block)
        .and_then(|b| b.terminator.as_ref())
    else {
        return;
    };
    let Some(target) = call_target(terminator) else {
        return;
    };
    let span = terminator.span;
    let ends: Vec<Statement> = places
        .into_iter()
        .map(|place| statement(StatementKind::StorageDead(place), span))
        .collect();
    if predecessors.get(target.0) == Some(&1) && target.0 != block {
        if let Some(data) = body.basic_blocks.get_mut(target.0) {
            data.statements.splice(0..0, ends);
        }
        return;
    }
    let landing = BasicBlock(body.basic_blocks.len());
    body.basic_blocks.push(BasicBlockData {
        statements: ends,
        terminator: Some(Terminator {
            kind: TerminatorKind::Goto { target },
            span,
        }),
        is_cleanup: false,
    });
    if let Some(TerminatorKind::Call { target, .. } | TerminatorKind::VirtualCall { target, .. }) =
        body.basic_blocks
            .get_mut(block)
            .and_then(|b| b.terminator.as_mut())
            .map(|t| &mut t.kind)
    {
        *target = Some(landing);
    }
}

fn statement(kind: StatementKind, span: crate::error::syntax::Span) -> Statement {
    Statement { kind, span }
}
