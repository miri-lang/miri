// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Orders in which to walk a body's control-flow graph, and the facts of a
//! body every analysis reads the same way: each block's successors and the
//! locals whose address is taken. Each is defined here once.

use std::collections::HashSet;

use crate::mir::{Body, Local, Rvalue, StatementKind};

/// The blocks the entry reaches, each before its successors except along a
/// back edge. A forward dataflow visiting blocks in this order sees every
/// block's predecessors first wherever no loop intervenes.
pub fn reverse_postorder(body: &Body) -> Vec<usize> {
    let count = body.basic_blocks.len();
    let mut visited = vec![false; count];
    let mut postorder = Vec::with_capacity(count);
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    if let Some(entry) = visited.first_mut() {
        *entry = true;
        stack.push((0, successors_of(body, 0)));
    }
    while let Some((block, pending)) = stack.last_mut() {
        let block = *block;
        let Some(next) = pending.pop() else {
            postorder.push(block);
            stack.pop();
            continue;
        };
        if let Some(seen) = visited.get_mut(next).filter(|seen| !**seen) {
            *seen = true;
            stack.push((next, successors_of(body, next)));
        }
    }
    postorder.reverse();
    postorder
}

/// The blocks `block`'s terminator may continue to.
pub fn successors_of(body: &Body, block: usize) -> Vec<usize> {
    (body.basic_blocks.get(block))
        .and_then(|data| data.terminator.as_ref())
        .map(|terminator| terminator.successors().iter().map(|b| b.0).collect())
        .unwrap_or_default()
}

/// The root of every place whose address `body` takes anywhere.
pub fn address_taken(body: &Body) -> HashSet<Local> {
    (body.basic_blocks.iter())
        .flat_map(|data| &data.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, Rvalue::Ref(place))
            | StatementKind::Reassign(_, Rvalue::Ref(place)) => Some(place.local),
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop
            | StatementKind::ReadHeader(_) => None,
        })
        .collect()
}
