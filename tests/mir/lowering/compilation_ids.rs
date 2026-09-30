// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::compilation_ids::CompilationIds;
use miri::mir::symbol::InstantiationKey;

/// The key of a body lowered at no instantiation.
fn plain() -> InstantiationKey {
    InstantiationKey::default()
}

fn at(kind: TypeKind) -> InstantiationKey {
    InstantiationKey::of(&[Type::new(kind, Span::default())])
}

#[test]
fn assigns_sequential_indices_in_first_seen_order() {
    let mut namer = CompilationIds::default();
    // Large, non-contiguous AST ids stand in for the process-global counter.
    assert_eq!(namer.index_for(9001, &plain()), 0);
    assert_eq!(namer.index_for(42, &plain()), 1);
    assert_eq!(namer.index_for(500, &plain()), 2);
}

#[test]
fn same_ast_id_maps_to_same_index() {
    let mut namer = CompilationIds::default();
    let first = namer.index_for(7, &plain());
    assert_eq!(namer.index_for(99, &plain()), 1);
    // Re-querying an already-seen node returns its original index, not a new one.
    assert_eq!(namer.index_for(7, &plain()), first);
}

#[test]
fn fresh_namer_reproduces_indices_for_the_same_id_sequence() {
    // Two compilations see AST ids from different regions of the global
    // counter, but a fresh namer each time yields identical indices for the
    // same relative order of nodes.
    let mut first = CompilationIds::default();
    let a = [10, 20, 30].map(|id| first.index_for(id, &plain()));

    let mut second = CompilationIds::default();
    let b = [1010, 1020, 1030].map(|id| second.index_for(id, &plain()));

    assert_eq!(a, b);
    assert_eq!(a, [0, 1, 2]);
}

/// One node lowered under two instantiations is two kernels, which may emit
/// different WGSL; the same instantiation reaching it again is the same one.
#[test]
fn one_node_under_two_instantiations_takes_two_indices() {
    let mut namer = CompilationIds::default();
    let at_int = namer.index_for(7, &at(TypeKind::Int));
    let at_f32 = namer.index_for(7, &at(TypeKind::F32));
    assert_ne!(at_int, at_f32);
    assert_eq!(namer.index_for(7, &at(TypeKind::Int)), at_int);
    assert_eq!(namer.index_for(7, &plain()), 2);
}
