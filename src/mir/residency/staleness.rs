// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where a host read of a `gpu`-resident binding needs a readback.
//!
//! A forward dataflow over the body tracks, for each mirror — a binding's host
//! array and the device buffer of the handle it carries — whether the host
//! array may lag the device buffer. It runs on the body as lowering left it and
//! assumes the readbacks it asks for are in place: a host read leaves its
//! binding's mirror current, because a readback will precede it.

use super::mirrors::{HandleCarriers, Mirror};
use super::{
    device_effect, gpu_handle, readback_destinations, statement_adoption, statement_host_reads,
    terminator_host_reads,
};
use crate::error::syntax::Span;
use crate::mir::body::DeviceHandleId;
use crate::mir::{Body, Local};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// How far a host array may lag the device buffer it mirrors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lag {
    /// On every path here the device has run since the last agreement.
    Behind,
    /// On some paths here the device has run since the last agreement, and on
    /// others it has not.
    MaybeBehind,
}

/// The lag of every mirror at one program point. A mirror absent from the map
/// is current: its host array holds what its device buffer holds, or the handle
/// has no device buffer at all.
pub(super) type Staleness = HashMap<Mirror, Lag>;

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

/// What a block does to one binding's host array before its terminator runs.
#[derive(Clone, Copy)]
enum BlockEvent {
    /// A host read of a gpu binding, before its handle is resolved.
    Read {
        index: usize,
        local: Local,
        span: Span,
    },
    /// `target` takes over `source`'s host array, and with it its lag.
    Adopt { target: Local, source: Local },
}

/// The readbacks each block needs, keyed by block, in the order its reads run.
pub(super) fn planned_refreshes(body: &Body) -> BTreeMap<usize, Vec<Refresh>> {
    let destinations = readback_destinations(body);
    let carriers = HandleCarriers::of(body);
    let events: Vec<Vec<BlockEvent>> = (0..body.basic_blocks.len())
        .map(|bb| block_events(body, bb, &destinations))
        .collect();
    staleness_on_entry(body, &carriers, &events)
        .into_iter()
        .filter_map(|(bb, entry)| {
            let refreshes = refreshes_in(body, &events[bb], entry);
            (!refreshes.is_empty()).then_some((bb, refreshes))
        })
        .collect()
}

/// Every mirror that lags on entry to the body; see
/// [`HandleCarriers::lagging_at_body_entry`].
pub(super) fn staleness_at_body_entry(body: &Body, carriers: &HandleCarriers) -> Staleness {
    carriers
        .lagging_at_body_entry(body)
        .into_iter()
        .map(|mirror| (mirror, Lag::Behind))
        .collect()
}

/// The host reads and adoptions of block `bb`, statement by statement, then
/// its terminator's reads.
fn block_events(body: &Body, bb: usize, destinations: &HashSet<Local>) -> Vec<BlockEvent> {
    let block = &body.basic_blocks[bb];
    let statement_events = block.statements.iter().enumerate().flat_map(|(index, s)| {
        let reads = statement_host_reads(body, s, destinations)
            .into_iter()
            .map(move |local| BlockEvent::Read {
                index,
                local,
                span: s.span,
            });
        let adoption = statement_adoption(body, s)
            .map(|(target, source)| BlockEvent::Adopt { target, source });
        reads.chain(adoption)
    });
    let terminator_reads = block.terminator.iter().flat_map(|t| {
        terminator_host_reads(body, &t.kind)
            .into_iter()
            .map(move |local| BlockEvent::Read {
                index: block.statements.len(),
                local,
                span: t.span,
            })
    });
    statement_events.chain(terminator_reads).collect()
}

/// The staleness on entry to every block reachable from the entry block.
///
/// Lags only ever grow at a join — current and behind make maybe-behind — so
/// the worklist reaches a fixpoint.
fn staleness_on_entry(
    body: &Body,
    carriers: &HandleCarriers,
    events: &[Vec<BlockEvent>],
) -> BTreeMap<usize, Staleness> {
    let mut entries = BTreeMap::new();
    if body.basic_blocks.is_empty() {
        return entries;
    }
    entries.insert(0, staleness_at_body_entry(body, carriers));
    let mut worklist = VecDeque::from([0]);
    while let Some(bb) = worklist.pop_front() {
        let exit = staleness_after(body, carriers, bb, &events[bb], entries[&bb].clone());
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
/// its binding's mirror current, each adoption hands the target the source's
/// lag, then the terminator's effect applies.
fn staleness_after(
    body: &Body,
    carriers: &HandleCarriers,
    bb: usize,
    events: &[BlockEvent],
    mut staleness: Staleness,
) -> Staleness {
    for event in events {
        apply_event(body, event, &mut staleness);
    }
    if let Some(terminator) = &body.basic_blocks[bb].terminator {
        let changes = carriers.changes(&device_effect(&terminator.kind));
        for mirror in changes.current {
            staleness.remove(&mirror);
        }
        for mirror in changes.lagging {
            staleness.insert(mirror, Lag::Behind);
        }
    }
    staleness
}

/// Apply one block event to `staleness`, returning the lag a read found.
fn apply_event(body: &Body, event: &BlockEvent, staleness: &mut Staleness) -> Option<Lag> {
    match *event {
        BlockEvent::Read { local, .. } => {
            let handle = gpu_handle(body, local)?;
            staleness.remove(&(handle.0, local))
        }
        BlockEvent::Adopt { target, source } => {
            let handle = gpu_handle(body, target)?.0;
            match staleness.get(&(handle, source)).copied() {
                Some(lag) => staleness.insert((handle, target), lag),
                None => staleness.remove(&(handle, target)),
            };
            None
        }
    }
}

/// Merge `from` into `into`, reporting whether `into` changed.
fn join(into: &mut Staleness, from: &Staleness) -> bool {
    let before = into.clone();
    for (mirror, lag) in into.iter_mut() {
        if from.get(mirror) != Some(lag) {
            *lag = Lag::MaybeBehind;
        }
    }
    for mirror in from.keys() {
        into.entry(*mirror).or_insert(Lag::MaybeBehind);
    }
    *into != before
}

/// The reads of a block that find their mirror lagging, given `staleness` on
/// entry. A read after the first of a binding finds it current.
fn refreshes_in(body: &Body, events: &[BlockEvent], mut staleness: Staleness) -> Vec<Refresh> {
    events
        .iter()
        .filter_map(|event| {
            let lag = apply_event(body, event, &mut staleness)?;
            let BlockEvent::Read { index, local, span } = *event else {
                return None;
            };
            Some(Refresh {
                index,
                local,
                handle: gpu_handle(body, local)?,
                lag,
                span,
            })
        })
        .collect()
}
