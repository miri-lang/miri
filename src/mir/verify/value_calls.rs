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
//! is an adapter built where the call is made: a closure value created on
//! every path to the call, every write to its local creating that one
//! closure, whose own body lends every parameter. Every other
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
use crate::mir::analysis::traversal::successors_of;
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::terminator::arg_mode;
use crate::mir::{
    AggregateKind, ArgMode, BasicBlock, Body, Local, Operand, Place, Rvalue, StatementKind,
    TerminatorKind,
};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::utils::{hands_on_a_reference, Instantiations, ResourceTypes};

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
/// `instantiations` are the program's generic class instantiations, which
/// decide whether a trait or an extended class is a resource.
pub fn verify_value_calls(
    bodies: &[(String, Body)],
    definitions: &HashMap<String, TypeDefinition>,
    instantiations: &Instantiations,
) -> Vec<(String, VerificationViolation)> {
    let resources = ResourceTypes::of(definitions, instantiations);
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
            match created_closure(body, BasicBlock(block), place) {
                Some(symbol) => {
                    adapters.insert(symbol.clone());
                }
                None => violations.push(unadapted_violation(body, place.local, block, position)),
            }
        }
    }
    violations
}

/// The closure the whole local `place` names holds when the call ending
/// `call_block` runs: every write to that local in `body` creates a closure
/// of one symbol, and every path from the entry to the call passes one, so
/// whichever write reaches the call is that creation. Lowering builds an
/// adapter once, in the block its call ends; a pass may split that block
/// (`drop_elaboration` splits at a guarded storage end) or copy the
/// creation onto several paths, and the creation still reaches the call.
fn created_closure<'b>(
    body: &'b Body,
    call_block: BasicBlock,
    place: &Place,
) -> Option<&'b Symbol> {
    if !place.projection.is_empty() {
        return None;
    }
    let mut created: Option<&Symbol> = None;
    let mut writing_blocks = HashSet::new();
    for (block, rvalue) in local_writes(body, place.local) {
        let symbol = rvalue.and_then(closure_created)?;
        if created.is_some_and(|known| known != symbol) {
            return None;
        }
        created = Some(symbol);
        writing_blocks.insert(block);
    }
    let reached_unwritten = reaches_avoiding(body, call_block, &writing_blocks);
    created.filter(|_| !reached_unwritten)
}

/// The closure `rvalue` creates, if it creates one.
fn closure_created(rvalue: &Rvalue) -> Option<&Symbol> {
    match rvalue {
        Rvalue::Aggregate(AggregateKind::Closure(symbol, _), _) => Some(symbol),
        Rvalue::Use(_)
        | Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::Aggregate(..)
        | Rvalue::AtomicOp { .. } => None,
    }
}

/// Whether some path from the entry of `body` reaches `target` through no
/// block of `avoided`, `target` included: a path passing one of them
/// leaves it written.
fn reaches_avoiding(body: &Body, target: BasicBlock, avoided: &HashSet<BasicBlock>) -> bool {
    let mut seen = HashSet::new();
    let mut pending = vec![0];
    while let Some(block) = pending.pop() {
        if avoided.contains(&BasicBlock(block)) || !seen.insert(block) {
            continue;
        }
        if block == target.0 {
            return true;
        }
        pending.extend(successors_of(body, block));
    }
    false
}

/// Every write to `local` in `body`, in block order, with the block it is
/// made in and the value an assignment stores, `None` for what a
/// terminator stores: each assignment to a place rooted at it, and each
/// place a terminator writes rooted at it.
fn local_writes(body: &Body, local: Local) -> impl Iterator<Item = (BasicBlock, Option<&Rvalue>)> {
    (body.basic_blocks.iter().enumerate()).flat_map(move |(block, data)| {
        let assigned =
            (data.statements.iter()).filter_map(move |statement| match &statement.kind {
                StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)
                    if dest.local == local =>
                {
                    Some((BasicBlock(block), Some(rvalue)))
                }
                StatementKind::Assign(..)
                | StatementKind::Reassign(..)
                | StatementKind::StorageLive(_)
                | StatementKind::StorageDead(..)
                | StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::ReadHeader(_)
                | StatementKind::Nop => None,
            });
        let called = (data.terminator.iter())
            .flat_map(|terminator| terminator.written_places())
            .filter(move |written| written.local == local)
            .map(move |_| (BasicBlock(block), None));
        assigned.chain(called)
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
            let ExpressionKind::Type(declared, _) = &param.typ.node else {
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
