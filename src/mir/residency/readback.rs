// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The pass that inserts the readbacks host reads of `gpu` bindings need.

use super::emit::{append_readback, flag_assignment, new_block};
use super::staleness::{planned_refreshes, staleness_at_body_entry, Lag, Refresh};
use super::{device_effect, DeviceEffect};
use crate::ast::types::{Type, TypeKind};
use crate::mir::body::{BindingResidency, DeviceHandleId};
use crate::mir::{
    BasicBlock, Body, Discriminant, Local, LocalDecl, Operand, Place, Statement, Terminator,
    TerminatorKind,
};
use std::collections::{BTreeMap, HashMap};

/// For each device handle that needs one, the flag that tracks at run time
/// whether its host array lags its device buffer.
type Flags = BTreeMap<u64, Local>;

/// Insert a readback before every host read of a `gpu`-resident binding whose
/// device buffer may hold results its host array lacks.
///
/// A read every path reaches with the device ahead reads back unconditionally.
/// A read some paths reach with the device ahead and others not — the first
/// read in a loop after a launch before it — tests a flag the pass keeps for
/// the handle: set by every launch on it, cleared by every readback, upload and
/// fresh activation. So a buffer the device has not written since the last
/// readback is never read back again, however often it is read. A read no path
/// reaches with the device ahead needs nothing.
///
/// Runs on host bodies after lowering and before reference counting, which then
/// accounts for the host arrays the readbacks detach.
pub fn insert_readbacks(body: &mut Body) {
    if body.is_gpu() || !declares_gpu_bindings(body) {
        return;
    }
    let plan = planned_refreshes(body);
    if plan.is_empty() {
        return;
    }
    let original_blocks = body.basic_blocks.len();
    let flags = allocate_flags(body, &plan);
    let tails: HashMap<usize, BasicBlock> = plan
        .iter()
        .map(|(&bb, refreshes)| (bb, rewrite_block(body, bb, refreshes, &flags)))
        .collect();
    if flags.is_empty() {
        return;
    }
    let block_ends = (0..original_blocks).map(|bb| tails.get(&bb).map_or(bb, |tail| tail.0));
    keep_flags_on_edges(body, block_ends.collect(), &flags);
    initialize_flags(body, &flags);
    body.device_stale_flags.extend(
        flags
            .iter()
            .map(|(&handle, &flag)| (flag, DeviceHandleId(handle))),
    );
}

fn declares_gpu_bindings(body: &Body) -> bool {
    body.local_decls
        .iter()
        .any(|decl| decl.residency == BindingResidency::Gpu && decl.device_handle.is_some())
}

/// A boolean flag for every handle some read reaches only maybe-behind.
fn allocate_flags(body: &mut Body, plan: &BTreeMap<usize, Vec<Refresh>>) -> Flags {
    let mut flags = Flags::new();
    for refresh in plan.values().flatten() {
        if refresh.lag == Lag::MaybeBehind && !flags.contains_key(&refresh.handle.0) {
            let flag = LocalDecl::new(Type::new(TypeKind::Boolean, refresh.span), refresh.span);
            flags.insert(refresh.handle.0, body.new_local(flag));
        }
    }
    flags
}

/// Split block `bb` at each of its `refreshes`, inserting the readback there.
/// Returns the block that ends with `bb`'s original terminator.
fn rewrite_block(body: &mut Body, bb: usize, refreshes: &[Refresh], flags: &Flags) -> BasicBlock {
    let statements = std::mem::take(&mut body.basic_blocks[bb].statements);
    let terminator = body.basic_blocks[bb].terminator.take();
    let mut pending = statements.into_iter();
    let mut emitted = 0;
    let mut current = BasicBlock(bb);
    for refresh in refreshes {
        let before = pending.by_ref().take(refresh.index - emitted);
        body.basic_blocks[current.0].statements.extend(before);
        emitted = refresh.index;
        current = emit_refresh(
            body,
            current,
            refresh,
            flags.get(&refresh.handle.0).copied(),
        );
    }
    body.basic_blocks[current.0].statements.extend(pending);
    body.basic_blocks[current.0].terminator = terminator;
    current
}

/// Append the readback `refresh` needs to block `from` — behind a test of the
/// handle's flag when the read is only maybe behind — and return the block the
/// read continues in.
fn emit_refresh(
    body: &mut Body,
    from: BasicBlock,
    refresh: &Refresh,
    flag: Option<Local>,
) -> BasicBlock {
    let Some(flag) = flag else {
        return append_readback(body, from, refresh.local, refresh.span);
    };
    if refresh.lag == Lag::Behind {
        return clear_after_readback(body, from, refresh, flag);
    }
    let span = refresh.span;
    let continuation = new_block(body);
    let readback = new_block(body);
    body.basic_blocks[from.0].terminator = Some(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(flag)),
            targets: vec![(Discriminant::bool_false(), continuation)],
            otherwise: readback,
        },
        span,
    ));
    let done = clear_after_readback(body, readback, refresh, flag);
    body.basic_blocks[done.0].terminator = Some(Terminator::new(
        TerminatorKind::Goto {
            target: continuation,
        },
        span,
    ));
    continuation
}

/// Append the readback of `refresh` to `from` and clear `flag` after it.
fn clear_after_readback(
    body: &mut Body,
    from: BasicBlock,
    refresh: &Refresh,
    flag: Local,
) -> BasicBlock {
    let next = append_readback(body, from, refresh.local, refresh.span);
    body.basic_blocks[next.0]
        .statements
        .push(flag_assignment(flag, false, refresh.span));
    next
}

/// Set each flag on the edges out of a launch on its handle, and clear it on the
/// edges out of a readback, upload or activation lowering emitted.
///
/// `block_ends` names, for every block lowering produced, the block now holding
/// its terminator; the readbacks this pass inserted clear their flags already.
fn keep_flags_on_edges(body: &mut Body, block_ends: Vec<usize>, flags: &Flags) {
    let mut edges: BTreeMap<(usize, usize), Vec<Statement>> = BTreeMap::new();
    for end in block_ends {
        let Some(terminator) = &body.basic_blocks[end].terminator else {
            continue;
        };
        let updates = flag_updates(&device_effect(&terminator.kind), flags, terminator);
        if updates.is_empty() {
            continue;
        }
        for successor in terminator.successors() {
            edges
                .entry((end, successor.0))
                .or_default()
                .extend(updates.iter().cloned());
        }
    }
    let predecessors = predecessor_counts(body);
    for ((from, to), statements) in edges {
        insert_on_edge(body, from, to, statements, predecessors[to]);
    }
}

/// The flag assignments a terminator with `effect` implies.
fn flag_updates(
    effect: &DeviceEffect<'_>,
    flags: &Flags,
    terminator: &Terminator,
) -> Vec<Statement> {
    let span = terminator.span;
    match effect {
        DeviceEffect::Synchronizes(handle) => flags
            .get(handle)
            .map(|&flag| flag_assignment(flag, false, span))
            .into_iter()
            .collect(),
        DeviceEffect::Launches(handles) => handles
            .iter()
            .flatten()
            .filter_map(|handle| flags.get(&handle.0))
            .map(|&flag| flag_assignment(flag, true, span))
            .collect(),
        DeviceEffect::Nothing => Vec::new(),
    }
}

/// How many edges enter each block. The entry block counts one more, for the
/// edge into the body.
fn predecessor_counts(body: &Body) -> Vec<usize> {
    let mut counts = vec![0; body.basic_blocks.len()];
    if let Some(entry) = counts.first_mut() {
        *entry = 1;
    }
    for terminator in body
        .basic_blocks
        .iter()
        .filter_map(|b| b.terminator.as_ref())
    {
        for successor in terminator.successors() {
            counts[successor.0] += 1;
        }
    }
    counts
}

/// Run `statements` on the edge from block `from` to block `to`: at the start
/// of `to` when it is entered from nowhere else, otherwise in a block of their
/// own spliced into the edge.
fn insert_on_edge(
    body: &mut Body,
    from: usize,
    to: usize,
    statements: Vec<Statement>,
    predecessors_of_to: usize,
) {
    if predecessors_of_to == 1 {
        body.basic_blocks[to].statements.splice(0..0, statements);
        return;
    }
    let span = statements.first().map(|s| s.span).unwrap_or(body.span);
    let edge = new_block(body);
    body.basic_blocks[edge.0].statements = statements;
    body.basic_blocks[edge.0].terminator = Some(Terminator::new(
        TerminatorKind::Goto {
            target: BasicBlock(to),
        },
        span,
    ));
    if let Some(terminator) = body.basic_blocks[from].terminator.as_mut() {
        terminator.replace_successor(BasicBlock(to), edge);
    }
}

/// Give every flag its value on entry to the body: set for a parameter's handle,
/// whose caller may have launched on it, clear for every other.
///
/// The entry block is where the body starts, so the assignments go first in it —
/// unless a loop also branches back to it, in which case its contents move to a
/// block of their own that the loop re-enters instead.
fn initialize_flags(body: &mut Body, flags: &Flags) {
    let lagging = staleness_at_body_entry(body);
    let span = body.span;
    let assignments: Vec<Statement> = flags
        .iter()
        .map(|(handle, &flag)| flag_assignment(flag, lagging.contains_key(handle), span))
        .collect();
    if predecessor_counts(body).first().copied().unwrap_or(0) > 1 {
        move_entry_block_contents(body);
    }
    body.basic_blocks[0].statements.splice(0..0, assignments);
}

/// Move the entry block's contents to a new block that every branch back to the
/// entry now targets, leaving the entry block empty and jumping there.
fn move_entry_block_contents(body: &mut Body) {
    let moved = new_block(body);
    let statements = std::mem::take(&mut body.basic_blocks[0].statements);
    let terminator = body.basic_blocks[0].terminator.take();
    body.basic_blocks[moved.0].statements = statements;
    body.basic_blocks[moved.0].terminator = terminator;
    for terminator in body
        .basic_blocks
        .iter_mut()
        .filter_map(|b| b.terminator.as_mut())
    {
        terminator.replace_successor(BasicBlock(0), moved);
    }
    body.basic_blocks[0].terminator = Some(Terminator::new(
        TerminatorKind::Goto { target: moved },
        body.span,
    ));
}
