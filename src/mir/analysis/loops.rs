// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The natural loops of a body, and the edges they are found along.
//!
//! An edge from a block to one that dominates it closes a loop: the target is
//! the loop's header, the source one of its latches, and the loop is every
//! block that reaches a latch without passing through the header. Loops
//! sharing a header are merged into one, so each header names one loop.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::{BasicBlock, Body};

/// A natural loop: its header and every block of it, the header included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NaturalLoop {
    /// The block every entry into the loop passes through.
    pub header: usize,
    /// Every block of the loop, the header included.
    pub blocks: BTreeSet<usize>,
    /// The blocks with an edge back to the header.
    pub latches: Vec<usize>,
}

impl NaturalLoop {
    /// The blocks outside the loop with an edge into its header.
    pub fn entries(&self, predecessors: &[Vec<usize>]) -> Vec<usize> {
        predecessors
            .get(self.header)
            .into_iter()
            .flatten()
            .copied()
            .filter(|pred| !self.blocks.contains(pred))
            .collect()
    }
}

/// Every natural loop of `body`, one per header, with the blocks of all the
/// loops sharing that header merged.
pub fn natural_loops(body: &Body, dominators: &DominatorTree) -> Vec<NaturalLoop> {
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
            NaturalLoop {
                header,
                blocks,
                latches,
            }
        })
        .collect()
}

/// The blocks `block`'s terminator may continue to.
pub fn successors(body: &Body, block: usize) -> Vec<usize> {
    body.basic_blocks
        .get(block)
        .and_then(|data| data.terminator.as_ref())
        .map(|terminator| terminator.successors().iter().map(|b| b.0).collect())
        .unwrap_or_default()
}

/// The blocks with an edge into each block, by block index.
pub fn predecessors(body: &Body) -> Vec<Vec<usize>> {
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
