// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Per-compilation kernel-name allocator.
//!
//! GPU kernel entry names must be unique within a single build and stable
//! across builds of the same source. Using the raw AST node id (drawn from a
//! process-global counter that never resets) satisfies uniqueness within one
//! build but not stability across builds: a long-lived compiler host (REPL,
//! daemon, IDE backend) would give the same source different kernel names on
//! each compilation, because the global counter keeps advancing.
//!
//! [`KernelNamer`] decouples kernel naming from that global counter. It hands
//! each kernel-bearing AST node a sequential index assigned in first-seen order
//! within one compilation. A fresh namer per compilation therefore reproduces
//! the same indices — and the same kernel names — for the same source every
//! time. Keying by AST id keeps the assignment idempotent: a node lowered more
//! than once in a compilation (e.g. a generic method reached by two
//! instantiations that share one device kernel) always maps to the same index.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A kernel-name allocator shared across the bodies of one compilation.
pub type SharedKernelNamer = Rc<RefCell<KernelNamer>>;

/// Builds a fresh, empty [`SharedKernelNamer`]. One is created per compilation
/// (so kernel names reset per compilation) and shared across every body lowered
/// in that compilation (so names stay unique within the build).
pub fn new_shared_kernel_namer() -> SharedKernelNamer {
    Rc::new(RefCell::new(KernelNamer::default()))
}

/// Allocates compilation-local, deterministic indices for kernel names.
///
/// Shared (via `Rc<RefCell<_>>`) across every function body lowered in a single
/// compilation so that indices are globally unique within the build, and reset
/// per compilation so that the same source always produces the same indices.
#[derive(Debug, Default)]
pub struct KernelNamer {
    /// AST node id → assigned index, keeping assignment idempotent per node.
    assigned: HashMap<usize, usize>,
    /// Next index to hand out, in first-seen order.
    next: usize,
}

impl KernelNamer {
    /// Returns the compilation-local index for `ast_id`, assigning the next
    /// sequential index the first time this node is seen and returning the same
    /// index on any later lookup of the same node.
    pub fn index_for(&mut self, ast_id: usize) -> usize {
        if let Some(&index) = self.assigned.get(&ast_id) {
            return index;
        }
        let index = self.next;
        self.next += 1;
        self.assigned.insert(ast_id, index);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_sequential_indices_in_first_seen_order() {
        let mut namer = KernelNamer::default();
        // Large, non-contiguous AST ids stand in for the process-global counter.
        assert_eq!(namer.index_for(9001), 0);
        assert_eq!(namer.index_for(42), 1);
        assert_eq!(namer.index_for(500), 2);
    }

    #[test]
    fn same_ast_id_maps_to_same_index() {
        let mut namer = KernelNamer::default();
        let first = namer.index_for(7);
        assert_eq!(namer.index_for(99), 1);
        // Re-querying an already-seen node returns its original index, not a new one.
        assert_eq!(namer.index_for(7), first);
    }

    #[test]
    fn fresh_namer_reproduces_indices_for_the_same_id_sequence() {
        // Two compilations see AST ids from different regions of the global
        // counter, but a fresh namer each time yields identical indices for the
        // same relative order of nodes.
        let mut first = KernelNamer::default();
        let a = [10, 20, 30].map(|id| first.index_for(id));

        let mut second = KernelNamer::default();
        let b = [1010, 1020, 1030].map(|id| second.index_for(id));

        assert_eq!(a, b);
        assert_eq!(a, [0, 1, 2]);
    }
}
