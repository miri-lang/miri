// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The write walk shared by `forall` lowering and the `gpu frame` checker.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::gpu_writes::{visit_buffer_writes, BufferWrite, OutArgumentQuery};
use miri::lexer::Lexer;
use miri::parser::Parser;
use std::collections::BTreeSet;

/// A call record in which no argument binds an `out` parameter.
struct NoOutArguments;

impl OutArgumentQuery for NoOutArguments {
    fn binds_out(&self, _call: &Expression, _argument: &Expression) -> bool {
        false
    }
}

/// A call record, filled by hand, in which the first argument of every call
/// to the function `callee` binds an `out` parameter. A named argument is
/// recorded as the value it names, as the type checker records it.
struct FirstArgumentOutOf(&'static str);

impl OutArgumentQuery for FirstArgumentOutOf {
    fn binds_out(&self, call: &Expression, argument: &Expression) -> bool {
        let ExpressionKind::Call(callee, args) = &call.node else {
            return false;
        };
        let is_callee =
            matches!(&callee.node, ExpressionKind::Identifier(name, _) if name == self.0);
        let first = args.first().map(|first| match &first.node {
            ExpressionKind::NamedArgument(_, value) => value.as_ref(),
            _ => first,
        });
        is_callee && first.is_some_and(|first| first.id == argument.id)
    }
}

/// How a test spells the index an `out` write carries: the name it reads, or
/// `<expr>` for anything else.
fn spelled_index(index: Option<&Expression>) -> String {
    match index.map(|index| &index.node) {
        Some(ExpressionKind::Identifier(name, _)) => name.clone(),
        Some(_) => "<expr>".to_string(),
        None => "-".to_string(),
    }
}

/// Every `(name, kind)` write the walk reports for the statements of
/// `source`, reading which arguments bind an `out` parameter off `outs`.
fn writes_with(source: &str, outs: &dyn OutArgumentQuery) -> BTreeSet<(String, String)> {
    let mut lexer = Lexer::new(source);
    let program = Parser::new(&mut lexer, source)
        .parse()
        .expect("the source parses");
    let mut found = BTreeSet::new();
    for stmt in &program.body {
        visit_buffer_writes(stmt, outs, &mut |name, kind| {
            let kind = match kind {
                BufferWrite::Store => "store".to_string(),
                BufferWrite::Atomic => "atomic".to_string(),
                BufferWrite::Out { index } => format!("out {}", spelled_index(index)),
            };
            found.insert((name.to_string(), kind));
        });
    }
    found
}

/// Every `(name, kind)` write the walk reports for the statements of `source`,
/// with the atomic builtins the GPU target recognizes and no `out` argument.
fn writes(source: &str) -> BTreeSet<(String, String)> {
    writes_with(source, &NoOutArguments)
}

fn set(entries: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    entries
        .iter()
        .map(|(n, k)| (n.to_string(), k.to_string()))
        .collect()
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

#[test]
fn an_element_handed_to_an_out_parameter_is_written_at_its_index() {
    let found = writes_with("bump(dst[i])\n", &FirstArgumentOutOf("bump"));
    assert_eq!(found, set(&[("dst", "out i")]));
}

#[test]
fn a_field_of_an_element_handed_out_carries_the_index_at_its_root() {
    let found = writes_with("bump(pts[j].x)\n", &FirstArgumentOutOf("bump"));
    assert_eq!(found, set(&[("pts", "out j")]));
}

#[test]
fn a_whole_binding_handed_out_carries_no_index() {
    let found = writes_with("let y = bump(total)\n", &FirstArgumentOutOf("bump"));
    assert_eq!(found, set(&[("total", "out -")]));
}

#[test]
fn a_named_argument_handed_out_is_written() {
    let found = writes_with("bump(x: dst[k + 1])\n", &FirstArgumentOutOf("bump"));
    assert_eq!(found, set(&[("dst", "out <expr>")]));
}

#[test]
fn an_argument_the_record_does_not_name_out_is_only_read() {
    let found = writes_with("bump(dst[i], src[i])\n", &FirstArgumentOutOf("bump"));
    assert_eq!(found, set(&[("dst", "out i")]));
}

#[test]
fn an_out_argument_inside_a_branch_is_a_write() {
    let found = writes_with("if k > 0\n    bump(dst[k])\n", &FirstArgumentOutOf("bump"));
    assert_eq!(found, set(&[("dst", "out k")]));
}
