// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The natural loops of a body, and the edges they are found along.
//!
//! An edge from a block to one that dominates it closes a loop: the target is
//! the loop's header, the source one of its latches, and the loop is every
//! block that reaches a latch without passing through the header. Loops
//! sharing a header are merged into one, so each header names one loop.
//!
//! A loop is commonly guarded by a comparison its header computes and then
//! switches on; [`comparison_before_switch`] reads it.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::mir::analysis::dominators::DominatorTree;
use crate::mir::{BasicBlock, BinOp, Body, Local, Rvalue, Statement, StatementKind};

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

/// A comparison of two whole locals: `lhs < rhs` when `strict`, and
/// `lhs <= rhs` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Comparison {
    pub lhs: Local,
    pub rhs: Local,
    pub strict: bool,
}

/// The comparison `condition` holds at the end of `block`: the last
/// statement of `block` writing `condition` is `condition = Lt(lhs, rhs)` or
/// `condition = Le(lhs, rhs)`, or a whole-local copy `condition = c` of a
/// `c` the last statement before it writing `c` so computes — the shape `and`
/// and `or` lower to — and nothing after the comparison writes `lhs` or
/// `rhs`. One copy is followed, not a chain.
pub fn comparison_before_switch(body: &Body, block: usize, condition: Local) -> Option<Comparison> {
    let statements = &body.basic_blocks.get(block)?.statements;
    let last_write = |end: usize, local: Local| {
        (statements.get(..end)?.iter()).rposition(|statement| writes_whole(statement, local))
    };
    let mut position = last_write(statements.len(), condition)?;
    if let Some(source) = copied_local(statements.get(position)?) {
        position = last_write(position, source)?;
    }
    let (op, lhs, rhs) = compared_locals(statements.get(position)?)?;
    let strict = match op {
        BinOp::Lt => true,
        BinOp::Le => false,
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::Eq
        | BinOp::Ne
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::Offset => return None,
    };
    let later = statements.get(position + 1..).unwrap_or_default();
    let is_kept = later
        .iter()
        .all(|statement| !writes_whole(statement, lhs) && !writes_whole(statement, rhs));
    is_kept.then_some(Comparison { lhs, rhs, strict })
}

/// `c` when `statement` gives a whole local the value of the whole local `c`.
fn copied_local(statement: &Statement) -> Option<Local> {
    match &statement.kind {
        StatementKind::Assign(_, Rvalue::Use(operand))
        | StatementKind::Reassign(_, Rvalue::Use(operand)) => operand.bare_local(),
        StatementKind::Assign(..)
        | StatementKind::Reassign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => None,
    }
}

/// `(op, lhs, rhs)` when `statement` assigns `op` over two whole locals.
fn compared_locals(statement: &Statement) -> Option<(BinOp, Local, Local)> {
    match &statement.kind {
        StatementKind::Assign(_, Rvalue::BinaryOp(op, lhs, rhs))
        | StatementKind::Reassign(_, Rvalue::BinaryOp(op, lhs, rhs)) => {
            Some((*op, lhs.bare_local()?, rhs.bare_local()?))
        }
        StatementKind::Assign(..)
        | StatementKind::Reassign(..)
        | StatementKind::StorageLive(_)
        | StatementKind::StorageDead(_)
        | StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => None,
    }
}

/// Whether `statement` gives the whole of `local` a value or starts or ends
/// its storage.
pub fn writes_whole(statement: &Statement, local: Local) -> bool {
    match &statement.kind {
        StatementKind::Assign(place, _)
        | StatementKind::Reassign(place, _)
        | StatementKind::StorageLive(place)
        | StatementKind::StorageDead(place) => place.local == local && place.projection.is_empty(),
        StatementKind::IncRef(_)
        | StatementKind::DecRef(_)
        | StatementKind::Dealloc(_)
        | StatementKind::Nop => false,
    }
}
