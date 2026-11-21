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
//!   only where the place holds its value at the width the callee reads it as
//!   ([`Representation::is_passed_out_by_address`]), never inside a class
//!   instance another binding may share ([`is_shared_on_assignment`]), whose
//!   slot the callee would release while that binding reads it, and never in
//!   device code, whose places have no address a call can be handed;
//! - no other `out` argument and no destination starts at the same local, so
//!   two writes never land in one value;
//! - no other operand, the callee included, reads a place starting at that
//!   local while it is taken, nor is a temp the call's block copied from a
//!   value on the way to a taken field or element, and no index the place is
//!   reached through is written by the call.

use super::{local_display_name, VerificationViolation};
use crate::mir::place::PlaceElem;
use crate::mir::place_type::PlaceTypes;
use crate::mir::representation::Representation;
use crate::mir::terminator::{arg_mode, taken_places, ArgMode, TerminatorKind};
use crate::mir::type_facts::unsharing::is_shared_on_assignment;
use crate::mir::{BasicBlockData, Body, Local, Operand, Place, Rvalue, StatementKind};
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
                ..
            } => Some(Self {
                body,
                block,
                callee: Some(func),
                args,
                modes: arg_modes,
                destination,
            }),
            TerminatorKind::VirtualCall {
                args,
                arg_modes,
                destination,
                ..
            } => Some(Self {
                body,
                block,
                callee: None,
                args,
                modes: arg_modes,
                destination,
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
    /// it cannot: it is reached through a dereference, lies in device code,
    /// or holds a value the callee reads at another width. A place whose type
    /// cannot be derived is reported as that.
    fn address_violation(&self, place: &Place) -> Option<String> {
        if (place.projection.iter()).any(|step| matches!(step, PlaceElem::Deref)) {
            return Some(
                "is reached through a dereference, which no call takes a place through".into(),
            );
        }
        if self.body.is_gpu() {
            return Some(
                "is a field or element taken in device code, where a place has no address to hand over"
                    .into(),
            );
        }
        let Some(ty) = self.types.declared(place) else {
            return Some("is taken by its address, but its type cannot be derived".into());
        };
        let representation = Representation::of(&ty.kind, self.body.layout_scope());
        (!representation.is_passed_out_by_address()).then(|| {
            "is a field or element of a type copied into a slot of its own, taken by its address"
                .into()
        })
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
