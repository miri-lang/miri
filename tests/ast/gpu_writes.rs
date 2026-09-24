// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The write walk shared by `forall` lowering and the `gpu frame` checker.

use miri::ast::gpu_writes::{visit_buffer_writes, BufferWrite};
use miri::lexer::Lexer;
use miri::parser::Parser;
use std::collections::BTreeSet;

/// Every `(name, kind)` write the walk reports for the statements of `source`,
/// with the atomic builtins the GPU target recognizes.
fn writes(source: &str) -> BTreeSet<(String, &'static str)> {
    let mut lexer = Lexer::new(source);
    let program = Parser::new(&mut lexer, source)
        .parse()
        .expect("the source parses");
    let mut found = BTreeSet::new();
    for stmt in &program.body {
        visit_buffer_writes(stmt, &mut |name, kind| {
            let kind = match kind {
                BufferWrite::Store => "store",
                BufferWrite::Atomic => "atomic",
            };
            found.insert((name.to_string(), kind));
        });
    }
    found
}

fn set(entries: &[(&str, &'static str)]) -> BTreeSet<(String, &'static str)> {
    entries.iter().map(|(n, k)| (n.to_string(), *k)).collect()
}

#[test]
fn store_inside_a_match_arm_is_a_write() {
    let found = writes("match k\n    0: a[0] = 1\n    _: b[1] = 2\n");
    assert_eq!(found, set(&[("a", "store"), ("b", "store")]));
}

#[test]
fn store_in_a_match_nested_in_an_if_is_a_write() {
    let found = writes("if k > 0\n    match k\n        1: c[k] = 1\n        _: d.x = 2\n");
    assert_eq!(found, set(&[("c", "store"), ("d", "store")]));
}

#[test]
fn store_through_nested_projections_is_rooted_at_the_variable() {
    let found = writes("grid[i][j] = 1\npoints[i].x = 2.0\n");
    assert_eq!(found, set(&[("grid", "store"), ("points", "store")]));
}

#[test]
fn atomic_in_a_declaration_initializer_is_an_atomic_write() {
    let found = writes("let old = atomic_add(hits, 0, 1)\n");
    assert_eq!(found, set(&[("hits", "atomic")]));
}

#[test]
fn a_call_that_is_not_atomic_writes_nothing() {
    assert!(writes("foo(buf, 0)\n").is_empty());
}

#[test]
fn a_store_inside_a_lambda_body_is_not_a_write_of_the_loop() {
    assert!(writes("let f = fn(x int): buf[0] = x\n").is_empty());
}

#[test]
fn every_atomic_builtin_of_the_gpu_target_is_an_atomic_write() {
    let found = writes("atomic_max(peak, 0, 3)\natomic_compare_exchange(slot, 0, 1, 2)\n");
    assert_eq!(found, set(&[("peak", "atomic"), ("slot", "atomic")]));
}
