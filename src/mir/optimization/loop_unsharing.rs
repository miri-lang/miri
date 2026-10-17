// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Makes a collection a loop writes every iteration its own once, before the
//! loop, instead of asking at the first write of each iteration.
//!
//! A write into a collection a local holds first asks the collection's
//! copy-on-write entry point whether anything else holds it. The
//! uniqueness-check elision answers a repeated question only when every path
//! into it already answered it, and the edge into a loop brings no answer, so
//! the first write of every iteration asks again even when nothing in the
//! loop shares the collection.
//!
//! For each loop with a block that asks about a whole local `x` and runs on
//! every iteration — it dominates every edge back to the header — this pass
//! puts a copy-on-write of `x` on a block of its own that every entry into the
//! loop passes through. The elision then knows `x` unshared on entry, and on
//! the edge back as well whenever nothing in the loop shares it, and drops the
//! questions inside. A loop that shares `x` keeps them, and pays one inline
//! compare more on entry.
//!
//! A uniqueness check on a whole local — the first question of a write
//! through a path, `l[i].x = v` — is not hoisted. It redefines the local at
//! its join every iteration; hoisted, the local is one value live through the
//! whole loop and across its cold calls, and code generation keeps it on the
//! stack and reloads it before every element access, which costs more than
//! the inline check saved.
//!
//! Only a local the loop writes no other way than by these questions' own
//! write-backs takes part, so `x` holds its value before the loop starts; a
//! parameter, a capture, a gpu-resident binding and an address-taken local
//! never do, as no fact is rooted at one. A loop whose question runs only on
//! some iterations is left alone: copying a shared collection before it would
//! copy one the loop may never write. The one copy this adds is a shared
//! collection entering a loop that runs no iteration at all.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::{
    BasicBlock, BasicBlockData, Body, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::{cow_fn, is_copy_on_write, returns_unshared};

/// A natural loop: its header and every block of it, the header included.
struct Loop {
    header: usize,
    blocks: BTreeSet<usize>,
    latches: Vec<usize>,
}

/// Inserts a copy-on-write before each loop that asks about a collection
/// every iteration and writes it no other way. Returns true if any was
/// inserted.
pub fn hoist_loop_unsharing(body: &mut Body) -> bool {
    let asked = whole_locals_asked(body);
    if asked.is_empty() {
        return false;
    }
    let dominators = DominatorTree::compute(body);
    let loops = natural_loops(body, &dominators);
    let mut hoists: Vec<(usize, Local)> = Vec::new();
    for found in &loops {
        for local in locals_asked_every_iteration(found, &asked, &dominators) {
            if is_written_only_by_its_questions(body, found, local) {
                hoists.push((found.header, local));
            }
        }
    }
    let mut changed = false;
    for (header, local) in hoists {
        changed |= make_own_before(body, &loops, header, local);
    }
    changed
}

/// The blocks asking about each whole local the elision may track: by local,
/// the blocks ending in a copy-on-write call on it.
fn whole_locals_asked(body: &Body) -> BTreeMap<usize, Vec<usize>> {
    let foreign = foreign_locals(body);
    let mut asked: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, data) in body.basic_blocks.iter().enumerate() {
        let Some(local) = data.terminator.as_ref().and_then(asked_whole_local) else {
            continue;
        };
        let is_collection = (body.local_decls.get(local.0))
            .and_then(|decl| decl.ty.kind.as_builtin_collection())
            .is_some();
        if is_collection && !foreign.contains(&local) {
            asked.entry(local.0).or_default().push(index);
        }
    }
    asked
}

/// The whole local `terminator` hands to a copy-on-write call.
// TODO: also hoist a uniqueness check on a whole local once code generation
// loads a loop-invariant list's header before the loop; until then the
// hoisted local is spilled and reloaded at every element access.
fn asked_whole_local(terminator: &Terminator) -> Option<Local> {
    let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
        return None;
    };
    if !func.called_runtime_name().is_some_and(is_copy_on_write) {
        return None;
    }
    let [Operand::Move(place)] = args.as_slice() else {
        return None;
    };
    place.projection.is_empty().then_some(place.local)
}

/// Every local whose value code this body does not show may hold or change:
/// parameters, captures, gpu-resident bindings and address-taken locals.
fn foreign_locals(body: &Body) -> HashSet<Local> {
    let mut foreign: HashSet<Local> = (1..=body.arg_count).map(Local).collect();
    foreign.extend(body.env_capture_locals.iter().copied());
    for (index, decl) in body.local_decls.iter().enumerate() {
        if decl.device_handle.is_some() {
            foreign.insert(Local(index));
        }
    }
    for statement in body.basic_blocks.iter().flat_map(|block| &block.statements) {
        if let StatementKind::Assign(_, Rvalue::Ref(place))
        | StatementKind::Reassign(_, Rvalue::Ref(place)) = &statement.kind
        {
            foreign.insert(place.local);
        }
    }
    foreign
}

/// Every natural loop of `body`, one per header, with the blocks of all the
/// loops sharing that header merged.
fn natural_loops(body: &Body, dominators: &DominatorTree) -> Vec<Loop> {
    let predecessors = predecessors(body);
    let reachable: HashSet<usize> = {
        let unreachable: HashSet<usize> = body.find_unreachable_blocks().into_iter().collect();
        (0..body.basic_blocks.len())
            .filter(|index| !unreachable.contains(index))
            .collect()
    };
    let mut latches: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &block in &reachable {
        for successor in successors(body, block) {
            if dominators.dominates(BasicBlock(successor), BasicBlock(block)) {
                latches.entry(successor).or_default().push(block);
            }
        }
    }
    latches
        .into_iter()
        .map(|(header, latches)| {
            let mut blocks = BTreeSet::from([header]);
            let mut pending = latches.clone();
            while let Some(block) = pending.pop() {
                if blocks.insert(block) {
                    pending.extend(predecessors.get(block).into_iter().flatten().copied());
                }
            }
            Loop {
                header,
                blocks,
                latches,
            }
        })
        .collect()
}

fn successors(body: &Body, block: usize) -> Vec<usize> {
    body.basic_blocks
        .get(block)
        .and_then(|data| data.terminator.as_ref())
        .map(|terminator| terminator.successors().iter().map(|b| b.0).collect())
        .unwrap_or_default()
}

fn predecessors(body: &Body) -> Vec<Vec<usize>> {
    let mut predecessors = vec![Vec::new(); body.basic_blocks.len()];
    for block in 0..body.basic_blocks.len() {
        for successor in successors(body, block) {
            if let Some(list) = predecessors.get_mut(successor) {
                list.push(block);
            }
        }
    }
    predecessors
}

/// The locals some block of `found` asks about on every iteration.
fn locals_asked_every_iteration(
    found: &Loop,
    asked: &BTreeMap<usize, Vec<usize>>,
    dominators: &DominatorTree,
) -> Vec<Local> {
    asked
        .iter()
        .filter(|(_, blocks)| {
            blocks.iter().any(|block| {
                found.blocks.contains(block)
                    && found
                        .latches
                        .iter()
                        .all(|latch| dominators.dominates(BasicBlock(*block), BasicBlock(*latch)))
            })
        })
        .map(|(local, _)| Local(*local))
        .collect()
}

/// Whether the blocks of `found` write `local` only as its questions do: a
/// copy-on-write's result moved back into it, or a uniqueness check's fresh
/// copy replacing it, and never start or end its storage.
fn is_written_only_by_its_questions(body: &Body, found: &Loop, local: Local) -> bool {
    let handed_back = handed_back_temps(body, found);
    found.blocks.iter().all(|&block| {
        let Some(data) = body.basic_blocks.get(block) else {
            return true;
        };
        let statements_ok = data
            .statements
            .iter()
            .all(|statement| match &statement.kind {
                StatementKind::Assign(place, Rvalue::Use(Operand::Move(source)))
                | StatementKind::Reassign(place, Rvalue::Use(Operand::Move(source)))
                    if place.local == local && place.projection.is_empty() =>
                {
                    source.projection.is_empty() && handed_back.contains(&source.local)
                }
                StatementKind::Assign(place, _) | StatementKind::Reassign(place, _) => {
                    place.local != local || !place.projection.is_empty()
                }
                StatementKind::StorageLive(place) | StatementKind::StorageDead(place) => {
                    place.local != local
                }
                StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::Nop => true,
            });
        let terminator_ok = (data.terminator.iter())
            .flat_map(Terminator::written_places)
            .all(|place| place.local != local);
        statements_ok && terminator_ok
    })
}

/// The temps a call in `found` writes a value only the caller holds into: a
/// copy-on-write's result, or a fresh copy.
fn handed_back_temps(body: &Body, found: &Loop) -> HashSet<Local> {
    found
        .blocks
        .iter()
        .filter_map(|block| body.basic_blocks.get(*block)?.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func, destination, ..
            } if destination.projection.is_empty()
                && func
                    .called_runtime_name()
                    .is_some_and(|name| is_copy_on_write(name) || returns_unshared(name)) =>
            {
                Some(destination.local)
            }
            TerminatorKind::Call { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::VirtualCall { .. } => None,
        })
        .collect()
}

/// Routes every entry into the loop at `header` through a copy-on-write of
/// `local`. Returns false when the loop has no entry from outside it.
fn make_own_before(body: &mut Body, loops: &[Loop], header: usize, local: Local) -> bool {
    let Some(found) = loops.iter().find(|found| found.header == header) else {
        return false;
    };
    let Some(decl) = body.local_decls.get(local.0) else {
        return false;
    };
    let Some(cow) = decl.ty.kind.as_builtin_collection().and_then(cow_fn) else {
        return false;
    };
    let ty = decl.ty.clone();
    let entries: Vec<usize> = predecessors(body)
        .get(header)
        .map(|list| {
            (list.iter().copied())
                .filter(|pred| !found.blocks.contains(pred))
                .collect()
        })
        .unwrap_or_default();
    if header == 0 || entries.is_empty() {
        return false;
    }
    let span = body.span;
    let handed_back = body.new_local(LocalDecl::new(ty, span));
    let ask = BasicBlock(body.basic_blocks.len());
    let write_back = BasicBlock(ask.0 + 1);
    body.basic_blocks
        .push(BasicBlockData::new(Some(Terminator::new(
            TerminatorKind::Call {
                func: Operand::runtime(cow, span),
                args: vec![Operand::Move(Place::new(local))],
                arg_modes: Vec::new(),
                arg_handles: Vec::new(),
                destination: Place::new(handed_back),
                target: Some(write_back),
            },
            span,
        ))));
    let mut back = BasicBlockData::new(Some(Terminator::new(
        TerminatorKind::Goto {
            target: BasicBlock(header),
        },
        span,
    )));
    back.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::Use(Operand::Move(Place::new(handed_back))),
        ),
        span,
    });
    body.basic_blocks.push(back);
    for entry in entries {
        let terminator =
            (body.basic_blocks.get_mut(entry)).and_then(|data| data.terminator.as_mut());
        if let Some(terminator) = terminator {
            terminator.replace_successor(BasicBlock(header), ask);
        }
    }
    true
}
