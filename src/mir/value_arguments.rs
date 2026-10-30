// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every value held as its bytes that a call takes lies in bytes the call
//! does not write, and every value parameter a body writes is its own copy.
//!
//! A value argument travels as the address of the caller's own bytes, which
//! the callee only reads. So a non-`out` value argument of a call to a Miri
//! function is copied into a fresh value local of its own just before the
//! call, and the call takes that, whenever the caller's bytes could change
//! while the callee reads them:
//!
//! - its local is the root of the call's destination — `v = f(v)`,
//!   `s.c = f(s.c)` — which the callee writes through its return slot;
//! - its local is the root of an `out` argument of the same call —
//!   `set(v, v)`, `set(s.c, s)` — which the callee writes in place;
//! - it is read through a projection that is not at a fixed offset of its
//!   root value local — `f(c, c.p)` on a counted `c`, `f(t[i])` — and so
//!   would hand over an address inside a holder the callee could write
//!   through another argument, or replace and free, or one no offset names.
//!
//! A field, however deep, of a value local the call writes neither way —
//! `len(s.center)` — sits at a fixed offset of that local's bytes, so it is
//! passed in place. For a local of the frame those bytes are its own stack
//! slot, which nothing else reaches during the call; for a value parameter
//! they are bytes its caller already guarantees no one writes during this
//! call. The one rule is [`exposure_of`], which the codegen backstop asks
//! too.
//!
//! The callee's half is [`written_value_params`]: a body that writes one of
//! its own value parameters copies it on entry, so a value local passed
//! anywhere else needs no copy. What a call writes is one rule,
//! [`call_written_roots`], read by both halves.
//!
//! A runtime entry point is spelled by the element ABI instead, and an `out`
//! argument names the place the callee writes, so neither is copied. The
//! address the element ABI takes of a value local for such a call is read or
//! written only during that call, so it never exposes a later argument. An
//! address it takes to hand over an element the runtime stores or looks up is
//! only read, so taking it does not write the value parameter it reaches.

use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::block::{BasicBlock, BasicBlockData};
use crate::mir::element_abi::reads_element_through;
use crate::mir::place::{PlaceContext, PlaceElem};
use crate::mir::place_type::PlaceTypes;
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
use crate::mir::visitor::Visitor;
use crate::mir::{Body, Local, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind};
use std::collections::{HashMap, HashSet};

/// Copy every value argument of every call in `body` that needs a copy into
/// a whole local of its own ahead of the call.
pub fn copy_value_arguments(body: &mut Body) {
    if body.is_gpu() {
        return;
    }
    for block in 0..body.basic_blocks.len() {
        let copies: Vec<ArgCopy> = value_arguments_needing_copy(body, block)
            .into_iter()
            .map(|argument| ArgCopy::of(block, argument))
            .collect();
        for copy in copies {
            copy_argument(body, copy);
        }
    }
}

/// Why a value argument cannot be passed as the caller's own bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    /// It is read through a projection not at a fixed offset of its root
    /// value local: from inside a counted holder, or picked by a runtime
    /// index.
    Projected,
    /// Its local is the root of the call's destination.
    Destination,
    /// Its local is the root of an `out` argument of the same call.
    Out,
}

/// A value argument of a call that needs a copy of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct ExposedArgument<'a> {
    /// Its position among the call's arguments.
    pub position: usize,
    /// The place it is read from.
    pub place: &'a Place,
    /// Why it needs the copy.
    pub exposure: Exposure,
    /// The type it is declared at, which the copy is declared at.
    pub declared: Type,
}

/// The arguments of the call ending `block` that pass a value held as its
/// bytes, are neither handed to a runtime entry point nor written back as
/// `out`, and are exposed to the callee as [`exposure_of`] decides.
pub fn value_arguments_needing_copy(body: &Body, block: usize) -> Vec<ExposedArgument<'_>> {
    let Some(call) = body.basic_blocks[block]
        .terminator
        .as_ref()
        .and_then(|terminator| MiriCall::of(&terminator.kind))
    else {
        return Vec::new();
    };
    let written: Vec<Local> =
        call_written_roots(call.args, call.arg_modes, call.destination).collect();
    let types = PlaceTypes::of(body);
    call.value_inputs()
        .filter_map(|(position, place)| {
            let exposure = exposure_of(body, place, call.destination, &written)?;
            let declared = types.declared(place)?;
            body.value_layout(&declared.kind)
                .is_some()
                .then_some(ExposedArgument {
                    position,
                    place,
                    exposure,
                    declared,
                })
        })
        .collect()
}

/// Why the bytes `place` reads could change under the callee of a call in
/// `body` into `destination` that writes the root locals `written`, if they
/// could.
///
/// A local the call writes is exposed however the argument reaches into it.
/// Otherwise only a place not at a fixed offset of its root value local is:
/// one inside a counted holder, which the callee could write through another
/// argument or replace and free, or one picked by a runtime index.
pub fn exposure_of(
    body: &Body,
    place: &Place,
    destination: &Place,
    written: &[Local],
) -> Option<Exposure> {
    if place.local == destination.local {
        Some(Exposure::Destination)
    } else if written.contains(&place.local) {
        Some(Exposure::Out)
    } else if !place.projection.is_empty() && !is_at_fixed_offset_of_value_root(body, place) {
        Some(Exposure::Projected)
    } else {
        None
    }
}

/// Whether `place` sits at a fixed offset of its root local's bytes: the
/// root is a value held as its bytes and every step into it names a field.
fn is_at_fixed_offset_of_value_root(body: &Body, place: &Place) -> bool {
    body.local_decls
        .get(place.local.0)
        .is_some_and(|decl| body.value_layout(&decl.ty.kind).is_some())
        && place.projection.iter().all(|elem| match elem {
            PlaceElem::Field(_) => true,
            PlaceElem::Deref | PlaceElem::Index(_) => false,
        })
}

/// The root locals a call taking `args` under `arg_modes` into `destination`
/// writes: its destination's, and that of every argument it passes `out`.
pub fn call_written_roots<'a>(
    args: &'a [Operand],
    arg_modes: &'a [ArgMode],
    destination: &'a Place,
) -> impl Iterator<Item = Local> + 'a {
    let out_roots = args
        .iter()
        .enumerate()
        .filter(move |(position, _)| arg_mode(arg_modes, *position) == ArgMode::Out)
        .filter_map(|(_, arg)| argument_place(arg))
        .map(|place| place.local);
    std::iter::once(destination.local).chain(out_roots)
}

/// The place `operand` reads, if it reads one.
pub fn argument_place(operand: &Operand) -> Option<&Place> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => Some(place),
        Operand::Constant(_) | Operand::Function(_) => None,
    }
}

/// A call to a Miri function: what it takes and where its result goes.
struct MiriCall<'a> {
    args: &'a [Operand],
    arg_modes: &'a [ArgMode],
    destination: &'a Place,
}

impl<'a> MiriCall<'a> {
    /// The call `kind` makes to a Miri function — statically or through a
    /// vtable or a function value — or `None` for a runtime entry point and
    /// every other terminator.
    fn of(kind: &'a TerminatorKind) -> Option<Self> {
        match kind {
            TerminatorKind::Call {
                func,
                args,
                arg_modes,
                destination,
                ..
            } => func.called_runtime_name().is_none().then_some(Self {
                args,
                arg_modes,
                destination,
            }),
            TerminatorKind::VirtualCall {
                args,
                arg_modes,
                destination,
                ..
            } => Some(Self {
                args,
                arg_modes,
                destination,
            }),
            TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
        }
    }

    /// The position and place of every argument read from a place and not
    /// passed `out`.
    fn value_inputs(&self) -> impl Iterator<Item = (usize, &'a Place)> + '_ {
        self.args
            .iter()
            .enumerate()
            .filter(|(position, _)| arg_mode(self.arg_modes, *position) != ArgMode::Out)
            .filter_map(|(position, arg)| Some((position, argument_place(arg)?)))
    }
}

/// One argument to copy: where it is, what it reads and its type.
struct ArgCopy {
    block: usize,
    position: usize,
    place: Place,
    declared: Type,
}

impl ArgCopy {
    fn of(block: usize, argument: ExposedArgument) -> Self {
        Self {
            block,
            position: argument.position,
            place: argument.place.clone(),
            declared: argument.declared,
        }
    }
}

/// Copy the argument `copy` names into a new value local and pass that
/// instead.
fn copy_argument(body: &mut Body, copy: ArgCopy) {
    let ArgCopy {
        block,
        position,
        place,
        declared,
    } = copy;
    let data = &body.basic_blocks[block];
    let span = data
        .terminator
        .as_ref()
        .map_or_else(Span::default, |terminator| terminator.span);
    let local = Place::new(body.new_local(LocalDecl::new(declared, span)));
    let data = &mut body.basic_blocks[block];
    data.statements.push(Statement {
        kind: StatementKind::Assign(local.clone(), Rvalue::Use(Operand::Copy(place))),
        span,
    });
    if let Some(TerminatorKind::Call { args, .. } | TerminatorKind::VirtualCall { args, .. }) = data
        .terminator
        .as_mut()
        .map(|terminator| &mut terminator.kind)
    {
        args[position] = Operand::Copy(local);
    }
}

/// The value parameters of `body` — its parameters held as their bytes, not
/// declared `out` — that it writes: the root of an assignment's destination,
/// of a call's destination or `out` argument, of a launch's destination or
/// buffer, of a release, or of a place whose address is taken for anything
/// but an element the runtime only reads ([`read_element_addresses`]).
///
/// A caller passes a value parameter as its own bytes, which the callee may
/// only read, so a callee copies each parameter named here on entry and
/// writes the copy. A kernel binds no parameter a caller passes, so it has
/// none.
pub fn written_value_params(body: &Body) -> HashSet<Local> {
    if body.is_gpu() {
        return HashSet::new();
    }
    let is_value_param = |local: &Local| {
        (1..=body.arg_count).contains(&local.0)
            && !body.out_params.get(local.0 - 1).copied().unwrap_or(false)
            && body
                .local_decls
                .get(local.0)
                .is_some_and(|decl| body.value_layout(&decl.ty.kind).is_some())
    };
    let read_only = read_element_addresses(body);
    body.basic_blocks
        .iter()
        .flat_map(|block| written_roots(block, &read_only))
        .filter(is_value_param)
        .collect()
}

/// Every local `body` assigns the address of a place to that is mentioned
/// only there and as an element argument of the call ending the same block:
/// the element ABI's spelling of an element a runtime entry point copies out
/// of those bytes during the call and never writes.
fn read_element_addresses(body: &Body) -> HashSet<Local> {
    let candidates: HashSet<Local> = body
        .basic_blocks
        .iter()
        .flat_map(element_addresses_taken)
        .collect();
    if candidates.is_empty() {
        return candidates;
    }
    let mut mentions = Mentions::of(candidates);
    mentions.visit_body(body);
    mentions.mentioned_twice()
}

/// Each whole local `block` assigns an address to that its call takes as an
/// element argument.
fn element_addresses_taken(block: &BasicBlockData) -> Vec<Local> {
    let Some(terminator) = &block.terminator else {
        return Vec::new();
    };
    block
        .statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(address, Rvalue::Ref(_)) => Some(address),
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => None,
        })
        .filter(|address| {
            address.projection.is_empty() && reads_element_through(&terminator.kind, address.local)
        })
        .map(|address| address.local)
        .collect()
}

/// How often a body mentions each of a set of locals.
struct Mentions(HashMap<Local, usize>);

impl Mentions {
    fn of(locals: HashSet<Local>) -> Self {
        Self(locals.into_iter().map(|local| (local, 0)).collect())
    }

    /// The locals mentioned exactly twice: where the address is taken and
    /// where the call reads it.
    fn mentioned_twice(self) -> HashSet<Local> {
        self.0
            .into_iter()
            .filter_map(|(local, count)| (count == 2).then_some(local))
            .collect()
    }
}

impl Visitor for Mentions {
    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        if let Some(count) = self.0.get_mut(&local) {
            *count += 1;
        }
    }
}

/// The root local of every place `block` writes or takes the address of,
/// leaving out the place behind each address in `read_only`.
fn written_roots(block: &BasicBlockData, read_only: &HashSet<Local>) -> Vec<Local> {
    let mut roots: Vec<Local> = block
        .statements
        .iter()
        .flat_map(|statement| statement_written_roots(&statement.kind, read_only))
        .flatten()
        .collect();
    if let Some(terminator) = &block.terminator {
        roots.extend(terminator_written_roots(&terminator.kind));
    }
    roots
}

/// The root locals `statement` writes or takes the address of, unless it
/// takes it into an address in `read_only`. A storage marker or a retain
/// leaves a value's bytes alone.
fn statement_written_roots(
    statement: &StatementKind,
    read_only: &HashSet<Local>,
) -> [Option<Local>; 2] {
    match statement {
        StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
            let referenced = referenced_root(rvalue).filter(|_| !read_only.contains(&place.local));
            [Some(place.local), referenced]
        }
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => [Some(place.local), None],
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::Nop => [None, None],
    }
}

/// The root local whose address `rvalue` takes.
fn referenced_root(rvalue: &Rvalue) -> Option<Local> {
    match rvalue {
        Rvalue::Ref(place) => Some(place.local),
        Rvalue::Use(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Aggregate(..)
        | Rvalue::Phi(_) => None,
    }
}

/// The root locals `terminator` writes: a call's, by [`call_written_roots`],
/// and a launch's destination and every buffer it binds.
fn terminator_written_roots(terminator: &TerminatorKind) -> Vec<Local> {
    match terminator {
        TerminatorKind::Call {
            args,
            arg_modes,
            destination,
            ..
        }
        | TerminatorKind::VirtualCall {
            args,
            arg_modes,
            destination,
            ..
        } => call_written_roots(args, arg_modes, destination).collect(),
        TerminatorKind::GpuLaunch {
            launch_args,
            destination,
            ..
        } => std::iter::once(destination.local)
            .chain(
                launch_args
                    .args()
                    .iter()
                    .filter_map(argument_place)
                    .map(|place| place.local),
            )
            .collect(),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
    }
}
