// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks the one calling convention of a function value.
//!
//! A call through a function value or a closure cannot see which body it
//! reaches, so every such call and every body such a call reaches agree on
//! one convention, read off the function type alone: the call passes an
//! argument [`ArgMode::Owned`] exactly where the declared parameter type
//! runs a drop hook of its own (`type_checker::utils::hands_on_a_reference`),
//! and every closure body — a lambda, a nested function, the forwarding body
//! of a function named as a value — owns exactly those parameters on its
//! signature. A call lending a position the body owns releases the argument
//! twice; one handing over a position the body lends leaks it.
//!
//! A runtime function calling a closure (`@closure`) lends it every
//! argument, so a closure handed to one either owns nothing by its type or
//! is an adapter built where the call is made: a closure value created in
//! the block the call ends, whose own body lends every parameter. Every other
//! closure body follows its type's convention.
//!
//! The rule reads the bodies, the type of the value each call is made
//! through and the type table alone, sharing only the drop-hook predicate
//! with the lowering it checks. A GPU body is not checked: it holds no
//! counts.

use std::collections::{HashMap, HashSet};

use super::{local_display_name, VerificationViolation};
use crate::ast::expression::ExpressionKind;
use crate::ast::types::{FunctionTypeData, TypeKind};
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::terminator::arg_mode;
use crate::mir::{
    AggregateKind, ArgMode, BasicBlockData, Body, Local, Operand, Place, Rvalue, StatementKind,
    TerminatorKind,
};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::utils::{hands_on_a_reference, ResourceTypes};

/// The local a closure body's environment arrives in: its first parameter.
const ENVIRONMENT: Local = Local(1);

/// The type table conventions are read against, with the types that are
/// resources decided once over it.
#[derive(Clone, Copy)]
struct Table<'d> {
    definitions: &'d HashMap<String, TypeDefinition>,
    resources: &'d ResourceTypes,
}

impl Table<'_> {
    /// Whether consuming a value of `kind` hands its reference on
    /// ([`hands_on_a_reference`]).
    fn hands_on(&self, kind: &TypeKind) -> bool {
        hands_on_a_reference(kind, self.definitions, self.resources)
    }
}

/// Every call through a function value among `bodies` whose owned positions
/// differ from its value's type; every closure owning a parameter handed to
/// a runtime closure slot unadapted; and every closure body whose signature
/// owns other parameters than its convention: none for an adapter a runtime
/// slot calls, its type's for any other. Each violation names its body.
pub fn verify_value_calls(
    bodies: &[(String, Body)],
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<(String, VerificationViolation)> {
    let resources = ResourceTypes::of(definitions);
    let table = Table {
        definitions,
        resources: &resources,
    };
    let host = || bodies.iter().filter(|(_, body)| !body.is_gpu());
    let mut adapters = HashSet::new();
    let mut violations: Vec<(String, VerificationViolation)> = Vec::new();
    for (name, body) in host() {
        let found = slot_violations(body, table, &mut adapters);
        violations.extend(found.into_iter().map(|found| (name.clone(), found)));
    }
    for (name, body) in host() {
        let found = body_violations(body, table, &adapters);
        violations.extend(found.into_iter().map(|found| (name.clone(), found)));
    }
    violations
}

/// Every closure owning a parameter that `body` hands a runtime closure slot
/// other than as an adapter it creates, recording each adapter's symbol in
/// `adapters`.
fn slot_violations(
    body: &Body,
    table: Table<'_>,
    adapters: &mut HashSet<Symbol>,
) -> Vec<VerificationViolation> {
    let types = PlaceTypes::of(body);
    let mut violations = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        let Some(TerminatorKind::Call { func, args, .. }) =
            data.terminator.as_ref().map(|terminator| &terminator.kind)
        else {
            continue;
        };
        let calls_a_closure = (func.called_runtime_effect())
            .is_some_and(|effect| !effect.closure_positions().is_empty());
        if !calls_a_closure {
            continue;
        }
        for (position, place) in args
            .iter()
            .enumerate()
            .filter_map(|(at, arg)| Some((at, arg.place()?)))
        {
            let Some(TypeKind::Function(function)) = types.declared(place).map(|ty| ty.kind) else {
                continue;
            };
            if !convention_of(&function, table).contains(&true) {
                continue;
            }
            match created_closure(data, place) {
                Some(symbol) => {
                    adapters.insert(symbol.clone());
                }
                None => violations.push(unadapted_violation(body, place.local, block, position)),
            }
        }
    }
    violations
}

/// The closure the block a call ends, `block`, creates in the whole local
/// `place` names ahead of the call, if it creates one there. Lowering builds
/// an adapter in the block its call ends, so one created anywhere else is
/// not the adapter of this call.
fn created_closure<'b>(block: &'b BasicBlockData, place: &Place) -> Option<&'b Symbol> {
    if !place.projection.is_empty() {
        return None;
    }
    block.statements.iter().find_map(|statement| {
        let StatementKind::Assign(dest, Rvalue::Aggregate(AggregateKind::Closure(symbol, _), _)) =
            &statement.kind
        else {
            return None;
        };
        (dest == place).then_some(symbol)
    })
}

/// Every call through a function value in `body` whose owned positions
/// differ from its value's type, and, for a closure body, a signature owning
/// other parameters than its convention.
fn body_violations(
    body: &Body,
    table: Table<'_>,
    adapters: &HashSet<Symbol>,
) -> Vec<VerificationViolation> {
    let own = own_convention(body, table);
    let is_adapter =
        (body.captures.closure.as_ref()).is_some_and(|symbol| adapters.contains(symbol));
    let expected_signature = own.as_ref().map(|own| {
        if is_adapter {
            vec![false; own.len()]
        } else {
            own.clone()
        }
    });
    let mut violations: Vec<VerificationViolation> = expected_signature
        .as_ref()
        .and_then(|expected| signature_violation(body, expected, is_adapter))
        .into_iter()
        .collect();
    let types = PlaceTypes::of(body);
    for (block, data) in body.basic_blocks.iter().enumerate() {
        let Some(TerminatorKind::Call {
            func: Operand::Copy(callee) | Operand::Move(callee),
            arg_modes,
            args,
            destination,
            ..
        }) = data.terminator.as_ref().map(|terminator| &terminator.kind)
        else {
            continue;
        };
        let expected = called_convention(callee, &types, own.as_deref(), table);
        let mismatch = (0..args.len()).find(|&position| {
            let is_owned = arg_mode(arg_modes, position) == ArgMode::Owned;
            is_owned != expected.get(position).copied().unwrap_or(false)
        });
        if let Some(position) = mismatch {
            violations.push(call_violation(body, destination.local, block, position));
        }
    }
    violations
}

/// The parameters a closure body owns by its own type, its environment
/// first and never owned; `None` for a body that is no closure's.
fn own_convention(body: &Body, table: Table<'_>) -> Option<Vec<bool>> {
    body.captures.closure.as_ref()?;
    let owns = |index: usize| {
        let is_out = body.signature.out_params.get(index - 1) == Some(&true);
        !is_out && table.hands_on(&body.local_decls[index].ty.kind)
    };
    Some(
        std::iter::once(false)
            .chain((2..=body.arg_count).map(owns))
            .collect(),
    )
}

/// Which positions a call through `callee` owns: each one its function type
/// hands a reference on at. A closure body calling itself through its own
/// environment reaches itself, so it follows the body's own convention,
/// `own`, its environment aside.
fn called_convention(
    callee: &Place,
    types: &PlaceTypes<'_>,
    own: Option<&[bool]>,
    table: Table<'_>,
) -> Vec<bool> {
    if let (Some(own), true) = (own, *callee == Place::new(ENVIRONMENT)) {
        return own.iter().skip(1).copied().collect();
    }
    let Some(ty) = types.declared(callee) else {
        return Vec::new();
    };
    let TypeKind::Function(function) = &ty.kind else {
        return Vec::new();
    };
    convention_of(function, table)
}

/// Each parameter of `function` whose declared type hands its reference on:
/// not `out`, not optional, and of a type running a drop hook of its own.
fn convention_of(function: &FunctionTypeData, table: Table<'_>) -> Vec<bool> {
    (function.params.iter())
        .map(|param| {
            let ExpressionKind::Type(declared, false) = &param.typ.node else {
                return false;
            };
            !param.is_out && table.hands_on(&declared.kind)
        })
        .collect()
}

/// The violation of a closure body whose signature owns other parameters
/// than its convention, `own`: none for an adapter a runtime closure slot
/// calls, `is_adapter`, its type's for any other.
fn signature_violation(
    body: &Body,
    own: &[bool],
    is_adapter: bool,
) -> Option<VerificationViolation> {
    let kept = &body.signature.kept_params;
    let agrees = (0..own.len().max(kept.len())).all(|index| {
        own.get(index).copied().unwrap_or(false) == kept.get(index).copied().unwrap_or(false)
    });
    if agrees {
        return None;
    }
    Some(VerificationViolation {
        local: Local(0),
        local_name: local_display_name(body, Local(0)),
        message: if is_adapter {
            format!(
                "is handed to a runtime closure slot, which lends every argument, yet its \
                 signature owns {kept:?}"
            )
        } else {
            format!(
                "is a closure body whose convention owns the parameters {own:?}, yet its \
                 signature owns {kept:?}"
            )
        },
    })
}

fn call_violation(
    body: &Body,
    destination: Local,
    block: usize,
    position: usize,
) -> VerificationViolation {
    VerificationViolation {
        local: destination,
        local_name: local_display_name(body, destination),
        message: format!(
            "the call through a function value ending bb{block} passes argument {position} \
             against the value's convention: a call through a value owns exactly the \
             positions its type hands a reference on at"
        ),
    }
}

fn unadapted_violation(
    body: &Body,
    local: Local,
    block: usize,
    position: usize,
) -> VerificationViolation {
    VerificationViolation {
        local,
        local_name: local_display_name(body, local),
        message: format!(
            "handed to the runtime closure slot at argument {position} of the call ending \
             bb{block} unadapted; the runtime lends every argument, so a closure owning a \
             parameter is handed over through an adapter that lends"
        ),
    }
}
