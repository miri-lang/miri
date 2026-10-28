// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether a loop leaves the header of an array or a list it indexes — the
//! data pointer, the length and the slot size an element access reads —
//! as it was on entry.
//!
//! A loop does when every entry passes through its header and, over every
//! block of it:
//!
//! - the local holding the collection keeps its value: nothing assigns it
//!   but the identity round trip `t = move l … l = move t` a copy-on-write
//!   found unneeded leaves behind, its storage neither starts nor ends, and
//!   its address is never taken, nor is it a capture or a gpu-resident
//!   binding;
//! - no statement can write a header: nothing is stored through a
//!   dereference, and no release or element store may run a drop hook;
//! - no call can write its header, as `header_reach` decides;
//! - for a parameter the body borrows, nothing that may hold such a
//!   collection is released or overwritten except through the parameter.
//!
//! The stable-collections pass records what this finds, and the
//! loop-versioning pass asks it before copying a loop it means to prove.

use std::collections::{HashMap, HashSet};

use crate::ast::types::{BuiltinCollectionKind, Type};
use crate::mir::analysis::header_reach::{may_hold_type, HeaderReach};
use crate::mir::analysis::loops::{predecessors, NaturalLoop};
use crate::mir::place_type::PlaceTypes;
use crate::mir::type_facts::DropHookReach;
use crate::mir::{
    Body, Local, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};

/// What decides, for one body, whether a loop leaves a collection's header
/// unchanged, with each collection's call reach computed once.
pub struct StableHeaders<'a> {
    body: &'a Body,
    candidates: HashSet<Local>,
    releases: ReleaseFacts<'a>,
    predecessors: Vec<Vec<usize>>,
    reaches: HashMap<Local, HeaderReach<'a>>,
}

impl<'a> StableHeaders<'a> {
    /// The facts for `body`, or `None` when it holds no array or list whose
    /// header could be read once.
    pub fn of(body: &'a Body) -> Option<Self> {
        let candidates = indexed_collections(body);
        (!candidates.is_empty()).then(|| Self {
            body,
            candidates,
            releases: ReleaseFacts::of(body),
            predecessors: predecessors(body),
            reaches: HashMap::new(),
        })
    }

    /// Whether every entry into `found` passes through its header and no
    /// statement in it can change any collection's header.
    pub fn leaves_every_header(&self, found: &NaturalLoop) -> bool {
        is_single_entry(found, &self.predecessors)
            && statements_leave_headers(self.body, found, &self.releases)
    }

    /// Whether `found`, a loop [`Self::leaves_every_header`] accepts, keeps
    /// the header of `local`: the local keeps its value, no release frees
    /// what a borrowed `local` points at, and no call reaches its header.
    pub fn keeps_header(&mut self, found: &NaturalLoop, local: Local) -> bool {
        let body = self.body;
        self.candidates.contains(&local)
            && keeps_its_value(body, found, local, &self.predecessors)
            && (!is_borrowed_param(body, local)
                || releases_nothing_it_borrows(body, found, local, &self.releases))
            && {
                let reach = self
                    .reaches
                    .entry(local)
                    .or_insert_with(|| HeaderReach::of(body, local));
                calls_leave_header(body, found, reach)
            }
    }
}

/// The array and list locals `body` may cache the header of: neither a
/// capture, a gpu-resident binding nor a local whose address is taken.
fn indexed_collections(body: &Body) -> HashSet<Local> {
    let mut foreign: HashSet<Local> = body.env_capture_locals.iter().copied().collect();
    for statement in body.basic_blocks.iter().flat_map(|block| &block.statements) {
        if let StatementKind::Assign(_, Rvalue::Ref(place))
        | StatementKind::Reassign(_, Rvalue::Ref(place)) = &statement.kind
        {
            foreign.insert(place.local);
        }
    }
    (body.local_decls.iter().enumerate())
        .filter(|(_, decl)| decl.device_handle.is_none())
        .filter(|(_, decl)| {
            matches!(
                decl.ty.kind.as_builtin_collection(),
                Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
            )
        })
        .map(|(index, _)| Local(index))
        .filter(|local| !foreign.contains(local))
        .collect()
}

/// Whether every edge into a block of `found` other than its header comes
/// from inside it, so every entry passes through the header.
pub fn is_single_entry(found: &NaturalLoop, predecessors: &[Vec<usize>]) -> bool {
    found.header != 0
        && !found.entries(predecessors).is_empty()
        && (found.blocks.iter())
            .filter(|block| **block != found.header)
            .all(|block| {
                (predecessors.get(*block).into_iter().flatten()).all(|p| found.blocks.contains(p))
            })
}

/// The type facts a release is read against: whether it may run a drop hook.
struct ReleaseFacts<'a> {
    places: PlaceTypes<'a>,
    hooks: DropHookReach<'a>,
}

impl<'a> ReleaseFacts<'a> {
    fn of(body: &'a Body) -> Self {
        Self {
            places: PlaceTypes::of(body),
            hooks: DropHookReach::new(body.rc_classifier.definitions()),
        }
    }

    /// Whether what `place` holds may hold a value of `target`'s type; a
    /// place whose type cannot be read may.
    fn may_hold(&self, place: &Place, target: &Type) -> bool {
        self.places
            .declared(place)
            .is_none_or(|ty| may_hold_type(&ty, target))
    }

    /// Whether releasing what `place` holds may run a drop hook; a place
    /// whose type cannot be read may.
    fn may_run_drop_hook(&self, place: &Place) -> bool {
        self.places
            .declared(place)
            .is_none_or(|ty| self.hooks.may_run_drop_hook(&ty))
    }
}

/// Whether no statement in `found` can change any collection's header.
fn statements_leave_headers(body: &Body, found: &NaturalLoop, facts: &ReleaseFacts) -> bool {
    found.blocks.iter().all(|block| {
        body.basic_blocks.get(*block).is_some_and(|data| {
            (data.statements.iter()).all(|statement| statement_leaves_headers(statement, facts))
        })
    })
}

/// Whether every block of `found` ends in a terminator that reaches no code
/// able to change the header of `reach`'s collection.
fn calls_leave_header(body: &Body, found: &NaturalLoop, reach: &HeaderReach) -> bool {
    found.blocks.iter().all(|block| {
        (body.basic_blocks.get(*block))
            .and_then(|data| data.terminator.as_ref())
            .is_some_and(|terminator| reach.call_leaves_header(terminator))
    })
}

/// Whether `statement` can neither write a header nor run a drop hook.
///
/// A whole local given a new value is not checked here: reference counting
/// releases its old value through an explicit `DecRef` before the
/// assignment, and that release is checked like any other.
fn statement_leaves_headers(statement: &Statement, facts: &ReleaseFacts) -> bool {
    match &statement.kind {
        StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
            !place.projection.contains(&PlaceElem::Deref)
                && (place.projection.is_empty() || !facts.may_run_drop_hook(place))
        }
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
            !facts.may_run_drop_hook(place)
        }
        StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::Nop => true,
    }
}

/// Whether `local` is a parameter the body borrows from its caller rather
/// than takes over, so it holds no reference of its own.
fn is_borrowed_param(body: &Body, local: Local) -> bool {
    (1..=body.arg_count).contains(&local.0) && !body.is_owned_param(local)
}

/// Whether nothing in `found` releases or overwrites, other than through
/// `local` itself, a value that may hold a collection of `local`'s type —
/// the release that could free what a borrowed `local` points at.
fn releases_nothing_it_borrows(
    body: &Body,
    found: &NaturalLoop,
    local: Local,
    facts: &ReleaseFacts,
) -> bool {
    let Some(target) = body.local_decls.get(local.0).map(|decl| &decl.ty) else {
        return false;
    };
    let statements = (found.blocks.iter())
        .filter_map(|block| body.basic_blocks.get(*block))
        .flat_map(|data| &data.statements);
    statements
        .into_iter()
        .all(|statement| match &statement.kind {
            StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
                place.local == local
                    || place.projection.is_empty()
                    || !facts.may_hold(place, target)
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
                place.local == local || !facts.may_hold(place, target)
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::Nop => true,
        })
}

/// Whether `local` holds the value it entered `found` with on every block of
/// it.
fn keeps_its_value(
    body: &Body,
    found: &NaturalLoop,
    local: Local,
    predecessors: &[Vec<usize>],
) -> bool {
    found.blocks.iter().all(|block| {
        let Some(data) = body.basic_blocks.get(*block) else {
            return false;
        };
        let statements_keep = (data.statements.iter().enumerate()).all(|(position, statement)| {
            !rebinds(statement, local)
                || is_identity_write_back(body, *block, position, local, predecessors)
        });
        let terminator_keeps = (data.terminator.iter())
            .flat_map(Terminator::written_places)
            .all(|place| place.local != local || !place.projection.is_empty());
        statements_keep && terminator_keeps
    })
}

/// Whether `statement` gives `local` a value, starts or ends its storage, or
/// releases it.
fn rebinds(statement: &Statement, local: Local) -> bool {
    match &statement.kind {
        StatementKind::Assign(place, _)
        | StatementKind::Reassign(place, _)
        | StatementKind::DecRef(place)
        | StatementKind::Dealloc(place) => place.local == local && place.projection.is_empty(),
        StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
            place.local == local
        }
        StatementKind::IncRef(_) | StatementKind::Nop => false,
    }
}

/// Whether the statement at `position` of `block` is `local = move t` with
/// `t = move local` just before it: earlier in the block, or as the last
/// statement of the one block jumping to it.
fn is_identity_write_back(
    body: &Body,
    block: usize,
    position: usize,
    local: Local,
    predecessors: &[Vec<usize>],
) -> bool {
    let Some(data) = body.basic_blocks.get(block) else {
        return false;
    };
    let Some(temp) = data
        .statements
        .get(position)
        .and_then(|statement| moved_whole(statement, local))
    else {
        return false;
    };
    let before = data.statements.get(..position).unwrap_or_default();
    if let Some(last) = before
        .iter()
        .rposition(|statement| writes_whole(statement, temp) || writes_whole(statement, local))
    {
        return before
            .get(last)
            .and_then(|statement| moved_whole(statement, temp))
            == Some(local);
    }
    let [only] = predecessors
        .get(block)
        .map(Vec::as_slice)
        .unwrap_or_default()
    else {
        return false;
    };
    let Some(pred) = body.basic_blocks.get(*only) else {
        return false;
    };
    let jumps_here = matches!(
        pred.terminator.as_ref().map(|terminator| &terminator.kind),
        Some(TerminatorKind::Goto { target }) if target.0 == block
    );
    jumps_here
        && pred
            .statements
            .last()
            .and_then(|statement| moved_whole(statement, temp))
            == Some(local)
}

/// The whole local `statement` moves into the whole local `dest`, if it is
/// `dest = move source`.
fn moved_whole(statement: &Statement, dest: Local) -> Option<Local> {
    let (StatementKind::Assign(place, Rvalue::Use(Operand::Move(source)))
    | StatementKind::Reassign(place, Rvalue::Use(Operand::Move(source)))) = &statement.kind
    else {
        return None;
    };
    (place.local == dest && place.projection.is_empty() && source.projection.is_empty())
        .then_some(source.local)
}

/// Whether `statement` gives the whole of `local` a value.
fn writes_whole(statement: &Statement, local: Local) -> bool {
    statement
        .written_place()
        .is_some_and(|place| place.local == local && place.projection.is_empty())
}
