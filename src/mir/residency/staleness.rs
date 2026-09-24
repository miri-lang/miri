// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where a host read of a `gpu`-resident binding needs a readback.
//!
//! A forward dataflow over the body tracks, for each device handle, whether its
//! host array may lag its device buffer. It runs on the body as lowering left
//! it and assumes the readbacks it asks for are in place: a host read leaves its
//! handle current, because a readback will precede it.

use super::{
    device_effect, gpu_handle, readback_destinations, statement_host_reads, terminator_host_reads,
    DeviceEffect,
};
use crate::error::syntax::Span;
use crate::mir::body::DeviceHandleId;
use crate::mir::{Body, Local};
use std::collections::{BTreeMap, HashSet, VecDeque};

/// How far a handle's host array may lag its device buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lag {
    /// On every path here the device has run since the last agreement.
    Behind,
    /// On some paths here the device has run since the last agreement, and on
    /// others it has not.
    MaybeBehind,
}

/// The lag of every handle at one program point. A handle absent from the map
/// is current: its host array holds what its device buffer holds, or it has no
/// device buffer at all.
pub(super) type Staleness = BTreeMap<u64, Lag>;

/// One host read that needs a readback before it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Refresh {
    /// The statement the read belongs to; the block's statement count for a
    /// read by its terminator.
    pub index: usize,
    pub local: Local,
    pub handle: DeviceHandleId,
    pub lag: Lag,
    pub span: Span,
}

/// A host read of a gpu binding within a block, before its handle is resolved.
#[derive(Clone, Copy)]
struct BlockRead {
    index: usize,
    local: Local,
    span: Span,
}

/// The readbacks each block needs, keyed by block, in the order its reads run.
pub(super) fn planned_refreshes(body: &Body) -> BTreeMap<usize, Vec<Refresh>> {
    let destinations = readback_destinations(body);
    let reads: Vec<Vec<BlockRead>> = (0..body.basic_blocks.len())
        .map(|bb| block_reads(body, bb, &destinations))
        .collect();
    staleness_on_entry(body, &reads)
        .into_iter()
        .filter_map(|(bb, entry)| {
            let refreshes = refreshes_in(body, &reads[bb], entry);
            (!refreshes.is_empty()).then_some((bb, refreshes))
        })
        .collect()
}

/// Every handle whose host array lags on entry to the body: a parameter's, whose
/// buffer belongs to a caller that may have launched on it. A binding the body
/// declares starts current — until a launch touches it the handle has no device
/// buffer, and its host value is the only copy.
pub(super) fn staleness_at_body_entry(body: &Body) -> Staleness {
    (1..=body.arg_count)
        .filter(|&index| index < body.local_decls.len())
        .filter_map(|index| gpu_handle(body, Local(index)))
        .map(|handle| (handle.0, Lag::Behind))
        .collect()
}

/// The host reads of block `bb`, statement by statement, then its terminator's.
fn block_reads(body: &Body, bb: usize, destinations: &HashSet<Local>) -> Vec<BlockRead> {
    let block = &body.basic_blocks[bb];
    let statement_reads = block.statements.iter().enumerate().flat_map(|(index, s)| {
        statement_host_reads(body, s, destinations)
            .into_iter()
            .map(move |local| BlockRead {
                index,
                local,
                span: s.span,
            })
    });
    let terminator_reads = block.terminator.iter().flat_map(|t| {
        terminator_host_reads(body, &t.kind)
            .into_iter()
            .map(move |local| BlockRead {
                index: block.statements.len(),
                local,
                span: t.span,
            })
    });
    statement_reads.chain(terminator_reads).collect()
}

/// The staleness on entry to every block reachable from the entry block.
///
/// Lags only ever grow at a join — current and behind make maybe-behind — so
/// the worklist reaches a fixpoint.
fn staleness_on_entry(body: &Body, reads: &[Vec<BlockRead>]) -> BTreeMap<usize, Staleness> {
    let mut entries = BTreeMap::new();
    if body.basic_blocks.is_empty() {
        return entries;
    }
    entries.insert(0, staleness_at_body_entry(body));
    let mut worklist = VecDeque::from([0]);
    while let Some(bb) = worklist.pop_front() {
        let exit = staleness_after(body, bb, &reads[bb], entries[&bb].clone());
        let successors = body.basic_blocks[bb]
            .terminator
            .as_ref()
            .map(|t| t.successors())
            .unwrap_or_default();
        for successor in successors {
            let changed = match entries.get_mut(&successor.0) {
                Some(state) => join(state, &exit),
                None => {
                    entries.insert(successor.0, exit.clone());
                    true
                }
            };
            if changed && !worklist.contains(&successor.0) {
                worklist.push_back(successor.0);
            }
        }
    }
    entries
}

/// The staleness after block `bb`, given `staleness` on entry: each read leaves
/// its handle current, then the terminator's effect applies.
fn staleness_after(
    body: &Body,
    bb: usize,
    reads: &[BlockRead],
    mut staleness: Staleness,
) -> Staleness {
    for read in reads {
        if let Some(handle) = gpu_handle(body, read.local) {
            staleness.remove(&handle.0);
        }
    }
    if let Some(terminator) = &body.basic_blocks[bb].terminator {
        apply_effect(&device_effect(&terminator.kind), &mut staleness);
    }
    staleness
}

/// Apply a terminator's effect to the lag of the handles it names.
pub(super) fn apply_effect(effect: &DeviceEffect<'_>, staleness: &mut Staleness) {
    match effect {
        DeviceEffect::Synchronizes(handle) => {
            staleness.remove(handle);
        }
        DeviceEffect::Launches(handles) => {
            for handle in handles.iter().flatten() {
                staleness.insert(handle.0, Lag::Behind);
            }
        }
        DeviceEffect::Nothing => {}
    }
}

/// Merge `from` into `into`, reporting whether `into` changed.
fn join(into: &mut Staleness, from: &Staleness) -> bool {
    let before = into.clone();
    for (handle, lag) in into.iter_mut() {
        if from.get(handle) != Some(lag) {
            *lag = Lag::MaybeBehind;
        }
    }
    for handle in from.keys() {
        into.entry(*handle).or_insert(Lag::MaybeBehind);
    }
    *into != before
}

/// The reads of a block that find their handle lagging, given `staleness` on
/// entry. A read after the first of a handle finds it current.
fn refreshes_in(body: &Body, reads: &[BlockRead], mut staleness: Staleness) -> Vec<Refresh> {
    reads
        .iter()
        .filter_map(|read| {
            let handle = gpu_handle(body, read.local)?;
            let lag = staleness.remove(&handle.0)?;
            Some(Refresh {
                index: read.index,
                local: read.local,
                handle,
                lag,
                span: read.span,
            })
        })
        .collect()
}
