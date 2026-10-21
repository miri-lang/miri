// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check that every access recorded in bounds is proven so.
//! Each entry is forged onto a body that does not prove it.

use crate::mir::optimization::in_bounds::proofs_of;
use crate::mir::optimization::stable_collections::{
    assign, int, loop_over_a_list, INDEX, LENGTH, LIST, LOOP_BODY, LOOP_HEADER,
};
use miri::mir::optimization::in_bounds::record_in_bounds;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::verify::verify_in_bounds;
use miri::mir::{BasicBlock, BinOp, Body, InBounds, Operand, Place, Rvalue, StatementKind};

/// `body` with its stable collections recorded and `proof` forged on.
fn forged(mut body: Body, proof: InBounds) -> Body {
    record_stable_collections(&mut body);
    body.in_bounds = vec![proof];
    body
}

fn read_at(position: usize) -> InBounds {
    InBounds {
        block: BasicBlock(LOOP_BODY),
        statement_index: position,
        collection: LIST,
        index: INDEX,
    }
}

fn messages(body: &Body) -> Vec<String> {
    verify_in_bounds(body)
        .into_iter()
        .map(|violation| violation.message)
        .collect()
}

#[test]
fn test_an_entry_the_pass_made_has_no_violation() {
    let mut body = loop_over_a_list();
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    assert_eq!(body.in_bounds.len(), 1);
    assert_eq!(messages(&body), Vec::<String>::new());
}

#[test]
fn test_an_entry_after_the_index_is_redefined_is_flagged() {
    let mut body = loop_over_a_list();
    let step = Rvalue::BinaryOp(
        BinOp::Add,
        Box::new(Operand::Copy(Place::new(INDEX))),
        Box::new(int(1)),
    );
    body.basic_blocks[LOOP_BODY]
        .statements
        .insert(0, assign(Place::new(INDEX), step));
    assert!(proofs_of(body.clone()).is_empty());
    let found = messages(&forged(body, read_at(1)));
    assert!(
        found.iter().any(|m| m.contains("no guard proves it")),
        "{found:?}"
    );
}

#[test]
fn test_an_entry_under_a_less_or_equal_guard_is_flagged() {
    let mut body = loop_over_a_list();
    let StatementKind::Assign(_, Rvalue::BinaryOp(op, ..)) =
        &mut body.basic_blocks[LOOP_HEADER].statements[1].kind
    else {
        panic!("the header compares the index to the length");
    };
    *op = BinOp::Le;
    let found = messages(&forged(body, read_at(0)));
    assert!(
        found.iter().any(|m| m.contains("no guard proves it")),
        "{found:?}"
    );
}

#[test]
fn test_an_entry_against_a_bound_that_is_not_the_length_is_flagged() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_HEADER].statements[0] = assign(Place::new(LENGTH), Rvalue::Use(int(3)));
    let found = messages(&forged(body, read_at(0)));
    assert!(
        found.iter().any(|m| m.contains("no guard proves it")),
        "{found:?}"
    );
}

#[test]
fn test_an_entry_no_stable_record_covers_is_flagged() {
    let mut body = loop_over_a_list();
    body.in_bounds = vec![read_at(0)];
    let found = messages(&body);
    assert!(
        found.iter().any(|m| m.contains("no stable record")),
        "{found:?}"
    );
}

#[test]
fn test_an_entry_where_nothing_indexes_the_list_is_flagged() {
    let found = messages(&forged(loop_over_a_list(), read_at(1)));
    assert!(
        found.iter().any(|m| m.contains("nothing there indexes it")),
        "{found:?}"
    );
}
