// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which functions a program's bodies call, directly and transitively.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::mir::symbol::Symbol;
use crate::mir::{Body, TerminatorKind};

/// The functions reached from some starting bodies by following their direct
/// calls, each yielded once, the starting ones included.
pub struct CalleeWalk<'m, 'b> {
    body_of: &'m HashMap<&'b Symbol, &'b Body>,
    visited: HashSet<&'b Symbol>,
    queue: VecDeque<&'b Symbol>,
}

impl<'m, 'b> CalleeWalk<'m, 'b> {
    pub fn from(
        body_of: &'m HashMap<&'b Symbol, &'b Body>,
        starts: impl IntoIterator<Item = &'b Symbol>,
    ) -> Self {
        let mut walk = Self {
            body_of,
            visited: HashSet::new(),
            queue: VecDeque::new(),
        };
        for start in starts {
            if walk.visited.insert(start) {
                walk.queue.push_back(start);
            }
        }
        walk
    }
}

impl<'b> Iterator for CalleeWalk<'_, 'b> {
    type Item = &'b Symbol;

    fn next(&mut self) -> Option<&'b Symbol> {
        let current = self.queue.pop_front()?;
        if let Some(body) = self.body_of.get(current) {
            for callee in direct_callees(body) {
                if self.visited.insert(callee) {
                    self.queue.push_back(callee);
                }
            }
        }
        Some(current)
    }
}

/// The functions a body calls directly.
pub fn direct_callees(body: &Body) -> impl Iterator<Item = &Symbol> {
    body.basic_blocks.iter().filter_map(|block| {
        let TerminatorKind::Call { func, .. } = &block.terminator.as_ref()?.kind else {
            return None;
        };
        func.called_symbol()
    })
}
