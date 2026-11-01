// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that no call which may release what its caller lends it is lent a
//! value held only through a class field or a dereference.
//!
//! A callee that reaches the field another way — through the instance passed
//! beside it, its receiver, a capture — can replace or empty it while it still
//! reads the argument. Reference counting reads such an argument into a local
//! holding a reference of its own before the call, unless the call is quiet;
//! this rule finds one it missed. Which calls cannot release it is decided by
//! the verifiers' own rule ([`super::callee_effects`]), never by the analysis
//! the pass asked, and which arguments are held that way is worked out here,
//! more broadly than the pass does: any step this check does not follow
//! counts as one through a field.
//!
//! An entry a runtime getter lends out of a collection counts as held through
//! a field unless the collection is a whole local the body owns.
//!
//! A call into the runtime is not checked: what it does with each argument is
//! its own contract. Nor is a receiver the callee writes in place, by the
//! verifiers' own reading of the callee, as a value its caller copies on
//! write: the caller made it unique for the call.

use super::borrow_reach::{reach_of, Reach};
use super::callee_effects::HarmlessCallees;
use super::{local_display_name, VerificationViolation};
use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::{arg_mode, ArgMode, Terminator, TerminatorKind};
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::type_facts::DropHookReach;
use crate::mir::{BasicBlock, Body, Local, Operand, Place};
use crate::runtime_effects::RuntimeEffect;
use std::collections::HashSet;

/// Every argument a call in `program` lends unretained through a class field
/// to a callee that may release it, paired with the body making the call.
/// `link_name` names a body the way a call to it names its callee.
pub fn verify_lent_arguments<N: Clone>(
    program: &[(N, Body)],
    link_name: impl Fn(&N) -> String,
    hooks: &DropHookReach,
) -> Vec<(N, VerificationViolation)> {
    let harmless = HarmlessCallees::of(program, link_name, hooks);
    program
        .iter()
        .filter(|(_, body)| !body.is_gpu())
        .flat_map(|(name, body)| {
            let receiver_writers = harmless.receiver_writing_calls_in(body);
            body_violations(body, &harmless.calls_in(body), &receiver_writers)
                .into_iter()
                .map(move |violation| (name.clone(), violation))
        })
        .collect()
}

/// The violations one body raises, its calls to harmless callees skipped,
/// and the receiver of a call to a method writing it in place exempt.
fn body_violations(
    body: &Body,
    harmless_calls: &HashSet<BasicBlock>,
    receiver_writers: &HashSet<BasicBlock>,
) -> Vec<VerificationViolation> {
    let types = PlaceTypes::of(body);
    let entries = reachable_entries(body);
    let mut found = Vec::new();
    for (index, block) in body.basic_blocks.iter().enumerate() {
        let Some(terminator) = &block.terminator else {
            continue;
        };
        if harmless_calls.contains(&BasicBlock(index)) {
            continue;
        }
        let writes_receiver = receiver_writers.contains(&BasicBlock(index));
        let (args, modes) = lent_arguments(terminator);
        for (position, arg) in args.iter().enumerate() {
            if arg_mode(modes, position) != ArgMode::Borrowed {
                continue;
            }
            let Some(place) = arg.place() else {
                continue;
            };
            let declared = types.declared(place);
            let made_unique = position == 0
                && writes_receiver
                && declared
                    .as_ref()
                    .is_some_and(|ty| is_copied_on_write(body, ty));
            let is_counted = declared.is_some_and(|ty| body.is_counted(&ty.kind));
            if made_unique {
                continue;
            }
            if !is_counted || !(is_held_by_field(body, place) || entries.contains(&place.local)) {
                continue;
            }
            found.push(VerificationViolation {
                local: place.local,
                local_name: local_display_name(body, place.local),
                message: format!(
                    "argument {position}, `{place}`, is lent through a class field to a call \
                     that may release it, without a reference of its own"
                ),
            });
        }
    }
    found
}

/// Whether `place` is reached through a class field, a dereference, or a
/// step this check does not follow.
fn is_held_by_field(body: &Body, place: &Place) -> bool {
    !place.projection.is_empty() && reach_of(body, place) != Some(Reach::InValue)
}

/// The locals of `body` holding an entry a runtime getter lends out of a
/// collection other code may reach: one held through a field, or by a
/// parameter or a capture.
fn reachable_entries(body: &Body) -> HashSet<Local> {
    let terminators = body
        .basic_blocks
        .iter()
        .filter_map(|data| data.terminator.as_ref());
    terminators
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } if destination.projection.is_empty()
                && (func.called_runtime_effect())
                    .is_some_and(RuntimeEffect::hands_back_a_borrow) =>
            {
                Some((destination.local, args.first().and_then(Operand::place)))
            }
            TerminatorKind::Call { .. }
            | TerminatorKind::VirtualCall { .. }
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
        })
        .filter(|(_, collection)| {
            collection.is_none_or(|place| {
                is_held_by_field(body, place)
                    || (1..=body.arg_count).contains(&place.local.0)
                    || body.env_capture_locals.contains(&place.local)
            })
        })
        .map(|(entry, _)| entry)
        .collect()
}

/// The arguments a call lends to code of the program, with their modes;
/// none for a call into the runtime or any other terminator.
fn lent_arguments(terminator: &Terminator) -> (&[Operand], &[ArgMode]) {
    match &terminator.kind {
        TerminatorKind::Call {
            func,
            args,
            arg_modes,
            ..
        } if func.called_runtime_name().is_none() => (args, arg_modes),
        TerminatorKind::VirtualCall {
            args, arg_modes, ..
        } => (args, arg_modes),
        TerminatorKind::Call { .. }
        | TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => (&[], &[]),
    }
}
