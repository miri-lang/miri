// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Orders in which to walk a body's control-flow graph.

use crate::mir::{BasicBlock, Body, Terminator};

/// The blocks the entry reaches, each before its successors except along a
/// back edge. A forward dataflow visiting blocks in this order sees every
/// block's predecessors first wherever no loop intervenes.
pub fn reverse_postorder(body: &Body) -> Vec<usize> {
    let count = body.basic_blocks.len();
    let mut visited = vec![false; count];
    let mut postorder = Vec::with_capacity(count);
    let mut stack: Vec<(usize, Vec<BasicBlock>)> = Vec::new();
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
        if let Some(seen) = visited.get_mut(next.0).filter(|seen| !**seen) {
            *seen = true;
            stack.push((next.0, successors_of(body, next.0)));
        }
    }
    postorder.reverse();
    postorder
}

fn successors_of(body: &Body, block: usize) -> Vec<BasicBlock> {
    body.basic_blocks
        .get(block)
        .and_then(|data| data.terminator.as_ref())
        .map(Terminator::successors)
        .unwrap_or_default()
}
