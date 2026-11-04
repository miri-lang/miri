// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which operations, and which functions of the program, touch nothing that a
//! name outside them holds, worked out for the verifiers on their own.
//!
//! A verifier that credits a call as harmless must not ask the analysis that
//! let the pass make the same credit: a defect the two share would pass both.
//! This rule is deliberately cruder than that analysis and reads only the
//! specification records (what each runtime entry point is stated to do, the
//! drop hooks a type may run) and the bodies themselves.
//!
//! An operation is **harmless** when it can neither release a value nor change
//! a collection's header that some name outside the body holds:
//!
//! - a store lands on a local the body owns — never a parameter or a capture —
//!   reached through value steps only, never through a field of a class
//!   instance or a dereference;
//! - a release runs no drop hook;
//! - a call into the runtime runs no code of the program and writes in place
//!   only whole locals the body owns;
//! - a call to a function of the program is harmless when its callee is: every
//!   operation of the callee's body is. A call through a vtable or a closure
//!   value, a kernel launch, and a call to a symbol no body of the program is
//!   emitted under are not.
//!
//! A call cycle of bodies harmless on their own stays harmless: running them
//! again runs only harmless operations.
//!
//! A runtime search or sort runs the matching rule of the element type it is
//! handed. Over values built only of scalars and strings that rule is the
//! runtime's own; over a type parameter left open it is whatever the caller
//! instantiates. A body whose only other harm is such a search over an open
//! type **searches**: a call to it is harmless when everything it is handed
//! is built only of scalars and strings, makes its caller search when what it
//! is handed is built of those and the caller's own open type parameters, and
//! is harmful otherwise.
//!
//! Apart from that, a method **writes its receiver in place** when it stores
//! through its receiver or hands it to a runtime entry point that writes that
//! argument in place. Its caller makes such a receiver unique before the
//! call when its type is one a write copies.

use crate::ast::type_arguments::TypeArgKind;
use crate::ast::type_arguments::TypeArgument;
use std::collections::{HashMap, HashSet};

use super::borrow_reach::{reach_of, Reach};
use crate::ast::types::TypeKind;
use crate::mir::place_type::PlaceTypes;
use crate::mir::type_facts::DropHookReach;
use crate::mir::{BasicBlock, Body, Local, Operand, Place, PlaceElem, StatementKind};
use crate::mir::{Terminator, TerminatorKind};
use crate::runtime_effects::RuntimeEffect;

/// The functions of a program found harmless, by the link name a call names
/// its callee by.
pub struct HarmlessCallees {
    status: HashMap<String, Status>,
    receiver_writers: HashSet<String>,
}

/// How much harm a body or an operation may do, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Harmless,
    /// Harmless, but for searches over elements of an open type.
    Searches,
    Harmful,
}

/// What a call hands over, by the matching rules its values may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Handed {
    /// Values built only of scalars and strings.
    ByValue,
    /// Values built of those and type parameters the body leaves open.
    Open,
    /// Anything else.
    Other,
}

/// What searching code a call to a body with `callee` status runs, given
/// what it is `handed`.
fn status_of_call(callee: Status, handed: Handed) -> Status {
    match (callee, handed) {
        (Status::Harmless, _) | (Status::Searches, Handed::ByValue) => Status::Harmless,
        (Status::Searches, Handed::Open) => Status::Searches,
        (Status::Searches, Handed::Other) | (Status::Harmful, _) => Status::Harmful,
    }
}

impl HarmlessCallees {
    /// The harmless functions of `program`; `link_name` names a body the way
    /// a call to it names its callee.
    pub fn of<N>(
        program: &[(N, Body)],
        link_name: impl Fn(&N) -> String,
        hooks: &DropHookReach,
    ) -> Self {
        let mut status: HashMap<String, Status> = HashMap::new();
        let mut calls: HashMap<String, Vec<(String, Handed)>> = HashMap::new();
        let mut receiver_writers = HashSet::new();
        for (name, body) in program {
            let name = link_name(name);
            if writes_receiver_in_place(body) {
                receiver_writers.insert(name.clone());
            }
            let (own, callees) = own_status(body, hooks);
            let entry = status.entry(name.clone()).or_insert(own);
            *entry = (*entry).max(own);
            calls.entry(name).or_default().extend(callees);
        }
        loop {
            let mut changed = false;
            for (name, callees) in &calls {
                let reached = callees
                    .iter()
                    .map(|(callee, handed)| {
                        let callee = status.get(callee).copied().unwrap_or(Status::Harmful);
                        status_of_call(callee, *handed)
                    })
                    .max()
                    .unwrap_or(Status::Harmless);
                let current = status.get(name).copied().unwrap_or(Status::Harmful);
                if reached > current {
                    status.insert(name.clone(), reached);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        HarmlessCallees {
            status,
            receiver_writers,
        }
    }

    /// The blocks of `body` ending in a direct call to a harmless function of
    /// the program.
    pub fn calls_in(&self, body: &Body) -> HashSet<BasicBlock> {
        let places = PlaceTypes::of(body);
        (body.basic_blocks.iter().enumerate())
            .filter(|(_, data)| {
                data.terminator.as_ref().is_some_and(|terminator| {
                    match operation_of(body, &places, terminator) {
                        Operation::CallsProgram(callee, handed) => {
                            let callee = self.status.get(&callee).copied();
                            callee.is_some_and(|callee| {
                                status_of_call(callee, handed) == Status::Harmless
                            })
                        }
                        Operation::Done(_) => false,
                    }
                })
            })
            .map(|(index, _)| BasicBlock(index))
            .collect()
    }

    /// The blocks of `body` ending in a direct call to a method of the
    /// program that writes its receiver in place.
    pub(super) fn receiver_writing_calls_in(&self, body: &Body) -> HashSet<BasicBlock> {
        calls_to(body, &self.receiver_writers)
    }
}

/// The blocks of `body` ending in a direct call to one of `callees`.
fn calls_to(body: &Body, callees: &HashSet<String>) -> HashSet<BasicBlock> {
    (body.basic_blocks.iter().enumerate())
        .filter(|(_, data)| {
            (data.terminator.as_ref())
                .and_then(called_program_link_name)
                .is_some_and(|callee| callees.contains(&callee))
        })
        .map(|(index, _)| BasicBlock(index))
        .collect()
}

/// Whether `body` is a method that stores through its receiver or hands it
/// to a runtime entry point writing that argument in place.
fn writes_receiver_in_place(body: &Body) -> bool {
    let Some(receiver) = body.signature.receiver else {
        return false;
    };
    let stores_through =
        (body.basic_blocks.iter().flat_map(|data| &data.statements)).any(|statement| {
            match &statement.kind {
                StatementKind::Assign(dest, _) | StatementKind::Reassign(dest, _) => {
                    dest.local == receiver && !dest.projection.is_empty()
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::Nop
                | StatementKind::ReadHeader(_) => false,
            }
        });
    let hands_to_writer = (body
        .basic_blocks
        .iter()
        .filter_map(|data| data.terminator.as_ref()))
    .any(|terminator| match &terminator.kind {
        TerminatorKind::Call { func, args, .. } => {
            func.called_runtime_effect().is_some_and(|effect| {
                args.iter().enumerate().any(|(position, arg)| {
                    effect.writes_in_place(position)
                        && arg.place().is_some_and(|place| place.local == receiver)
                })
            })
        }
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::VirtualCall { .. }
        | TerminatorKind::GpuLaunch { .. } => false,
    });
    stores_through || hands_to_writer
}

/// The operations of one body judged by the rule, with its calls to the
/// program judged by the harmless calls a caller found.
pub(super) struct BodyHarm<'b> {
    body: &'b Body,
    places: PlaceTypes<'b>,
    hooks: &'b DropHookReach<'b>,
    harmless_calls: Option<&'b HashSet<BasicBlock>>,
}

impl<'b> BodyHarm<'b> {
    /// The rule over `body`; with no `harmless_calls`, every call to a
    /// function of the program is taken as harmful.
    pub(super) fn of(
        body: &'b Body,
        hooks: &'b DropHookReach<'b>,
        harmless_calls: Option<&'b HashSet<BasicBlock>>,
    ) -> Self {
        BodyHarm {
            body,
            places: PlaceTypes::of(body),
            hooks,
            harmless_calls,
        }
    }

    /// Whether the statement `kind` is harmless.
    pub(super) fn is_harmless_statement(&self, kind: &StatementKind) -> bool {
        match kind {
            StatementKind::Assign(dest, _) | StatementKind::Reassign(dest, _) => {
                stores_into_own_value(self.body, dest)
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => (self.places)
                .declared(place)
                .is_some_and(|ty| !self.hooks.may_run_drop_hook(&ty)),
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::Nop
            | StatementKind::ReadHeader(_) => true,
        }
    }

    /// Whether `terminator`, ending `block`, is harmless.
    pub(super) fn is_harmless_terminator(
        &self,
        block: BasicBlock,
        terminator: &Terminator,
    ) -> bool {
        match operation_of(self.body, &self.places, terminator) {
            Operation::Done(status) => status == Status::Harmless,
            Operation::CallsProgram(..) => self
                .harmless_calls
                .is_some_and(|calls| calls.contains(&block)),
        }
    }
}

/// What a terminator does, by the rule.
enum Operation {
    /// An operation whose harm the body alone decides.
    Done(Status),
    /// A direct call to the function of the program with this link name,
    /// handed what the second field says.
    CallsProgram(String, Handed),
}

fn operation_of(body: &Body, places: &PlaceTypes, terminator: &Terminator) -> Operation {
    match &terminator.kind {
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Operation::Done(Status::Harmless),
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            let Some(symbol) = func.called_symbol() else {
                return Operation::Done(Status::Harmful);
            };
            if !stores_into_own_value(body, destination) {
                return Operation::Done(Status::Harmful);
            }
            match symbol.runtime_effect() {
                Some(effect) => Operation::Done(runtime_call_status(body, places, effect, args)),
                None => Operation::CallsProgram(symbol.link_name(), handed(body, places, args)),
            }
        }
        TerminatorKind::VirtualCall { .. } | TerminatorKind::GpuLaunch { .. } => {
            Operation::Done(Status::Harmful)
        }
    }
}

/// The link name of the function of the program `terminator` calls
/// directly, if it calls one.
fn called_program_link_name(terminator: &Terminator) -> Option<String> {
    let TerminatorKind::Call { func, .. } = &terminator.kind else {
        return None;
    };
    func.called_symbol()
        .filter(|symbol| !symbol.is_runtime())
        .map(|symbol| symbol.link_name())
}

/// The harm `body`'s own operations may do, with each function of the
/// program it calls directly and what it hands that call.
fn own_status(body: &Body, hooks: &DropHookReach) -> (Status, Vec<(String, Handed)>) {
    if body.is_gpu() {
        return (Status::Harmful, Vec::new());
    }
    let harm = BodyHarm::of(body, hooks, None);
    let statements = body.basic_blocks.iter().flat_map(|data| &data.statements);
    let mut own = Status::Harmless;
    for statement in statements {
        if !harm.is_harmless_statement(&statement.kind) {
            return (Status::Harmful, Vec::new());
        }
    }
    let mut callees = Vec::new();
    for terminator in body
        .basic_blocks
        .iter()
        .filter_map(|data| data.terminator.as_ref())
    {
        match operation_of(body, &harm.places, terminator) {
            Operation::Done(status) => own = own.max(status),
            Operation::CallsProgram(callee, handed) => callees.push((callee, handed)),
        }
    }
    (own, callees)
}

/// Whether a store to `dest` in `body` lands only on a value the body owns:
/// the return slot or a local other than a parameter or a capture, reached
/// through value steps.
fn stores_into_own_value(body: &Body, dest: &Place) -> bool {
    is_own_local(body, dest.local)
        && (dest.projection.is_empty()
            || (!dest.projection.contains(&PlaceElem::Deref)
                && reach_of(body, dest) == Some(Reach::InValue)))
}

/// Whether `local` is the return slot or a local of `body` other than a
/// parameter or a capture.
fn is_own_local(body: &Body, local: Local) -> bool {
    (local.0 == 0 || local.0 > body.arg_count) && !body.captures.env_capture_locals.contains(&local)
}

/// The harm of a call in `body` to a runtime entry point doing `effect`, handed
/// `args`: harmful when it calls a closure or writes in place anything but a
/// whole local the body owns; otherwise a search or sort is as harmful as
/// the matching rule of what it is handed, and anything else is harmless.
fn runtime_call_status(
    body: &Body,
    places: &PlaceTypes,
    effect: &RuntimeEffect,
    args: &[Operand],
) -> Status {
    let writes_only_own = args.iter().enumerate().all(|(position, arg)| {
        !effect.writes_in_place(position)
            || arg
                .place()
                .is_none_or(|place| place.projection.is_empty() && is_own_local(body, place.local))
    });
    if !effect.closure_positions().is_empty() || !writes_only_own {
        return Status::Harmful;
    }
    if effect.matches.is_none() && effect.orders.is_none() {
        return Status::Harmless;
    }
    match handed(body, places, args) {
        Handed::ByValue => Status::Harmless,
        Handed::Open => Status::Searches,
        Handed::Other => Status::Harmful,
    }
}

/// What `args`, handed by a call in `body`, are built of.
fn handed(body: &Body, places: &PlaceTypes, args: &[Operand]) -> Handed {
    args.iter()
        .map(|arg| match arg.place() {
            None => Handed::ByValue,
            Some(place) => places.declared(place).map_or(Handed::Other, |ty| {
                built_of(&ty.kind, &body.generics.type_params)
            }),
        })
        .max()
        .unwrap_or(Handed::ByValue)
}

/// What a value of `kind` is built of, as far as a search or sort of it can
/// see, with `open` the type parameters its body leaves open. A map is
/// searched by its keys alone, whichever way it is spelled: no entry point that matches runs anything on
/// a value, and one that replaces a value writes the map in place, which
/// `runtime_call_status` refuses before asking.
fn built_of(kind: &TypeKind, open: &HashSet<String>) -> Handed {
    let of_expression = |expression: &TypeArgument| {
        if let TypeArgKind::Type(ty, _) = &expression.node {
            built_of(&ty.kind, open)
        } else {
            Handed::Other
        }
    };
    match kind {
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::String
        | TypeKind::Void => Handed::ByValue,
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Array(element, _) => {
            of_expression(element)
        }
        TypeKind::Map(key, _) => of_expression(key),
        TypeKind::Tuple(elements) => (elements.iter().map(of_expression))
            .max()
            .unwrap_or(Handed::ByValue),
        TypeKind::Option(inner) => built_of(&inner.kind, open),
        TypeKind::Generic(name, _, _) | TypeKind::Custom(name, None) if open.contains(name) => {
            Handed::Open
        }
        // A built-in collection spelled as its class: the first argument is
        // what a search or sort matches, the element or the key.
        TypeKind::Custom(_, Some(arguments)) if kind.as_builtin_collection().is_some() => {
            arguments.first().map_or(Handed::Other, of_expression)
        }
        // A built-in collection spelled as its bare class: the receiver of
        // the collection's own generic method, whose elements are a type
        // the body leaves open. A call to the body is decided by what its
        // caller hands it.
        TypeKind::Custom(_, None) if kind.as_builtin_collection().is_some() && !open.is_empty() => {
            Handed::Open
        }
        TypeKind::Generic(..)
        | TypeKind::Custom(..)
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Result(..)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Meta(_)
        | TypeKind::Error
        | TypeKind::Linear(_)
        | TypeKind::OneOf(_) => Handed::Other,
    }
}
