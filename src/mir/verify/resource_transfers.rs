// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that a resource a body owns is handed over by `move`, never funded.
//!
//! A call passing an argument [`ArgMode::Owned`] gives the callee a reference
//! it releases. When the argument is a binding of the body's own whose
//! release may run a drop hook, the caller gives up its binding at the call:
//! handing over a copy with a retain beside it leaves the caller a reference
//! it releases later, so the hook runs where the caller's binding ends rather
//! than where the callee's new owner does. The hand-over is spelled
//! `move x`, and the binding's end after the call releases nothing.
//!
//! A finding is a call passing, at an owned position, a copy of a whole user
//! binding the body owns — no parameter, no capture, no borrow — whose type
//! may run a drop hook. The rule reads the body, its ownership facts and the
//! drop-hook table alone. A GPU body is not checked: it holds no counts.

use super::{local_display_name, VerificationViolation};
use crate::mir::facts::OwnershipFacts;
use crate::mir::terminator::arg_mode;
use crate::mir::type_facts::DropHookReach;
use crate::mir::{ArgMode, Body, Local, Operand, TerminatorKind};

/// Every resource binding `body` owns that a call hands over by copy.
pub fn verify_resource_transfers(
    body: &Body,
    ownership: &OwnershipFacts,
    hooks: &DropHookReach,
) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let mut violations = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        let Some(TerminatorKind::Call {
            args, arg_modes, ..
        }) = data.terminator.as_ref().map(|terminator| &terminator.kind)
        else {
            continue;
        };
        for (position, arg) in args.iter().enumerate() {
            let Operand::Copy(place) = arg else {
                continue;
            };
            let is_owned = arg_mode(arg_modes, position) == ArgMode::Owned;
            if is_owned
                && place.projection.is_empty()
                && is_owned_resource_binding(body, ownership, hooks, place.local)
            {
                violations.push(funded_transfer(body, place.local, block, position));
            }
        }
    }
    violations
}

/// Whether `local` is a whole user binding `body` owns whose release may run
/// a drop hook.
fn is_owned_resource_binding(
    body: &Body,
    ownership: &OwnershipFacts,
    hooks: &DropHookReach,
    local: Local,
) -> bool {
    let decl = &body.local_decls[local.0];
    local.0 > body.arg_count
        && decl.is_user_variable
        && !body.captures.env_capture_locals.contains(&local)
        && !ownership.is_borrowed(local)
        && body.is_counted_local(local)
        && hooks.may_run_drop_hook(&decl.ty)
}

fn funded_transfer(
    body: &Body,
    local: Local,
    block: usize,
    position: usize,
) -> VerificationViolation {
    VerificationViolation {
        local,
        local_name: local_display_name(body, local),
        message: format!(
            "handed over by copy at argument {position} of the call ending bb{block}; a \
             resource the body owns is handed over by `move`, so the callee holds its only \
             reference"
        ),
    }
}
