// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks every `out` argument of a call against the rules of taking a place.
//!
//! An `out` argument names a place the caller owns — a whole local, or a
//! field or element reached from one — which the callee writes. The place is
//! taken for the call: the callee is its only reader and writer until the
//! call returns. A backend hands a whole local over as a slot it reads back
//! after the call, and a field or element as the address of the place itself,
//! so the callee's write lands in the caller's value. Three rules keep that
//! sound, each read off the call alone:
//!
//! - the argument is a place spelled as a copy, never a constant, a move or a
//!   step through a dereference; a field or element is taken by its address
//!   only where [`out_handover`] hands it over by address, the place holding
//!   its value at the width the callee reads it as, and never inside a class
//!   instance another binding may share ([`is_shared_on_assignment`]), whose
//!   slot the callee would release while that binding reads it;
//! - no other `out` argument and no destination starts at the same local, so
//!   two writes never land in one value;
//! - no other operand, the callee included, reads a place starting at that
//!   local while it is taken, nor is a temp the call's block copied from a
//!   value on the way to a taken field or element, and no index the place is
//!   reached through is written by the call.
//!
//! Device code hands every `out` argument over copied in ([`out_handover`]):
//! a device pointer parameter reaches only a whole function variable, never a
//! buffer's storage or a sub-object. So in a GPU body an `out` argument is a
//! whole temp of the compiler's, never a declared binding or a parameter,
//! copied from a place in the call's own block, and the block the call
//! continues in, or a block of stores put in front of it, starts by storing
//! the temp back into that place. The places the temps are copied from keep
//! the host's rules: no two of them and no destination start at one local,
//! and no index one is reached through is another of them, the destination,
//! or a place a store after the call writes, which would move the element
//! before it is stored back. Device code also keeps these, read off the
//! body's signature and its calls:
//!
//! - a kernel's storage access names every parameter, and binds read-write
//!   every buffer of atomics, every buffer an `out` argument is copied from,
//!   and every parameter it declares `out`;
//! - a device helper's `out` parameter is never a buffer, which no function
//!   variable can stand for;
//! - no call asks the runtime to copy a value on write or to check that it is
//!   unique ([`crate::runtime_effects::RuntimeEffect`]): device code holds no counts to ask about.

use super::{local_display_name, VerificationViolation};
use crate::ast::type_arguments::TypeArgKind;
use crate::ast::types::{BuiltinCollectionKind, TypeKind, ATOMIC_TYPE_NAME};
use crate::mir::place::PlaceElem;
use crate::mir::place_type::PlaceTypes;
use crate::mir::representation::{out_handover, OutHandover, Representation};
use crate::mir::terminator::{arg_mode, taken_places, ArgMode, TerminatorKind};
use crate::mir::type_facts::unsharing::is_shared_on_assignment;
use crate::mir::{
    BasicBlock, BasicBlockData, Body, ExecutionModel, Local, Operand, Place, Rvalue, Statement,
    StatementKind, StorageClass,
};
use crate::type_checker::utils::ResourceTypes;

/// The operands of one call, as this check reads them, and the block it
/// ends.
struct CallOperands<'b> {
    body: &'b Body,
    block: &'b BasicBlockData,
    /// The callee, when it is an operand of its own.
    callee: Option<&'b Operand>,
    args: &'b [Operand],
    modes: &'b [ArgMode],
    destination: &'b Place,
    /// The block the call continues in, when it returns.
    target: Option<BasicBlock>,
}

impl<'b> CallOperands<'b> {
    /// The operands of the call ending `block` of `body`; `None` for
    /// anything else.
    fn of(body: &'b Body, block: &'b BasicBlockData) -> Option<Self> {
        match &block.terminator.as_ref()?.kind {
            TerminatorKind::Call {
                func,
                args,
                arg_modes,
                destination,
                target,
                ..
            } => Some(Self {
                body,
                block,
                callee: Some(func),
                args,
                modes: arg_modes,
                destination,
                target: *target,
            }),
            TerminatorKind::VirtualCall {
                args,
                arg_modes,
                destination,
                target,
                ..
            } => Some(Self {
                body,
                block,
                callee: None,
                args,
                modes: arg_modes,
                destination,
                target: *target,
            }),
            TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
        }
    }

    /// Whether the argument at `position` is passed `out`.
    fn is_out(&self, position: usize) -> bool {
        arg_mode(self.modes, position) == ArgMode::Out
    }

    /// The local each `out` argument ahead of the one at `position` starts
    /// at, so a local taken twice is reported once, at its second taking.
    fn earlier_out_roots(&self, position: usize) -> impl Iterator<Item = Local> + '_ {
        (self.args.iter().enumerate().take(position))
            .filter(|(other, _)| self.is_out(*other))
            .filter_map(|(_, arg)| arg.place().map(|place| place.local))
    }

    /// Every operand the call reads but does not take: the callee and each
    /// argument not passed `out`.
    fn read_operands(&self) -> impl Iterator<Item = &'b Operand> + '_ {
        let lent = (self.args.iter().enumerate())
            .filter(|(position, _)| !self.is_out(*position))
            .map(|(_, arg)| arg);
        self.callee.into_iter().chain(lent)
    }

    /// Whether `local` is a temp the compiler made, which no binding of the
    /// program names: what an operand of the call is held in.
    fn is_temp(&self, local: Local) -> bool {
        (self.body.local_decls.get(local.0)).is_some_and(|decl| decl.name.is_none())
    }

    /// The place the block copies into `local` last before the call, when it
    /// does: what a temp holding an operand of the call was made from.
    fn copied_into(&self, local: Local) -> Option<&'b Place> {
        (self.block.statements.iter().rev()).find_map(|statement| match &statement.kind {
            StatementKind::Assign(
                place,
                Rvalue::Use(Operand::Copy(from) | Operand::Move(from)),
            ) if place.local == local && place.projection.is_empty() => Some(from),
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(..)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::ReadHeader(_)
            | StatementKind::Nop => None,
        })
    }
}

/// Every `out` argument of a call in `body` that breaks a rule of taking a
/// place; `resources` says which types are resources.
pub fn verify_out_places(body: &Body, resources: &ResourceTypes) -> Vec<VerificationViolation> {
    let types = PlaceTypes::of(body);
    let taker = Taker {
        body,
        types: &types,
        resources,
    };
    let mut found = Vec::new();
    let calls = (body.basic_blocks.iter()).filter_map(|block| CallOperands::of(body, block));
    for call in calls {
        for (position, arg) in call.args.iter().enumerate() {
            if !call.is_out(position) {
                continue;
            }
            let messages = taker.spelling_violation(&call, position, arg);
            let messages = messages
                .into_iter()
                .chain(arg.place().into_iter().flat_map(|place| {
                    let mut messages = taken_twice(&call, position, place);
                    messages.extend(read_while_taken(&call, place));
                    messages
                }));
            found.extend(messages.map(|(local, message)| VerificationViolation {
                local,
                local_name: local_display_name(body, local),
                message,
            }));
        }
        if body.is_gpu() {
            found.extend(
                copied_in_aliasing(&call)
                    .into_iter()
                    .map(|(local, message)| VerificationViolation {
                        local,
                        local_name: local_display_name(body, local),
                        message,
                    }),
            );
        }
        for (position, place) in taken_places(call.args, call.modes) {
            found.extend(taker.shared_holders(place).into_iter().map(|message| {
                VerificationViolation {
                    local: place.local,
                    local_name: local_display_name(body, place.local),
                    message: format!("`out` argument {position}, `{place}`, {message}"),
                }
            }));
        }
    }
    found.extend(device_violations(body));
    found
}

/// What judging how an `out` argument is spelled reads: the body, the type
/// of each of its places, and which types are resources.
struct Taker<'b> {
    body: &'b Body,
    types: &'b PlaceTypes<'b>,
    resources: &'b ResourceTypes,
}

impl Taker<'_> {
    /// The rule the `out` argument `arg` at `position` of `call` breaks in
    /// how it is spelled, with the local it names.
    fn spelling_violation(
        &self,
        call: &CallOperands,
        position: usize,
        arg: &Operand,
    ) -> Option<(Local, String)> {
        let place = match arg {
            Operand::Copy(place) => place,
            Operand::Move(place) => {
                return Some((
                    place.local,
                    format!(
                        "`out` argument {position}, `{place}`, is moved, not a place taken for the call"
                    ),
                ))
            }
            Operand::Constant(_) | Operand::Function(_) => {
                return Some((
                    call.destination.local,
                    format!(
                        "`out` argument {position} of the call writing `{}`, `{arg}`, is not a place",
                        call.destination
                    ),
                ))
            }
        };
        if self.body.is_gpu() {
            return copied_in_violation(call, position, place);
        }
        if place.projection.is_empty() {
            return None;
        }
        let message = self.address_violation(place)?;
        Some((
            place.local,
            format!("`out` argument {position}, `{place}`, {message}"),
        ))
    }

    /// Why the field or element `place` cannot be taken by its address, when
    /// it cannot: it is reached through a dereference, or [`out_handover`]
    /// copies a value of its type in. A place whose type cannot be derived is
    /// reported as that.
    fn address_violation(&self, place: &Place) -> Option<String> {
        if (place.projection.iter()).any(|step| matches!(step, PlaceElem::Deref)) {
            return Some(
                "is reached through a dereference, which no call takes a place through".into(),
            );
        }
        let Some(ty) = self.types.declared(place) else {
            return Some("is taken by its address, but its type cannot be derived".into());
        };
        let representation = Representation::of(&ty.kind, self.body.layout_scope());
        match out_handover(&representation, self.body.is_gpu()) {
            OutHandover::ByAddress => None,
            OutHandover::CopiedIn => Some(
                "is a field or element of a type copied into a slot of its own, taken by its address"
                    .into(),
            ),
        }
    }

    /// Each value above the field or element `place` that is a class
    /// instance another binding may share, or whose type cannot be derived:
    /// a callee writing the slot in place would release a value that binding
    /// still reads.
    fn shared_holders(&self, place: &Place) -> Vec<String> {
        let mut holder = Place::new(place.local);
        let mut found = Vec::new();
        for step in &place.projection {
            match self.types.declared(&holder) {
                None => found.push(format!(
                    "is taken by its address, but the type of `{holder}`, which holds it, cannot be derived"
                )),
                Some(ty) if is_shared_on_assignment(self.body, &ty, self.resources) => found.push(
                    format!(
                        "is taken by its address inside `{holder}`, a class instance another binding may share"
                    ),
                ),
                Some(_) => {}
            }
            holder.projection.push(step.clone());
        }
        found
    }
}

/// The `out` argument `place`, at `position`, starting at a local another
/// `out` argument or the destination also writes.
fn taken_twice(call: &CallOperands, position: usize, place: &Place) -> Vec<(Local, String)> {
    let root = place.local;
    let mut found = Vec::new();
    if call.earlier_out_roots(position).any(|other| other == root) {
        found.push((
            root,
            format!(
                "`out` argument {position}, `{place}`, starts at a local taken twice by the call"
            ),
        ));
    }
    if call.destination.local == root {
        found.push((
            root,
            format!(
                "`out` argument {position}, `{place}`, starts at the local the call's \
                 destination `{}` writes",
                call.destination
            ),
        ));
    }
    found
}

/// Each operand reading the root of the `out` argument `place` while the call
/// takes it, each temp operand the call's block copied from a value holding
/// `place`, and each index `place` is reached through that the call writes.
fn read_while_taken(call: &CallOperands, place: &Place) -> Vec<(Local, String)> {
    let root = place.local;
    let reads = (call.read_operands())
        .filter_map(Operand::place)
        .filter(|read| read.local == root)
        .map(|read| {
            (
                root,
                format!("`{read}` reads the place `{place}` starts at while it is taken"),
            )
        });
    let holders = (call.read_operands())
        .filter_map(Operand::place)
        .filter(|read| read.projection.is_empty() && read.local != root)
        .filter(|read| call.is_temp(read.local))
        .filter_map(|read| {
            let from = call.copied_into(read.local)?;
            holds_place(from, place).then(|| {
                (
                    read.local,
                    format!(
                        "`{read}`, copied from `{from}`, lends a value holding the place `{place}` \
                         the call takes"
                    ),
                )
            })
        });
    let indexes = place.projection.iter().filter_map(|step| match step {
        PlaceElem::Index(index, _) => Some(*index),
        PlaceElem::Deref | PlaceElem::Field(_) => None,
    });
    let written_indexes = indexes
        .filter(|index| {
            call.destination.local == *index
                || (call.args.iter().enumerate()).any(|(other, arg)| {
                    call.is_out(other) && arg.place().is_some_and(|out| out.local == *index)
                })
        })
        .map(|index| {
            (
                index,
                format!("the index `{index}` of `{place}` is written by the call that takes it"),
            )
        });
    reads.chain(holders).chain(written_indexes).collect()
}

/// Whether `holder` is a value on the way to the field or element `place`:
/// it starts at the same local, and its steps lead toward `place`, field for
/// field, any element standing for any other since two indexes may be equal.
fn holds_place(holder: &Place, place: &Place) -> bool {
    holder.local == place.local
        && holder.projection.len() < place.projection.len()
        && (holder.projection.iter().zip(&place.projection)).all(|pair| match pair {
            (PlaceElem::Field(a), PlaceElem::Field(b)) => a == b,
            (PlaceElem::Index(..), PlaceElem::Index(..)) | (PlaceElem::Deref, PlaceElem::Deref) => {
                true
            }
            (PlaceElem::Field(_), PlaceElem::Index(..) | PlaceElem::Deref)
            | (PlaceElem::Index(..), PlaceElem::Field(_) | PlaceElem::Deref)
            | (PlaceElem::Deref, PlaceElem::Field(_) | PlaceElem::Index(..)) => false,
        })
}

/// The rule the `out` argument `place`, at `position` of `call` in device
/// code, breaks in how it is copied in and stored back, with the local it
/// names; `None` when it keeps them.
fn copied_in_violation(
    call: &CallOperands,
    position: usize,
    place: &Place,
) -> Option<(Local, String)> {
    let temp = place.local;
    if !place.projection.is_empty() || !is_compiler_temp(call.body, temp) {
        return Some((
            temp,
            format!(
                "`out` argument {position}, `{place}`, is not a whole temp of its own, which \
                 device code hands every `out` argument over as"
            ),
        ));
    }
    let Some(from) = copied_in_from(call.block, temp) else {
        return Some((
            temp,
            format!(
                "`out` argument {position}, `{place}`, is not copied from a place before the call"
            ),
        ));
    };
    if !is_stored_back(call, temp, from) {
        return Some((
            temp,
            format!(
                "`out` argument {position}, `{place}`, is not stored back into `{from}`, the place \
                 it was copied from, first thing after the call"
            ),
        ));
    }
    read_only_source(call, position, from)
}

/// Whether `local` is a temp of the compiler's in `body`: neither a
/// parameter nor a declared binding.
fn is_compiler_temp(body: &Body, local: Local) -> bool {
    !(1..=body.arg_count).contains(&local.0)
        && (body.local_decls.get(local.0)).is_some_and(|decl| !decl.is_user_variable)
}

/// The place `block` copies the whole of `temp` from, when the last
/// statement of `block` writing `temp` is that copy.
fn copied_in_from(block: &BasicBlockData, temp: Local) -> Option<&Place> {
    let last_write = (block.statements.iter().rev()).find(|statement| match &statement.kind {
        StatementKind::Assign(to, _) | StatementKind::Reassign(to, _) => to.local == temp,
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(..)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::ReadHeader(_)
        | StatementKind::Nop => false,
    })?;
    match &last_write.kind {
        StatementKind::Assign(to, Rvalue::Use(Operand::Copy(from))) if to.projection.is_empty() => {
            Some(from)
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
    }
}

/// Whether the block `call` continues in starts by storing `temp` back
/// into `from`, among the stores of the call's other `out` temps. A call
/// that never returns stores nothing back.
fn is_stored_back(call: &CallOperands, temp: Local, from: &Place) -> bool {
    let Some(target) = call.target else {
        return true;
    };
    let Some(continued) = call.body.basic_blocks.get(target.0) else {
        return false;
    };
    (store_back_run(call, continued).into_iter()).any(|(to, stored)| stored == temp && to == from)
}

/// The stores `continued` starts with, each of a whole `out` temp of
/// `call` into a place: the run device code stores the call's `out`
/// arguments back in, with the place and the temp of each.
fn store_back_run<'b>(
    call: &CallOperands<'b>,
    continued: &'b BasicBlockData,
) -> Vec<(&'b Place, Local)> {
    let out_temps: Vec<Local> = (call.args.iter().enumerate())
        .filter(|(position, _)| call.is_out(*position))
        .filter_map(|(_, arg)| arg.place())
        .filter(|place| place.projection.is_empty())
        .map(|place| place.local)
        .collect();
    (continued.statements.iter())
        .map_while(|statement| stored_back(statement).filter(|(_, t)| out_temps.contains(t)))
        .collect()
}

/// Each place an `out` temp of `call` in device code is copied from, with
/// the position of its argument: the places the call takes, which device
/// code reaches through its temps.
fn copied_in_sources<'b>(call: &CallOperands<'b>) -> Vec<(usize, &'b Place)> {
    (call.args.iter().enumerate())
        .filter(|(position, _)| call.is_out(*position))
        .filter_map(|(position, arg)| {
            let temp = arg.place().filter(|place| place.projection.is_empty())?;
            Some((position, copied_in_from(call.block, temp.local)?))
        })
        .collect()
}

/// The host's rules of taking a place, applied in device code to the places
/// the `out` temps of `call` are copied from: no two of them and no
/// destination start at one local, and no index one is reached through is
/// another of them, the destination, or a place the stores after the call
/// write, which would move the element before it is stored back.
fn copied_in_aliasing(call: &CallOperands) -> Vec<(Local, String)> {
    let sources = copied_in_sources(call);
    let continued = (call.target).and_then(|target| call.body.basic_blocks.get(target.0));
    let stored: Vec<Local> = continued
        .map(|continued| store_back_run(call, continued))
        .unwrap_or_default()
        .into_iter()
        .map(|(to, _)| to.local)
        .collect();
    let is_written = |local: Local| {
        call.destination.local == local
            || stored.contains(&local)
            || sources.iter().any(|(_, source)| source.local == local)
    };
    let mut found = Vec::new();
    for (n, &(position, from)) in sources.iter().enumerate() {
        let root = from.local;
        if sources[..n]
            .iter()
            .any(|(_, earlier)| earlier.local == root)
        {
            found.push((
                root,
                format!(
                    "`out` argument {position}, copied from `{from}`, starts at a local taken \
                     twice by the call"
                ),
            ));
        }
        if call.destination.local == root {
            found.push((
                root,
                format!(
                    "`out` argument {position}, copied from `{from}`, starts at the local the \
                     call's destination `{}` writes",
                    call.destination
                ),
            ));
        }
        let mut indexes: Vec<Local> = (from.projection.iter())
            .filter_map(|step| match step {
                PlaceElem::Index(index, _) => Some(*index),
                PlaceElem::Deref | PlaceElem::Field(_) => None,
            })
            .collect();
        indexes.dedup();
        found.extend(
            indexes
                .into_iter()
                .filter(|index| is_written(*index))
                .map(|index| {
                    (
                        index,
                        format!(
                    "the index `{index}` of `{from}`, which `out` argument {position} is copied \
                     from, is written by the call that takes it or by the stores after it"
                ),
                    )
                }),
        );
    }
    found
}

/// The place `statement` stores a whole local into, and that local, when it
/// is such a copy.
fn stored_back(statement: &Statement) -> Option<(&Place, Local)> {
    match &statement.kind {
        StatementKind::Assign(to, Rvalue::Use(Operand::Copy(from)))
            if from.projection.is_empty() =>
        {
            Some((to, from.local))
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
    }
}

/// The finding for a kernel buffer `from` the `out` argument at `position`
/// of `call` is copied from and stored back into, when the kernel binds it
/// read-only.
fn read_only_source(call: &CallOperands, position: usize, from: &Place) -> Option<(Local, String)> {
    let body = call.body;
    let is_buffer = is_parameter(body, from.local) && is_storage_buffer(body, from.local);
    let binds_read_write = (from.local.0.checked_sub(1))
        .and_then(|index| body.signature.storage_read_write.get(index))
        == Some(&true);
    (is_kernel(body) && is_buffer && !binds_read_write).then(|| {
        (
            from.local,
            format!(
                "`{from}`, copied into `out` argument {position} and stored back, is in a \
                 buffer the kernel binds read-only; a buffer a call writes binds read_write"
            ),
        )
    })
}

/// Whether `body` is a kernel a launch enters.
fn is_kernel(body: &Body) -> bool {
    match body.execution_model {
        ExecutionModel::GpuKernel => true,
        ExecutionModel::Cpu | ExecutionModel::Async | ExecutionModel::GpuDevice => false,
    }
}

/// Whether `local` is a parameter of `body`.
fn is_parameter(body: &Body, local: Local) -> bool {
    (1..=body.arg_count).contains(&local.0)
}

/// Whether `local` of `body` is bound to a storage buffer, as a kernel binds
/// one.
fn is_storage_buffer(body: &Body, local: Local) -> bool {
    (body.local_decls.get(local.0)).is_some_and(|decl| match decl.storage_class {
        StorageClass::GpuGlobal | StorageClass::StorageBuffer => true,
        StorageClass::Stack
        | StorageClass::GpuShared
        | StorageClass::GpuConstant
        | StorageClass::GpuPrivate
        | StorageClass::UniformBuffer => false,
    })
}

/// Whether `local` of `body` is a buffer: bound to one, or of a type a
/// kernel binds as one, an array or a list.
fn is_buffer(body: &Body, local: Local) -> bool {
    let is_buffer_type =
        (body.local_decls.get(local.0)).is_some_and(|decl| is_buffer_type(&decl.ty.kind));
    is_buffer_type || is_storage_buffer(body, local)
}

/// Whether `kind` is a type a kernel binds as a buffer: an array or a list.
fn is_buffer_type(kind: &TypeKind) -> bool {
    kind.as_builtin_collection().is_some_and(|kind| match kind {
        BuiltinCollectionKind::Array | BuiltinCollectionKind::List => true,
        BuiltinCollectionKind::Map | BuiltinCollectionKind::Set => false,
    })
}

/// What device code `body` breaks in its signature and its runtime calls:
/// a kernel's `out` parameter bound read-only, a helper's `out` parameter
/// that is a buffer, and a call asking the runtime to copy on write or to
/// check uniqueness. Nothing for a host body.
fn device_violations(body: &Body) -> Vec<VerificationViolation> {
    if !body.is_gpu() {
        return Vec::new();
    }
    let violation = |(local, message): (Local, String)| VerificationViolation {
        local,
        local_name: local_display_name(body, local),
        message,
    };
    out_parameter_violations(body)
        .chain(storage_access_violations(body))
        .chain(unsharing_calls(body))
        .map(violation)
        .collect()
}

/// What a kernel `body` breaks in how it binds its buffers: a storage access
/// that does not name every parameter, and a buffer of atomics bound
/// read-only, which WGSL keeps only in a read-write binding. Nothing for any
/// other body.
fn storage_access_violations(body: &Body) -> Vec<(Local, String)> {
    if !is_kernel(body) {
        return Vec::new();
    }
    let access = &body.signature.storage_read_write;
    if access.len() != body.arg_count {
        return vec![(
            Local(0),
            format!(
                "the kernel's storage access names {} parameters, not its {}",
                access.len(),
                body.arg_count
            ),
        )];
    }
    (1..=body.arg_count)
        .map(Local)
        .filter(|param| is_storage_buffer(body, *param) && holds_atomics(body, *param))
        .filter(|param| access.get(param.0 - 1) != Some(&true))
        .map(|param| {
            (
                param,
                format!(
                    "`{param}` is a buffer of atomics the kernel binds read-only; atomic storage \
                     binds read_write"
                ),
            )
        })
        .collect()
}

/// Whether `local` of `body` is a collection whose elements are atomics.
fn holds_atomics(body: &Body, local: Local) -> bool {
    let Some(decl) = body.local_decls.get(local.0) else {
        return false;
    };
    let kind = &decl.ty.kind;
    let element = if let TypeKind::List(element) | TypeKind::Array(element, _) = kind {
        Some(&**element)
    } else if let TypeKind::Custom(_, Some(args)) = kind {
        args.first().filter(|_| is_buffer_type(kind))
    } else {
        None
    };
    let Some(TypeArgKind::Type(element, _)) = element.map(|element| &element.node) else {
        return false;
    };
    matches!(&element.kind, TypeKind::Custom(name, _) if name == ATOMIC_TYPE_NAME)
}

/// Each `out` parameter of device code `body` it does not hand over as a
/// function variable: a kernel's bound read-only, a helper's that is a
/// buffer.
fn out_parameter_violations(body: &Body) -> impl Iterator<Item = (Local, String)> + '_ {
    let out_parameters = (body.signature.out_params.iter().enumerate())
        .filter(|(_, is_out)| **is_out)
        .map(|(index, _)| (index, Local(index + 1)));
    out_parameters.filter_map(move |(index, param)| {
        if is_kernel(body) {
            let binds_read_write = body.signature.storage_read_write.get(index) == Some(&true);
            return (!binds_read_write).then(|| {
                (
                    param,
                    format!(
                        "`{param}` is a kernel parameter declared `out` that binds read-only; a \
                         kernel's `out` parameter binds read_write"
                    ),
                )
            });
        }
        is_buffer(body, param).then(|| {
            (
                param,
                format!(
                    "`{param}` is a device helper's `out` parameter of buffer type; device code \
                     hands an `out` argument over as a function variable, never a buffer"
                ),
            )
        })
    })
}

/// Each call of device code `body` asking the runtime to copy a value on
/// write, whether it is shared, or to check it is unique, with the local
/// it asks about.
fn unsharing_calls(body: &Body) -> impl Iterator<Item = (Local, String)> + '_ {
    let calls =
        (body.basic_blocks.iter()).filter_map(|block| match &block.terminator.as_ref()?.kind {
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => Some((func, args, destination)),
            TerminatorKind::VirtualCall { .. }
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => None,
        });
    calls.filter_map(|(func, args, destination)| {
        let effect = func.called_runtime_effect()?;
        let unshares =
            effect.is_copy_on_write() || effect.asks_whether_shared() || effect.insists_on_unique();
        let asked = args.first().and_then(Operand::place).unwrap_or(destination);
        let name = func
            .called_runtime_name()
            .unwrap_or("a runtime entry point");
        unshares.then(|| {
            (
                asked.local,
                format!(
                    "device code calls `{name}` on `{asked}`, a copy-on-write or uniqueness \
                     check; device code holds no count to check"
                ),
            )
        })
    })
}
