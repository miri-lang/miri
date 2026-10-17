// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that names each computed index by the first temp
//! holding its value.
//!
//! Each program indexes one list through `i + 1` more than once; the tests
//! count how many different locals the compiled body indexes it through.

use std::collections::HashSet;

use miri::mir::place::PlaceContext;
use miri::mir::visitor::Visitor;
use miri::mir::{BasicBlock, Body, Local, Place};
use miri::pipeline::Pipeline;

/// The body `source` compiles to whose link name ends in `suffix`.
fn body_named(source: &str, suffix: &str) -> Body {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("pipeline should lower the program")
        .into_iter()
        .find(|(name, _)| name.ends_with(suffix))
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("no body named *{suffix}"))
}

/// The locals some place of `body` is indexed by.
fn index_locals(body: &Body) -> HashSet<Local> {
    struct Indexes(HashSet<Local>);
    impl Visitor for Indexes {
        fn visit_place(&mut self, place: &Place, _context: PlaceContext, _location: BasicBlock) {
            self.0.extend(place.index_locals());
        }
    }
    let mut indexes = Indexes(HashSet::new());
    indexes.visit_body(body);
    indexes.0
}

fn program(function: &str) -> String {
    format!(
        "use system.io\nuse system.collections.list\n\nstruct Cell\n    x int\n    y int\n\n{function}\n\nfn main()\n    println(\"ok\")\n"
    )
}

#[test]
fn test_one_computed_index_is_named_by_one_temp() {
    let source = program(
        r#"
fn touch(start [Cell], i int) [Cell]
    var l = start
    l[i + 1].x = 1
    l[i + 1].y = 2
    l[i + 1].x = 3
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 1);
}

#[test]
fn test_writing_an_operand_between_indices_keeps_two_temps() {
    let source = program(
        r#"
fn touch(start [Cell], j int) [Cell]
    var l = start
    var i = j
    l[i + 1].x = 1
    i = j + 1
    l[i + 1].y = 2
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 2);
}

#[test]
fn test_different_expressions_keep_their_own_temps() {
    let source = program(
        r#"
fn touch(start [Cell], i int) [Cell]
    var l = start
    l[i + 1].x = 1
    l[i - 1].y = 2
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 2);
}

#[test]
fn test_an_index_computed_on_one_branch_only_keeps_its_own_temp() {
    // The temp written on one branch does not reach the join on the other,
    // so the write after the join cannot be named by it.
    let source = program(
        r#"
fn touch(start [Cell], i int, flip bool) [Cell]
    var l = start
    if flip
        l[i + 1].x = 1
    l[i + 1].y = 2
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 2);
}

#[test]
fn test_three_writes_across_branches_are_named_by_the_first_temp() {
    // The temps after each join are named by the one written before the
    // branch, whichever block order the branches are laid out in.
    let source = program(
        r#"
fn touch(start [Cell], i int, flip bool) [Cell]
    var l = start
    l[i + 1].x = 1
    if flip
        l[i + 1].y = 2
    else
        l[i + 1].y = 3
    l[i + 1].x = 4
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 1);
}

#[test]
fn test_an_index_in_a_loop_is_named_within_one_iteration_only() {
    // `i` changes at the end of each iteration, so the temps of one iteration
    // share a name and the index before the loop keeps its own.
    let source = program(
        r#"
fn touch(start [Cell], n int) [Cell]
    var l = start
    var i = 0
    l[i + 1].x = 0
    while i < n
        l[i + 1].x = i
        l[i + 1].y = i
        i = i + 1
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 2);
}

#[test]
fn test_a_negated_index_is_named_like_a_sum() {
    let source = program(
        r#"
fn touch(start [Cell], i int) [Cell]
    var l = start
    l[-i].x = 1
    l[-i].y = 2
    l
"#,
    );
    assert_eq!(index_locals(&body_named(&source, ".touch")).len(), 1);
}
