// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every value held as its bytes a call takes lies in bytes the
//! call does not write, and is never handed over owned.
//!
//! A value argument is passed as the address of the caller's own bytes, which
//! the callee only reads. An argument read from a local the same call writes
//! as its destination or through an `out` argument, or through a projection
//! not at a fixed offset of its root value local, would let those bytes
//! change while the callee reads them; the copy pass ([`crate::mir::value_arguments`])
//! hands each such argument a copy, and this rule finds one it missed. A
//! field of a value local the call does not write is passed in place. A
//! write a callee makes to its own value parameter is not a finding: the
//! callee copies that parameter on entry.
//!
//! Such a value has no count, so no call takes one over: an argument passed
//! [`ArgMode::Owned`] is always a counted value.

use super::{local_display_name, VerificationViolation};
use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::value_arguments::{argument_place, value_arguments_needing_copy, Exposure};
use crate::mir::{Body, Place};

/// Every value argument a call in `body` takes from inside a holder, not at
/// a fixed offset of its root value local, or from a local the call also
/// writes, and every one it passes owned.
///
/// A GPU body is skipped: its calls pass no value by address.
pub fn verify_value_arguments(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let mut violations: Vec<VerificationViolation> = (0..body.basic_blocks.len())
        .flat_map(|block| value_arguments_needing_copy(body, block))
        .map(|argument| {
            violation(
                body,
                argument.position,
                argument.place,
                describe(argument.exposure),
            )
        })
        .collect();
    violations.extend(owned_value_violations(body));
    violations
}

/// Every value held as its bytes a call in `body` passes `ArgMode::Owned`.
fn owned_value_violations(body: &Body) -> Vec<VerificationViolation> {
    let types = PlaceTypes::of(body);
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                args, arg_modes, ..
            }
            | TerminatorKind::VirtualCall {
                args, arg_modes, ..
            } => Some((args, arg_modes)),
            TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
        })
        .flat_map(|(args, arg_modes)| {
            args.iter()
                .enumerate()
                .filter(|(position, _)| arg_mode(arg_modes, *position) == ArgMode::Owned)
                .filter_map(|(position, arg)| Some((position, argument_place(arg)?)))
                .filter(|(_, place)| {
                    types
                        .declared(place)
                        .is_some_and(|declared| body.value_layout(&declared.kind).is_some())
                })
                .map(|(position, place)| {
                    violation(
                        body,
                        position,
                        place,
                        "passed owned, but it has no count to hand over",
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The finding for argument `position`, read from `place`, being `what`.
fn violation(body: &Body, position: usize, place: &Place, what: &str) -> VerificationViolation {
    VerificationViolation {
        local: place.local,
        local_name: local_display_name(body, place.local),
        message: format!("argument {position}, `{place}`, a value held as its bytes, is {what}"),
    }
}

/// Why the argument's bytes could change while the callee reads them.
fn describe(exposure: Exposure) -> &'static str {
    match exposure {
        Exposure::Projected => {
            "passed by address from inside its holder, not at a fixed offset of its root value local"
        }
        Exposure::Destination => {
            "passed by address from the local the call also writes as its destination"
        }
        Exposure::Out => {
            "passed by address from a local the call also writes through an `out` argument"
        }
    }
}
