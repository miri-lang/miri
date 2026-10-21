// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Finds the arrays and lists a loop indexes without changing their header,
//! so code generation reads each header once before the loop rather than at
//! every element access.
//!
//! An element access reads three words of the collection's header: the data
//! pointer, the length it is checked against and, for elements held at a
//! stride of their own, the slot size. A loop leaves all three as they were
//! on entry when, over every block of it:
//!
//! - the local holding the collection keeps its value: nothing assigns it
//!   but the identity round trip `t = move l … l = move t` a copy-on-write
//!   found unneeded leaves behind, its storage neither starts nor ends, and
//!   its address is never taken, nor is it a capture or a gpu-resident
//!   binding, any of which lets code this body does not show change it;
//! - no call can write a header: every call names a runtime entry point
//!   `runtime_fns::leaves_collection_headers_unchanged` lists, and nothing is
//!   stored through a dereference;
//! - no release can run code of the program's own: no release of a value
//!   whose drop may run a `fn drop(self)` hook anywhere inside it, and no
//!   store into a slot holding one, which releases what it overwrites;
//! - for a parameter the body borrows, which holds no reference of its own,
//!   no release may free the collection out from under it: nothing that may
//!   hold such a collection is released or overwritten except through the
//!   parameter itself. A local the body binds holds its own reference, which
//!   no other release can take.
//!
//! Every header a write could change is changed by one of those, so the
//! collection need not be unshared: another name for it cannot change it
//! while the loop runs either.
//!
//! For each such loop the pass gives the loop a preheader of its own — a block
//! every entry into the header passes through, placed after any copy-on-write
//! hoisted in front of the loop — and records the collection, the preheader
//! and the loop's blocks on [`Body::stable_collections`]. Where loops nest,
//! only the outermost loop a collection is stable in is recorded for it. A
//! body that runs on a GPU keeps no header and is left alone.
//!
//! A write through a path in a loop, `l[i].x = v`, first asks whether the
//! whole list is shared, and copies it when it is: a rebinding that keeps
//! the loop from being recorded. `loop_unsharing` makes such a list its own
//! before a loop that calls nothing else, which lets the uniqueness-check
//! elision answer the question inside; this pass then finds the loop with no
//! rebinding and no call, and records it.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::analysis::loops::{natural_loops, predecessors, NaturalLoop};
use crate::mir::instantiation::shape::constructor_parts;
use crate::mir::place::PlaceContext;
use crate::mir::place_type::PlaceTypes;
use crate::mir::type_facts::DropHookReach;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BasicBlockData, Body, Local, Operand, Place, PlaceElem, Rvalue, StableCollection,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::leaves_collection_headers_unchanged;

/// Records every collection a loop of `body` leaves the header of unchanged,
/// inserting a preheader before each such loop. Returns true if any was
/// recorded.
pub fn record_stable_collections(body: &mut Body) -> bool {
    if body.is_gpu() {
        return false;
    }
    let found = stable_loops(body);
    if found.is_empty() {
        return false;
    }
    let preheaders = insert_preheaders(body, &found);
    body.stable_collections = found
        .iter()
        .filter_map(|(found_loop, local)| record(found_loop, *local, &preheaders))
        .collect();
    !body.stable_collections.is_empty()
}

/// Every loop of `body` paired with a collection it leaves unchanged, the
/// outermost loop first.
fn stable_loops(body: &Body) -> Vec<(NaturalLoop, Local)> {
    let candidates = indexed_collections(body);
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut loops = natural_loops(body, &DominatorTree::compute(body));
    loops.sort_by_key(|found| Reverse(found.blocks.len()));
    let releases = ReleaseFacts::of(body);
    let predecessors = predecessors(body);
    let mut found: Vec<(NaturalLoop, Local)> = Vec::new();
    for candidate in loops {
        if !is_single_entry(&candidate, &predecessors)
            || !leaves_every_header_unchanged(body, &candidate, &releases)
        {
            continue;
        }
        let stable: Vec<Local> = accessed_collections(body, &candidate)
            .into_iter()
            .filter(|local| candidates.contains(local))
            .filter(|local| !is_recorded_around(&found, &candidate, *local))
            .filter(|local| keeps_its_value(body, &candidate, *local, &predecessors))
            .filter(|local| {
                !is_borrowed_param(body, *local)
                    || releases_nothing_it_borrows(body, &candidate, *local, &releases)
            })
            .collect();
        found.extend(stable.into_iter().map(|local| (candidate.clone(), local)));
    }
    found
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
fn is_single_entry(found: &NaturalLoop, predecessors: &[Vec<usize>]) -> bool {
    found.header != 0
        && !found.entries(predecessors).is_empty()
        && (found.blocks.iter())
            .filter(|block| **block != found.header)
            .all(|block| {
                (predecessors.get(*block).into_iter().flatten()).all(|p| found.blocks.contains(p))
            })
}

/// Whether a collection the loop `found` is recorded for already covers
/// `local` in a loop around `candidate`.
fn is_recorded_around(
    found: &[(NaturalLoop, Local)],
    candidate: &NaturalLoop,
    local: Local,
) -> bool {
    found
        .iter()
        .any(|(outer, recorded)| *recorded == local && outer.blocks.is_superset(&candidate.blocks))
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
            .is_none_or(|ty| may_hold_type(&ty, target, 0))
    }

    /// Whether releasing what `place` holds may run a drop hook; a place
    /// whose type cannot be read may.
    fn may_run_drop_hook(&self, place: &Place) -> bool {
        self.places
            .declared(place)
            .is_none_or(|ty| self.hooks.may_run_drop_hook(&ty))
    }
}

/// Whether nothing in `found` can change any collection's header.
fn leaves_every_header_unchanged(body: &Body, found: &NaturalLoop, facts: &ReleaseFacts) -> bool {
    found.blocks.iter().all(|block| {
        body.basic_blocks.get(*block).is_some_and(|data| {
            (data.statements.iter()).all(|statement| statement_leaves_headers(statement, facts))
                && data
                    .terminator
                    .as_ref()
                    .is_some_and(terminator_leaves_headers)
        })
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

/// Whether a value of `ty` may be, or hold, a value of `target`'s type: a
/// type whose contents its spelling does not show — a declared type, a
/// type parameter, a function — may hold anything.
fn may_hold_type(ty: &Type, target: &Type, depth: usize) -> bool {
    const MAX_DEPTH: usize = 16;
    let is_opaque = matches!(ty.kind, TypeKind::Generic(..) | TypeKind::Function(_))
        || (matches!(ty.kind, TypeKind::Custom(..)) && ty.kind.as_builtin_collection().is_none());
    if depth > MAX_DEPTH || is_opaque || is_same_type(ty, target) {
        return true;
    }
    (constructor_parts(ty).1.iter()).any(|part| may_hold_type(part, target, depth + 1))
}

/// Whether `a` and `b` spell one type, constructor by constructor.
fn is_same_type(a: &Type, b: &Type) -> bool {
    let (a_name, a_parts) = constructor_parts(a);
    let (b_name, b_parts) = constructor_parts(b);
    a_name == b_name
        && a_parts.len() == b_parts.len()
        && a_parts
            .iter()
            .zip(&b_parts)
            .all(|(a, b)| is_same_type(a, b))
}

/// Whether `terminator` reaches no code that could write a header.
fn terminator_leaves_headers(terminator: &Terminator) -> bool {
    match &terminator.kind {
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => true,
        TerminatorKind::Call { func, .. } => func
            .called_runtime_name()
            .is_some_and(leaves_collection_headers_unchanged),
        TerminatorKind::VirtualCall { .. } | TerminatorKind::GpuLaunch { .. } => false,
    }
}

/// The collections a block of `found` indexes or takes the length of.
fn accessed_collections(body: &Body, found: &NaturalLoop) -> Vec<Local> {
    let mut accessed = AccessedCollections::default();
    for block in &found.blocks {
        if let Some(data) = body.basic_blocks.get(*block) {
            accessed.visit_basic_block(BasicBlock(*block), data);
        }
    }
    accessed.locals.into_iter().map(Local).collect()
}

/// Collects the root of every place indexed at its first projection, and
/// every whole local whose length is taken.
#[derive(Default)]
struct AccessedCollections {
    locals: BTreeSet<usize>,
}

impl Visitor for AccessedCollections {
    fn visit_assign(&mut self, block: BasicBlock, place: &Place, rvalue: &Rvalue) {
        if let Rvalue::Len(measured) = rvalue {
            if measured.projection.is_empty() {
                self.locals.insert(measured.local.0);
            }
        }
        self.visit_place(place, PlaceContext::MutatingUse, block);
        self.visit_rvalue(rvalue, block);
    }

    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        if matches!(place.projection.first(), Some(PlaceElem::Index(_))) {
            self.locals.insert(place.local.0);
        }
    }
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

/// Gives each loop `found` names a preheader of its own: a new block ending
/// in a jump to the header, which every entry into the header now reaches
/// first. Returns the preheader of each header.
fn insert_preheaders(body: &mut Body, found: &[(NaturalLoop, Local)]) -> BTreeMap<usize, usize> {
    let headers: BTreeMap<usize, &NaturalLoop> = (found.iter())
        .map(|(found_loop, _)| (found_loop.header, found_loop))
        .collect();
    let mut preheaders = BTreeMap::new();
    for (header, found_loop) in headers {
        let entries = found_loop.entries(&predecessors(body));
        let preheader = body.basic_blocks.len();
        let jump = TerminatorKind::Goto {
            target: BasicBlock(header),
        };
        let span = body.span;
        body.basic_blocks
            .push(BasicBlockData::new(Some(Terminator::new(jump, span))));
        for entry in entries {
            let terminator =
                (body.basic_blocks.get_mut(entry)).and_then(|data| data.terminator.as_mut());
            if let Some(terminator) = terminator {
                terminator.replace_successor(BasicBlock(header), BasicBlock(preheader));
            }
        }
        preheaders.insert(header, preheader);
    }
    preheaders
}

/// The record of `local` over `found`, its blocks joined by the preheader of
/// every loop nested in it.
fn record(
    found: &NaturalLoop,
    local: Local,
    preheaders: &BTreeMap<usize, usize>,
) -> Option<StableCollection> {
    let preheader = *preheaders.get(&found.header)?;
    let nested = (preheaders.iter())
        .filter(|(header, _)| **header != found.header && found.blocks.contains(header))
        .map(|(_, nested)| *nested);
    let blocks: BTreeSet<usize> = found.blocks.iter().copied().chain(nested).collect();
    Some(StableCollection {
        local,
        preheader: BasicBlock(preheader),
        header: BasicBlock(found.header),
        blocks: blocks.into_iter().map(BasicBlock).collect(),
    })
}
