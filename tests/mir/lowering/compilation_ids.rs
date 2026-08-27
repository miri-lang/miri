// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::mir::lowering::compilation_ids::CompilationIds;

#[test]
fn assigns_sequential_indices_in_first_seen_order() {
    let mut namer = CompilationIds::default();
    // Large, non-contiguous AST ids stand in for the process-global counter.
    assert_eq!(namer.index_for(9001), 0);
    assert_eq!(namer.index_for(42), 1);
    assert_eq!(namer.index_for(500), 2);
}

#[test]
fn same_ast_id_maps_to_same_index() {
    let mut namer = CompilationIds::default();
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
    let mut first = CompilationIds::default();
    let a = [10, 20, 30].map(|id| first.index_for(id));

    let mut second = CompilationIds::default();
    let b = [1010, 1020, 1030].map(|id| second.index_for(id));

    assert_eq!(a, b);
    assert_eq!(a, [0, 1, 2]);
}
