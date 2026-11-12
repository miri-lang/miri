// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The rules that a call through a vtable slot and every body the slot
//! reaches agree on what the call hands over.
//!
//! A slot has one convention, the positions it owns in operand order with
//! the receiver first, read at each side's own types: a position is handed
//! over only where its value is counted. The rules re-derive it from that
//! table alone, sharing nothing else with the lowering that writes it:
//! - every virtual call passes a position `Owned` exactly where the slot owns
//!   it at the operand's type, so two calls through one slot never disagree;
//! - every body a slot is filled with owns exactly what the slot owns at the
//!   body's parameter types, so no reference is released twice or never;
//! - every body a slot is filled with is a body of the program, unless the
//!   slot is withheld at its instance and keeps the runtime trap;
//! - an adapter a slot reaches a body through makes one call, and that call
//!   is to the body it adapts, and owns every parameter the body keeps, so
//!   it never has to retain.

use std::collections::{HashMap, HashSet};

use crate::ast::types::Type;
use crate::mir::dispatch::VtableFills;
use crate::mir::facts::ProgramFacts;
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::{arg_mode, ArgMode, Body, Local, Operand, TerminatorKind, RETURN_LOCAL};

use super::VerificationViolation;

/// What the rules read besides the bodies: the convention of every slot
/// owning a position, by slot number, and the body each vtable's slots are
/// filled with.
pub struct SlotSpecification<'a> {
    /// The positions each slot owns, in operand order with the receiver
    /// first, at the types its bodies declare; a slot absent lends every
    /// position.
    pub conventions: &'a HashMap<usize, Vec<bool>>,
    /// The body every filled slot of every vtable points at.
    pub fills: &'a VtableFills,
    /// The bodies withheld at their instance, whose slots keep the runtime
    /// trap and name no body of the program.
    pub withheld: &'a HashSet<Symbol>,
}

impl SlotSpecification<'_> {
    fn convention(&self, slot: usize) -> &[bool] {
        self.conventions.get(&slot).map_or(&[], Vec::as_slice)
    }
}

/// Each virtual call, filled slot and slot adapter of `program` breaking a
/// rule above, named by the body it is found in; `facts` runs parallel to
/// `program`.
pub fn verify_slot_conventions(
    program: &[(Symbol, Body)],
    facts: &ProgramFacts,
    specification: &SlotSpecification<'_>,
) -> Vec<(Symbol, VerificationViolation)> {
    let owned: HashMap<&Symbol, (&Body, &[bool])> = (program.iter().zip(facts.iter()))
        .map(|((symbol, body), facts)| {
            let owned_params = facts.ownership().owned_params.as_slice();
            (symbol, (body, owned_params))
        })
        .collect();
    let mut found: Vec<(Symbol, VerificationViolation)> = (program.iter())
        .flat_map(|(symbol, body)| {
            let calls = virtual_call_violations(body, specification);
            let adapter = adapter_violation(symbol, body, &owned);
            (calls.into_iter().chain(adapter)).map(move |violation| (symbol.clone(), violation))
        })
        .collect();
    found.extend(filled_slot_violations(&owned, specification));
    found
}

/// What each body of the program owns, by its symbol: the body and its
/// owned parameters, in operand order.
type OwnedBodies<'p> = HashMap<&'p Symbol, (&'p Body, &'p [bool])>;

/// Whether a slot owning `convention` hands over the position `position` of
/// a value of `ty` in `body`: where it owns it and the value is counted.
fn is_handed(body: &Body, convention: &[bool], position: usize, ty: Option<&Type>) -> bool {
    let is_owned = convention.get(position).copied().unwrap_or(false);
    is_owned && !body.is_gpu() && ty.is_some_and(|ty| body.is_counted(&ty.kind))
}

/// Each argument of a virtual call in `body` passed `Owned` where its slot
/// lends it, or passed otherwise where its slot owns it.
fn virtual_call_violations(
    body: &Body,
    specification: &SlotSpecification<'_>,
) -> Vec<VerificationViolation> {
    let types = PlaceTypes::of(body);
    (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| {
            let TerminatorKind::VirtualCall {
                vtable_slot,
                args,
                arg_modes,
                ..
            } = &terminator.kind
            else {
                return None;
            };
            Some((*vtable_slot, args, arg_modes))
        })
        .flat_map(|(slot, args, modes)| {
            let convention = specification.convention(slot);
            let types = &types;
            (args.iter().enumerate()).filter_map(move |(position, arg)| {
                let ty = types.operand_type(arg);
                let is_owned = arg_mode(modes, position) == ArgMode::Owned;
                let should_own = is_handed(body, convention, position, ty.as_ref());
                (is_owned != should_own).then(|| call_violation(slot, position, arg, should_own))
            })
        })
        .collect()
}

fn call_violation(
    slot: usize,
    position: usize,
    arg: &Operand,
    should_own: bool,
) -> VerificationViolation {
    let local = arg.bare_local().unwrap_or(RETURN_LOCAL);
    let (passed, convention) = if should_own {
        ("lends", "owns")
    } else {
        ("hands over", "lends")
    };
    VerificationViolation {
        local,
        local_name: format!("{local}"),
        message: format!(
            "a call through vtable slot {slot} {passed} argument {position}, which the slot \
             {convention} at its type, so the body it reaches would release a reference \
             the caller still holds or one nobody releases"
        ),
    }
}

/// Each body a filled slot points at that does not own exactly what the
/// slot owns at the body's parameter types, and each filled slot naming no
/// body of the program, named by the symbol the slot holds.
fn filled_slot_violations(
    owned: &OwnedBodies<'_>,
    specification: &SlotSpecification<'_>,
) -> Vec<(Symbol, VerificationViolation)> {
    let mut found: Vec<(Symbol, VerificationViolation)> = (specification.fills.iter())
        .flat_map(|(vtable, slots)| slots.iter().map(move |filled| (vtable, filled)))
        .filter(|(_, filled)| !specification.withheld.contains(&filled.symbol))
        .filter_map(|(vtable, filled)| {
            let slot = (vtable, filled.slot);
            let violation = match owned.get(&filled.symbol) {
                None => Some(missing_body(slot, &filled.symbol)),
                Some((body, owned_params)) => {
                    let convention = specification.convention(filled.slot);
                    filled_body_violation(body, owned_params, convention)
                        .map(|disagreement| fill_violation(slot, disagreement))
                }
            };
            violation.map(|violation| (filled.symbol.clone(), violation))
        })
        .collect();
    found.sort_by_cached_key(|(symbol, violation)| (symbol.to_string(), violation.message.clone()));
    found.dedup_by(|(a, x), (b, y)| a == b && x.message == y.message);
    found
}

/// The first parameter position at which `body`, owning `owned_params`,
/// disagrees with a slot owning `convention`, with whether the body owns it.
fn filled_body_violation(
    body: &Body,
    owned_params: &[bool],
    convention: &[bool],
) -> Option<(usize, bool)> {
    (0..body.arg_count)
        .map(|position| {
            let ty = body.local_decls.get(position + 1).map(|decl| &decl.ty);
            let is_owned = owned_params.get(position).copied().unwrap_or(false);
            (
                position,
                is_owned,
                is_handed(body, convention, position, ty),
            )
        })
        .find(|(_, is_owned, is_handed)| is_owned != is_handed)
        .map(|(position, is_owned, _)| (position, is_owned))
}

fn fill_violation(
    (vtable, slot): (&str, usize),
    (position, is_owned): (usize, bool),
) -> VerificationViolation {
    let local = Local(position + 1);
    let message = if is_owned {
        format!(
            "fills slot {slot} of `{vtable}` but owns parameter {position}, which the slot \
             lends, so it releases a reference every caller through the slot still holds"
        )
    } else {
        format!(
            "fills slot {slot} of `{vtable}` but lends parameter {position}, which the slot \
             owns, so the reference every call through the slot hands it is never released"
        )
    };
    VerificationViolation {
        local,
        local_name: format!("{local}"),
        message,
    }
}

fn missing_body((vtable, slot): (&str, usize), symbol: &Symbol) -> VerificationViolation {
    VerificationViolation {
        local: RETURN_LOCAL,
        local_name: format!("{RETURN_LOCAL}"),
        message: format!(
            "fills slot {slot} of `{vtable}`, yet `{symbol}` is no body of the program and the \
             slot is not withheld, so no rule can check what a call through it hands over"
        ),
    }
}

/// The finding for `body` when `symbol` names a slot adapter that does not
/// make exactly one call, to the body it adapts, or that owns less than that
/// body keeps.
fn adapter_violation(
    symbol: &Symbol,
    body: &Body,
    owned: &OwnedBodies<'_>,
) -> Option<VerificationViolation> {
    let (target, adapter_owns) = symbol.slot_adapter_target()?;
    let callees: Vec<Callee<'_>> = (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| program_callee(&terminator.kind))
        .collect();
    let message = match callees.as_slice() {
        [Callee::Program(callee)] if *callee == target => {
            let target_keeps = owned.get(target).map_or(&[][..], |(_, kept)| *kept);
            let retained = (target_keeps.iter().enumerate())
                .find(|(position, is_kept)| {
                    **is_kept && !adapter_owns.get(*position).copied().unwrap_or(false)
                })?
                .0;
            format!(
                "is the vtable slot adapter of `{target}` but does not own parameter \
                 {retained}, which `{target}` keeps, so it could hand it over only by a retain"
            )
        }
        [] => format!("is the vtable slot adapter of `{target}` but never calls it"),
        [callee] => format!(
            "is the vtable slot adapter of `{target}` but calls {callee} instead, a body the \
             slot was never filled with"
        ),
        more => format!(
            "is the vtable slot adapter of `{target}` but makes {} calls, where it calls \
             `{target}` exactly once",
            more.len()
        ),
    };
    Some(VerificationViolation {
        local: RETURN_LOCAL,
        local_name: format!("{RETURN_LOCAL}"),
        message,
    })
}

/// A body of the program a terminator runs.
enum Callee<'b> {
    /// A function named by its symbol.
    Program(&'b Symbol),
    /// Whatever body a vtable slot holds.
    Vtable(usize),
    /// Whatever body a function value holds.
    Value,
}

impl std::fmt::Display for Callee<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Callee::Program(symbol) => write!(f, "`{symbol}`"),
            Callee::Vtable(slot) => write!(f, "vtable slot {slot}"),
            Callee::Value => write!(f, "a function value"),
        }
    }
}

/// The body of the program a terminator calls: `None` for a terminator that
/// calls nothing and for a call into the runtime library, which releases or
/// copies and runs no body of the program.
fn program_callee(kind: &TerminatorKind) -> Option<Callee<'_>> {
    if let TerminatorKind::VirtualCall { vtable_slot, .. } = kind {
        return Some(Callee::Vtable(*vtable_slot));
    }
    let TerminatorKind::Call { func, .. } = kind else {
        return None;
    };
    match func.called_symbol() {
        Some(callee) if callee.is_runtime() => None,
        Some(callee) => Some(Callee::Program(callee)),
        None => Some(Callee::Value),
    }
}
