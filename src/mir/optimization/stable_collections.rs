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
//! - no call can write its header: every call into the runtime names an
//!   entry point `runtime_fns::leaves_collection_headers_unchanged` lists,
//!   or copies an array or a list whose elements run no `clone` method of
//!   the program's own — a copy-on-write hands back its argument or a fresh
//!   copy and writes no header of a collection that already exists, so a
//!   loop writing another list stays recorded while the list it writes, which
//!   takes the result, does not — and a call to a function of the program
//!   is allowed only where `analysis::header_reach` finds the local holds a
//!   reference of its own and the call names nothing that may hold the
//!   collection without one; a call through a vtable or a kernel launch is
//!   never allowed, and nothing is stored through a dereference;
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
//! collection need not be unshared: another name for it holds a reference
//! of its own, so a write through it copies the collection first.
//!
//! `analysis::stable_headers` decides those conditions, for this pass and
//! for the loop-versioning pass that runs before it.
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
use std::collections::{BTreeMap, BTreeSet};

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::analysis::loops::{natural_loops, predecessors, NaturalLoop};
use crate::mir::analysis::stable_headers::StableHeaders;
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BasicBlockData, Body, Local, Place, PlaceElem, Rvalue, StableCollection,
    Terminator, TerminatorKind,
};

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
    let Some(mut headers) = StableHeaders::of(body) else {
        return Vec::new();
    };
    let mut loops = natural_loops(body, &DominatorTree::compute(body));
    loops.sort_by_key(|found| Reverse(found.blocks.len()));
    let mut found: Vec<(NaturalLoop, Local)> = Vec::new();
    for candidate in loops {
        if !headers.leaves_every_header(&candidate) {
            continue;
        }
        let stable: Vec<Local> = accessed_collections(body, &candidate)
            .into_iter()
            .filter(|local| !is_recorded_around(&found, &candidate, *local))
            .filter(|local| headers.keeps_header(&candidate, *local))
            .collect();
        found.extend(stable.into_iter().map(|local| (candidate.clone(), local)));
    }
    found
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
