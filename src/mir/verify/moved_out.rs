// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that no local is read or released after a `move` handed its
//! reference on, until something re-binds it.
//!
//! [`Operand::Move`] means the source's reference transfers to wherever the
//! operand goes, so whatever produced the move has deleted or re-bound the
//! source's own release. Reading the source again reads a value somebody else
//! now owns, and releasing it frees that value a second time.
//!
//! A forward may-dataflow over every host body: a local is moved at a point
//! when some path into the point moves it and no later statement on that
//! path re-binds it.
//!
//! | MIR event | Effect |
//! |---|---|
//! | `Move(x)` of a whole local, in any operand position | `x` moved, after the statement's reads are checked |
//! | `Assign`/`Reassign` of the whole of `x`, a call writing its result into `x`, `StorageLive(x)` | `x` re-bound: no longer moved |
//! | any other mention of `x` — a `Copy` or `Move` of `x` or of a place under it, an index by `x`, a store under `x`, `IncRef`, `ReadHeader` | a read: a finding while moved |
//! | `DecRef(x)`, `Dealloc(x)` | a release: a finding while moved |
//! | `StorageDead(x, _)`, either cause | nothing: the end of storage releases no reference |
//! | `flag = true` / `flag = false` on a drop flag | the paths take the flag's new value |
//! | the edge a switch on a drop flag leaves along | taken only by the paths whose flag holds the value leading there |
//! | `Move` of a place under `x` | a finding: a move hands on a whole local's reference |
//! | `Return` | a read of the return slot |
//!
//! The reads of a statement are checked before its moves take effect, so the
//! move in a copy-on-write sequence, `t = cow(move x); x = move t`, is the one
//! read of `x` it makes; a second read before `x = move t` is the finding. A
//! statement moving one local twice, `f(move x, move x)`, hands one reference
//! to two holders, and is a finding of its own.
//!
//! `StorageDead` is not a release: reference counting places an explicit
//! `DecRef` for every release, and code generation lowers a `StorageDead`
//! only to the release of a `gpu`-resident binding's device buffer, which a
//! `move` of the host local does not hand on.
//!
//! A drop flag is followed, never trusted: the moved locals are kept apart
//! for each combination of values the body's drop flags may hold, followed
//! from their constant writes, and the edge a switch on a flag leaves along
//! carries only the paths whose flag leads there. Drop elaboration sets a
//! binding's flag right after it is bound whole and clears it right after
//! its storage ends, so a path that moved a binding and then ended it reaches
//! the release behind its flag only if the flag was never cleared, and the
//! release is the finding. The reference-count verifier holds each write to
//! a flag to where it may sit.
//!
//! The rule shares nothing with the passes that write a `move`; it reads the
//! body alone. A GPU body is not checked: it holds no counts.

use super::drop_flags::{DropFlags, Partitioned};
use super::{local_display_name, VerificationViolation};
use crate::mir::place::PlaceContext;
use crate::mir::visitor::Visitor;
use crate::mir::{BasicBlock, Body, Local, Operand, Place, StatementKind, TerminatorKind};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::fmt;

/// Where in a body a statement or terminator sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Site {
    block: usize,
    /// The statement's index, or `None` for the block's terminator.
    statement: Option<usize>,
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.statement {
            Some(index) => write!(f, "bb{}[{}]", self.block, index),
            None => write!(f, "bb{} terminator", self.block),
        }
    }
}

/// The locals moved on some path into a point, indexed by local, each with
/// the least site, in block order, that moved it, so a report names the same
/// move every run.
type Moved = Vec<Option<Site>>;

/// How a statement or terminator touches a local the analysis tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Touch {
    Read,
    Release,
    /// A second move of the local in the statement that first moved it.
    Move,
    /// A move of a place under the local, which no pass may write: a move
    /// hands on a whole local's reference.
    ProjectedMove,
}

/// What one statement or terminator does to the locals it mentions, in the
/// order the analysis applies it: uses are checked, then moves take effect,
/// then re-bindings clear them.
#[derive(Debug, Default)]
struct Effect {
    uses: Vec<(Local, Touch)>,
    moves: Vec<Local>,
    rebinds: Vec<Local>,
    projected_moves: Vec<Local>,
}

impl Effect {
    fn read_place(&mut self, place: &Place) {
        self.uses.push((place.local, Touch::Read));
        self.read_index_locals(place);
    }

    fn release_place(&mut self, place: &Place) {
        self.uses.push((place.local, Touch::Release));
        self.read_index_locals(place);
    }

    fn read_index_locals(&mut self, place: &Place) {
        self.uses
            .extend(place.index_locals().map(|local| (local, Touch::Read)));
    }
}

impl Visitor for Effect {
    fn visit_operand(&mut self, operand: &Operand, _location: BasicBlock) {
        match operand {
            Operand::Move(place) => {
                self.read_place(place);
                if place.projection.is_empty() {
                    self.moves.push(place.local);
                } else {
                    self.projected_moves.push(place.local);
                }
            }
            Operand::Copy(place) => self.read_place(place),
            Operand::Constant(_) | Operand::Function(_) => {}
        }
    }

    /// Reached for a place an rvalue or a terminator names outside an
    /// operand, and for a destination: writing a whole local re-binds it,
    /// writing under one reads it.
    fn visit_place(&mut self, place: &Place, context: PlaceContext, _location: BasicBlock) {
        match context {
            PlaceContext::MutatingUse if place.projection.is_empty() => {
                self.rebinds.push(place.local)
            }
            PlaceContext::MutatingUse | PlaceContext::NonMutatingUse => self.read_place(place),
            PlaceContext::StorageLive => self.rebinds.push(place.local),
            PlaceContext::StorageDead => {}
        }
    }
}

/// What `kind`, a statement of block `here`, does to the locals it mentions.
fn statement_effect(kind: &StatementKind, here: BasicBlock) -> Effect {
    let mut effect = Effect::default();
    match kind {
        StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
            effect.visit_rvalue(rvalue, here);
            effect.visit_place(dest, PlaceContext::MutatingUse, here);
        }
        StatementKind::StorageLive(place) => effect.rebinds.push(place.local),
        StatementKind::IncRef(place) => effect.read_place(place),
        StatementKind::ReadHeader(read) => effect.read_place(&read.collection),
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => effect.release_place(place),
        StatementKind::StorageDead(_, _) | StatementKind::Nop => {}
    }
    effect
}

/// What the terminator of `block` does to the locals it mentions; a return
/// reads the return slot.
fn terminator_effect(body: &Body, block: usize) -> Effect {
    let mut effect = Effect::default();
    if let Some(terminator) = &body.basic_blocks[block].terminator {
        effect.visit_terminator(BasicBlock(block), terminator);
        if matches!(terminator.kind, TerminatorKind::Return) {
            effect.uses.push((Local(0), Touch::Read));
        }
    }
    effect
}

/// Every effect of `block`, each at its site, the terminator last.
fn block_effects(body: &Body, block: usize) -> Vec<(Site, Effect)> {
    let statements = body.basic_blocks[block].statements.iter().enumerate();
    statements
        .map(|(index, statement)| {
            let site = Site {
                block,
                statement: Some(index),
            };
            (site, statement_effect(&statement.kind, BasicBlock(block)))
        })
        .chain(std::iter::once((
            Site {
                block,
                statement: None,
            },
            terminator_effect(body, block),
        )))
        .collect()
}

/// A use of a moved local, found while applying an effect.
struct Finding {
    local: Local,
    moved_at: Site,
    used_at: Site,
    touch: Touch,
}

/// Apply `effect` at `site` to `moved`, handing each use of a moved local to
/// `found`.
fn apply(effect: &Effect, site: Site, moved: &mut Moved, found: &mut impl FnMut(Finding)) {
    for &local in &effect.projected_moves {
        found(Finding {
            local,
            moved_at: site,
            used_at: site,
            touch: Touch::ProjectedMove,
        });
    }
    for &(local, touch) in &effect.uses {
        if let Some(moved_at) = moved[local.0] {
            found(Finding {
                local,
                moved_at,
                used_at: site,
                touch,
            });
        }
    }
    for (index, &local) in effect.moves.iter().enumerate() {
        if effect.moves[..index].contains(&local) {
            found(Finding {
                local,
                moved_at: site,
                used_at: site,
                touch: Touch::Move,
            });
        }
        moved[local.0].get_or_insert(site);
    }
    for local in &effect.rebinds {
        moved[local.0] = None;
    }
}

/// Merge `from` into `into`, keeping the least move site of each local;
/// whether `into` changed.
fn join(into: &mut Moved, from: &Moved) -> bool {
    let mut changed = false;
    for (entry, site) in into.iter_mut().zip(from) {
        let Some(site) = site else {
            continue;
        };
        if entry.is_none_or(|current| *site < current) {
            *entry = Some(*site);
            changed = true;
        }
    }
    changed
}

/// The control flow the analysis walks: each block's successors, and the
/// blocks the entry reaches in reverse postorder.
struct Flow {
    successors: Vec<Vec<usize>>,
    /// Each block's position in reverse postorder; `usize::MAX` when the
    /// entry does not reach it.
    order: Vec<usize>,
}

impl Flow {
    fn of(body: &Body) -> Flow {
        let blocks = body.basic_blocks.len();
        let successors: Vec<Vec<usize>> = (0..blocks)
            .map(|block| successors_of(body, block))
            .collect();
        let mut postorder = Vec::with_capacity(blocks);
        let mut visited = vec![false; blocks];
        let mut stack = vec![(0usize, 0usize)];
        visited[0] = true;
        while let Some((block, next)) = stack.pop() {
            match successors[block].get(next) {
                Some(&successor) => {
                    stack.push((block, next + 1));
                    if !visited[successor] {
                        visited[successor] = true;
                        stack.push((successor, 0));
                    }
                }
                None => postorder.push(block),
            }
        }
        let mut order = vec![usize::MAX; blocks];
        for (position, block) in postorder.into_iter().rev().enumerate() {
            order[block] = position;
        }
        Flow { successors, order }
    }
}

/// The moved locals at one point, kept apart by the values the drop flags
/// hold along the paths reaching it.
type FlowState = Partitioned<Moved>;

/// Apply the effects of `block`, each at its site, to `state`, handing each
/// finding to `found`; a write to a drop flag moves each set of paths to the
/// flag's new value.
fn run_block(
    body: &Body,
    flags: &DropFlags,
    effects: &[(Site, Effect)],
    state: &mut FlowState,
    found: &mut impl FnMut(Finding),
) {
    for (site, effect) in effects {
        for moved in state.states_mut() {
            apply(effect, *site, moved, found);
        }
        let statement =
            (site.statement).and_then(|index| body.basic_blocks[site.block].statements.get(index));
        if let Some((flag, value)) = statement.and_then(|s| flags.write_of(&s.kind)) {
            state.write(flag, value, join);
        }
    }
}

/// The moved locals on entry to every block reachable from the entry block.
fn entry_states(
    body: &Body,
    flags: &DropFlags,
    effects: &[Vec<(Site, Effect)>],
    flow: &Flow,
) -> Vec<Option<FlowState>> {
    let blocks = body.basic_blocks.len();
    let mut entries: Vec<Option<FlowState>> = vec![None; blocks];
    entries[0] = Some(Partitioned::single(
        flags.on_entry(),
        vec![None; body.local_decls.len()],
    ));
    let mut queued = vec![false; blocks];
    let mut work = BinaryHeap::from([Reverse((flow.order[0], 0usize))]);
    queued[0] = true;
    while let Some(Reverse((_, block))) = work.pop() {
        queued[block] = false;
        let mut state = entries[block]
            .clone()
            .unwrap_or_else(Partitioned::unreached);
        run_block(body, flags, &effects[block], &mut state, &mut |_| {});
        for &successor in &flow.successors[block] {
            let arriving = match &body.basic_blocks[block].terminator {
                Some(terminator) => state.along_edge(flags, &terminator.kind, successor),
                None => state.clone(),
            };
            let changed = match &mut entries[successor] {
                Some(entry) => entry.join(&arriving, join),
                slot @ None => {
                    *slot = Some(arriving);
                    true
                }
            };
            if changed && !queued[successor] {
                queued[successor] = true;
                work.push(Reverse((flow.order[successor], successor)));
            }
        }
    }
    entries
}

fn successors_of(body: &Body, block: usize) -> Vec<usize> {
    let terminator = body.basic_blocks[block].terminator.as_ref();
    let successors = terminator.map(|terminator| terminator.successors());
    let successors = successors.unwrap_or_default().into_iter();
    successors
        .map(|successor| successor.0)
        .filter(|successor| *successor < body.basic_blocks.len())
        .collect()
}

/// Every read or release of a local on some path after a `move` of it and
/// before a re-binding, one finding for each move and local, and every move
/// of a place under a local.
pub fn verify_moved_out(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() || body.basic_blocks.is_empty() {
        return Vec::new();
    }
    let effects: Vec<_> = (0..body.basic_blocks.len())
        .map(|block| block_effects(body, block))
        .collect();
    let flow = Flow::of(body);
    let flags = DropFlags::of(body);
    let mut reported = HashSet::new();
    let mut violations = Vec::new();
    let entries = entry_states(body, &flags, &effects, &flow);
    for (block, entry) in entries.into_iter().enumerate() {
        let Some(mut state) = entry else {
            continue;
        };
        run_block(body, &flags, &effects[block], &mut state, &mut |finding| {
            if reported.insert((finding.local, finding.moved_at, finding.touch)) {
                violations.push(violation_of(body, &finding));
            }
        });
    }
    violations
}

fn violation_of(body: &Body, finding: &Finding) -> VerificationViolation {
    let message = match finding.touch {
        Touch::ProjectedMove => format!(
            "moved through a projection at {}; a `move` hands on the reference of a whole \
             local, never of a place under one",
            finding.used_at
        ),
        Touch::Read | Touch::Release | Touch::Move => {
            let verb = match finding.touch {
                Touch::Read => "read",
                Touch::Release => "released",
                Touch::Move | Touch::ProjectedMove => "moved again",
            };
            format!(
                "{verb} at {} after the `move` at {} handed its reference on, with no \
                 re-binding between on some path",
                finding.used_at, finding.moved_at
            )
        }
    };
    VerificationViolation {
        local: finding.local,
        local_name: local_display_name(body, finding.local),
        message,
    }
}
