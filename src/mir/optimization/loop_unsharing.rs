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
//! through a path, `l[i].x = v` — is hoisted the same way, but only out of a
//! loop that calls nothing but these questions, their copy paths and the
//! runtime entry points that leave collection headers unchanged. Hoisted, the
//! local is one value live through the whole loop; such a loop is one
//! `stable_collections` records once the checks inside are answered, and code
//! generation then reads the collection's header once before the loop. A loop
//! making any other call keeps its check: code generation reloads the header
//! at every element access there, and a local live across the calls costs
//! more than the inline check it saves.
//!
//! Only a local the loop writes no other way than by these questions' own
//! write-backs takes part, so `x` holds its value before the loop starts; a
//! parameter, a capture, a gpu-resident binding and an address-taken local
//! never do, as no fact is rooted at one. A loop whose question runs only on
//! some iterations is left alone: copying a shared collection before it would
//! copy one the loop may never write.
//!
//! The copy-on-write before a loop is itself a question its enclosing loop
//! asks, on every iteration when the inner loop's header dominates every edge
//! back to the enclosing header. So it moves out again, through every
//! enclosing loop that asks it on every iteration, writes `x` no other way
//! than by the questions' write-backs and never hands `x`'s value to another
//! holder — no retain, no copy or move of the whole local, no call it is
//! handed to but a question or a question's copy — and the pass puts one
//! copy-on-write before the outermost loop it reaches and none before the
//! loops inside: the elision then answers every question within, those of
//! later loops the collection reaches unshared included. A loop that shares
//! `x` stops the move, so the inner loop is made its own before each of its
//! runs, after the sharing. The walk happens within the one pass, on the
//! loops as found before it inserts anything: running the pass again would
//! put another copy-on-write before each outer loop every time.
//!
//! The one copy this can add is made by the copy-on-write before the
//! outermost loop reached, which runs whenever that loop is entered: it
//! copies a shared collection that no question of the loop it was made for
//! would have copied, when the outermost loop runs no iteration, or when that
//! inner loop runs none each time it is entered. Every other entry runs one
//! of those questions at least once, and the first would have copied the
//! shared collection there.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::analysis::loops::{natural_loops, predecessors, NaturalLoop};
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, BasicBlockData, Body, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::{
    cow_fn, is_copy_on_write, leaves_collection_headers_unchanged, returns_unshared, rt,
};

/// What a block asks about a whole local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Question {
    /// Hands it to its copy-on-write entry point.
    CopyOnWrite,
    /// Asks whether anything else holds it, before writing through it.
    IsShared,
}

/// Inserts a copy-on-write before each loop that asks about a collection
/// every iteration and writes it no other way, or before the outermost loop
/// enclosing it that asks that copy-on-write on every iteration and neither
/// writes nor shares the collection. Returns true if any was inserted.
pub fn hoist_loop_unsharing(body: &mut Body) -> bool {
    let asked = whole_locals_asked(body);
    if asked.is_empty() {
        return false;
    }
    let dominators = DominatorTree::compute(body);
    let loops = natural_loops(body, &dominators);
    let mut hoists: BTreeSet<(usize, usize)> = BTreeSet::new();
    for found in &loops {
        for (local, question) in locals_asked_every_iteration(found, &asked, &dominators) {
            let is_worth_hoisting =
                question == Question::CopyOnWrite || calls_only_questions(body, found, &asked);
            if is_worth_hoisting && is_written_only_by_its_questions(body, found, local) {
                let outermost =
                    lift_through_enclosing_loops(body, &loops, &dominators, found, local);
                hoists.insert((outermost.header, local.0));
            }
        }
    }
    let mut changed = false;
    for (header, local) in hoists {
        changed |= make_own_before(body, &loops, header, Local(local));
    }
    changed
}

/// The outermost loop a copy-on-write of `local` made before `found` can be
/// made before instead. It moves out of a loop to the one enclosing it when
/// the moved copy-on-write runs on every iteration of the enclosing loop —
/// `found`'s header dominates every edge back to the enclosing header, so
/// each of its iterations enters `found` — and that loop writes `local` no
/// other way than by its questions and never hands its value to another
/// holder. The copy-on-write then answers every question inside, `found`'s
/// own included, and the inner loops get none of their own.
fn lift_through_enclosing_loops<'l>(
    body: &Body,
    loops: &'l [NaturalLoop],
    dominators: &DominatorTree,
    found: &'l NaturalLoop,
    local: Local,
) -> &'l NaturalLoop {
    let mut lifted = found;
    while let Some(enclosing) = enclosing_loop(loops, lifted) {
        let is_entered_every_iteration = (enclosing.latches.iter())
            .all(|latch| dominators.dominates(BasicBlock(lifted.header), BasicBlock(*latch)));
        if !is_entered_every_iteration
            || !is_written_only_by_its_questions(body, enclosing, local)
            || may_share(body, enclosing, local)
        {
            break;
        }
        lifted = enclosing;
    }
    lifted
}

/// The innermost loop of `loops` holding `inner` other than `inner` itself.
fn enclosing_loop<'l>(loops: &'l [NaturalLoop], inner: &NaturalLoop) -> Option<&'l NaturalLoop> {
    loops
        .iter()
        .filter(|outer| outer.header != inner.header && outer.blocks.contains(&inner.header))
        .min_by_key(|outer| outer.blocks.len())
}

/// Whether a block of `found` may hand the value of the whole local `local`
/// to another holder: retain it, or read it whole anywhere but in taking its
/// length, in a release, or in a call to one of its questions or their copy
/// paths.
fn may_share(body: &Body, found: &NaturalLoop, local: Local) -> bool {
    let mut uses = WholeUses {
        local,
        is_used: false,
    };
    for &block in &found.blocks {
        let Some(data) = body.basic_blocks.get(block) else {
            continue;
        };
        let location = BasicBlock(block);
        for statement in &data.statements {
            match &statement.kind {
                StatementKind::Assign(_, Rvalue::Len(_))
                | StatementKind::Reassign(_, Rvalue::Len(_)) => {}
                StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) => {
                    uses.visit_rvalue(rvalue, location);
                }
                StatementKind::IncRef(place) => {
                    uses.visit_place(place, PlaceContext::NonMutatingUse, location);
                }
                StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Nop => {}
            }
        }
        if let Some(terminator) = data.terminator.as_ref().filter(|t| !is_question_call(t)) {
            uses.visit_terminator(location, terminator);
        }
    }
    uses.is_used
}

/// Whether `terminator` calls a copy-on-write entry point, asks whether a
/// value is shared, or makes a fresh copy.
fn is_question_call(terminator: &Terminator) -> bool {
    let TerminatorKind::Call { func, .. } = &terminator.kind else {
        return false;
    };
    func.called_runtime_name().is_some_and(|name| {
        is_copy_on_write(name) || returns_unshared(name) || name == rt::RC_IS_SHARED
    })
}

/// Whether a visited node reads the whole of `local`.
struct WholeUses {
    local: Local,
    is_used: bool,
}

impl Visitor for WholeUses {
    fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
        if place.local == self.local && place.projection.is_empty() {
            self.is_used = true;
        }
    }
}

/// The blocks asking about each whole local the elision may track: by local,
/// the blocks ending in a copy-on-write call on it or a uniqueness check of
/// it, with which of the two each asks.
fn whole_locals_asked(body: &Body) -> BTreeMap<usize, Vec<(usize, Question)>> {
    let foreign = foreign_locals(body);
    let mut asked: BTreeMap<usize, Vec<(usize, Question)>> = BTreeMap::new();
    for (index, data) in body.basic_blocks.iter().enumerate() {
        let Some((local, question)) = data.terminator.as_ref().and_then(asked_whole_local) else {
            continue;
        };
        let is_collection = (body.local_decls.get(local.0))
            .and_then(|decl| decl.ty.kind.as_builtin_collection())
            .is_some();
        if is_collection && !foreign.contains(&local) {
            asked.entry(local.0).or_default().push((index, question));
        }
    }
    asked
}

/// The whole local `terminator` asks about, and how: a copy-on-write call
/// on it, or a uniqueness check of it.
fn asked_whole_local(terminator: &Terminator) -> Option<(Local, Question)> {
    let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
        return None;
    };
    let name = func.called_runtime_name()?;
    let [argument] = args.as_slice() else {
        return None;
    };
    let question = match argument {
        Operand::Move(_) if is_copy_on_write(name) => Question::CopyOnWrite,
        Operand::Copy(_) | Operand::Move(_) if name == rt::RC_IS_SHARED => Question::IsShared,
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => {
            return None;
        }
    };
    let place = argument.place()?;
    place
        .projection
        .is_empty()
        .then_some((place.local, question))
}

/// Whether every call in `found` is a question about a whole local `asked`
/// names, a copy one of those questions makes on its copy path, or a runtime
/// entry point that leaves collection headers unchanged.
fn calls_only_questions(
    body: &Body,
    found: &NaturalLoop,
    asked: &BTreeMap<usize, Vec<(usize, Question)>>,
) -> bool {
    found
        .blocks
        .iter()
        .filter_map(|block| body.basic_blocks.get(*block)?.terminator.as_ref())
        .all(|terminator| match &terminator.kind {
            TerminatorKind::Call { func, args, .. } => {
                let Some(name) = func.called_runtime_name() else {
                    return false;
                };
                let copies_an_asked_local = matches!(
                    args.as_slice(),
                    [argument] if argument.place().is_some_and(|place| {
                        place.projection.is_empty() && asked.contains_key(&place.local.0)
                    })
                );
                leaves_collection_headers_unchanged(name)
                    || ((is_copy_on_write(name) || returns_unshared(name)) && copies_an_asked_local)
            }
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => true,
            TerminatorKind::GpuLaunch { .. } | TerminatorKind::VirtualCall { .. } => false,
        })
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

/// The locals some block of `found` asks about on every iteration, each
/// with the question asked there, a copy-on-write first.
fn locals_asked_every_iteration(
    found: &NaturalLoop,
    asked: &BTreeMap<usize, Vec<(usize, Question)>>,
    dominators: &DominatorTree,
) -> Vec<(Local, Question)> {
    let runs_every_iteration = |block: usize| {
        found.blocks.contains(&block)
            && (found.latches.iter())
                .all(|latch| dominators.dominates(BasicBlock(block), BasicBlock(*latch)))
    };
    asked
        .iter()
        .filter_map(|(local, blocks)| {
            let questions = blocks
                .iter()
                .filter(|(block, _)| runs_every_iteration(*block));
            let question = questions
                .map(|(_, question)| *question)
                .min_by_key(|question| matches!(question, Question::IsShared))?;
            Some((Local(*local), question))
        })
        .collect()
}

/// Whether the blocks of `found` write `local` only as its questions do: a
/// copy-on-write's result moved back into it, or a uniqueness check's fresh
/// copy replacing it, and never start or end its storage.
fn is_written_only_by_its_questions(body: &Body, found: &NaturalLoop, local: Local) -> bool {
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
fn handed_back_temps(body: &Body, found: &NaturalLoop) -> HashSet<Local> {
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
fn make_own_before(body: &mut Body, loops: &[NaturalLoop], header: usize, local: Local) -> bool {
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
    let entries = found.entries(&predecessors(body));
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
