// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Per-compilation identifier allocators.
//!
//! Some identifiers chosen during lowering are emitted into the generated
//! artifact: GPU kernel entry names, and the device handles that name a
//! persistent device buffer across dispatches. Both must be unique within a
//! single build and identical across builds of the same source.
//!
//! A process-global counter satisfies uniqueness but not stability. A
//! long-lived compiler host — an agent session, a watch loop, an IDE backend —
//! keeps such a counter advancing between compilations, so the same source
//! yields different names and different handles on each build, and therefore
//! different bytes. [`CompilationIds`] decouples both id spaces from process
//! lifetime: one allocator per compilation, shared across every body lowered in
//! it, so the same source always reproduces the same ids.
//!
//! Kernel indices are keyed by AST node id, which keeps the assignment
//! idempotent: a node lowered more than once in a compilation (e.g. a generic
//! method reached by two instantiations sharing one device kernel) always maps
//! to the same index. Device handles have no such node to key on and are handed
//! out sequentially in allocation order.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The id allocators shared across the bodies of one compilation.
pub type SharedCompilationIds = Rc<RefCell<CompilationIds>>;

/// Builds a fresh, empty [`SharedCompilationIds`]. One is created per
/// compilation (so ids reset per compilation) and shared across every body
/// lowered in that compilation (so ids stay unique within the build).
pub fn new_shared_compilation_ids() -> SharedCompilationIds {
    Rc::new(RefCell::new(CompilationIds::default()))
}

/// Allocates compilation-local, deterministic kernel indices and device handles.
///
/// Shared (via `Rc<RefCell<_>>`) across every function body lowered in a single
/// compilation so that ids are unique within the build, and reset per
/// compilation so that the same source always produces the same ids.
#[derive(Debug)]
pub struct CompilationIds {
    /// AST node id → assigned kernel index, keeping assignment idempotent per node.
    assigned: HashMap<usize, usize>,
    /// Next kernel index to hand out, in first-seen order.
    next: usize,
    /// Next device handle to hand out. The runtime reserves `0` as the
    /// host-resident sentinel, so allocation starts at `1`.
    next_device_handle: u64,
}

impl Default for CompilationIds {
    fn default() -> Self {
        Self {
            assigned: HashMap::new(),
            next: 0,
            next_device_handle: 1,
        }
    }
}

impl CompilationIds {
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

    /// Hands out the next device handle for this compilation.
    pub fn fresh_device_handle(&mut self) -> u64 {
        let handle = self.next_device_handle;
        self.next_device_handle += 1;
        handle
    }
}
