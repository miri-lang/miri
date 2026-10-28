// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that no call which may release what its caller lends it is lent a
//! value held only through a class field or a dereference.
//!
//! A callee that reaches the field another way — through the instance passed
//! beside it, its receiver, a capture — can replace or empty it while it still
//! reads the argument. Reference counting reads such an argument into a local
//! holding a reference of its own before the call, unless the call is quiet;
//! this rule finds one it missed. Which calls are quiet is worked out again
//! here, over the program as reference counting left it
//! ([`crate::mir::analysis::heap_effects`], read in [`Phase::Counted`]), not
//! read from what the pass recorded.
//!
//! An entry a runtime getter borrowed from a collection held that way counts
//! as held through the field.
//!
//! A call into the runtime is not checked: what it does with each argument is
//! its own contract. Nor is a receiver the callee writes in place as a value
//! its caller copies on write, which the caller made unique for the call; the
//! callee's body says whether it does.

use super::{local_display_name, VerificationViolation};
use crate::mir::analysis::heap_effects::{CallFacts, HeapEffects, HeapLends, Phase};
use crate::mir::terminator::{arg_mode, ArgMode, Terminator, TerminatorKind};
use crate::mir::type_facts::DropHookReach;
use crate::mir::{BasicBlock, Body, Operand};

/// Every argument a call in `program` lends unretained through a class field
/// to a callee that may release it, paired with the body making the call.
/// `link_name` names a body the way a call to it names its callee.
pub fn verify_lent_arguments<N: Clone>(
    program: &[(N, Body)],
    link_name: impl Fn(&N) -> String,
    hooks: &DropHookReach,
) -> Vec<(N, VerificationViolation)> {
    let effects = HeapEffects::of(program, link_name, Phase::Counted, hooks);
    program
        .iter()
        .filter(|(_, body)| !body.is_gpu())
        .flat_map(|(name, body)| {
            let facts = effects.call_facts(body, Phase::Counted, hooks);
            body_violations(body, &facts)
                .into_iter()
                .map(move |violation| (name.clone(), violation))
        })
        .collect()
}

/// The violations one body raises, its calls judged by `facts`.
fn body_violations(body: &Body, facts: &CallFacts) -> Vec<VerificationViolation> {
    let lends = HeapLends::of(body);
    let mut found = Vec::new();
    for (index, block) in body.basic_blocks.iter().enumerate() {
        let Some(terminator) = &block.terminator else {
            continue;
        };
        if facts.quiet.contains(&BasicBlock(index)) {
            continue;
        }
        let keeps_receiver = facts.unique_receivers.contains(&BasicBlock(index));
        let (args, modes) = lent_arguments(terminator);
        for (position, arg) in args.iter().enumerate() {
            if position == 0 && keeps_receiver {
                continue;
            }
            let Some(place) = lends.lent_place(arg_mode(modes, position), arg) else {
                continue;
            };
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
