// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check that every access recorded in bounds is proven so.
//! Each entry is forged onto a body that does not prove it.

use crate::mir::optimization::in_bounds::{
    compare, loop_under_a_gap, loop_under_a_guard, proofs_of, reading_at_an_offset, stepped,
    switching_on_a_copy, two_pointer_loop, BOUND, GUARD, OFFSET_INDEX,
};
use crate::mir::optimization::stable_collections::{
    assign, call, int, loop_over_a_list, terminator, BELOW, INDEX, LENGTH, LIST, LOOP_BODY,
    LOOP_HEADER, SPARE, SPARE_LIST,
};
use miri::error::syntax::Span;
use miri::mir::optimization::in_bounds::record_in_bounds;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::verify::verify_in_bounds;
use miri::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Discriminant, InBounds, Local, Operand, Place, Rvalue,
    StatementKind, TerminatorKind,
};

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

/// Whether the verifier flags the read of the guarded test loop that `body`
/// does not prove, after checking the pass does not record it either.
fn flags_the_unproven_read(body: Body) -> bool {
    assert!(proofs_of(body.clone()).is_empty(), "{body}");
    let found = messages(&forged(body, read_at(0)));
    found.iter().any(|m| m.contains("no guard proves it"))
}

#[test]
fn test_an_entry_proven_by_a_bound_no_greater_than_the_length_has_no_violation() {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    assert_eq!(body.in_bounds.len(), 1);
    assert_eq!(messages(&body), Vec::<String>::new());
}

#[test]
fn test_an_entry_proven_by_a_bound_below_the_length_has_no_violation() {
    let mut body = loop_under_a_guard(compare(BinOp::Lt, BOUND, LENGTH), BinOp::Le);
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    assert_eq!(body.in_bounds.len(), 1);
    assert_eq!(messages(&body), Vec::<String>::new());
}

#[test]
fn test_an_entry_under_a_guard_comparing_the_wrong_way_is_flagged() {
    let body = loop_under_a_guard(compare(BinOp::Le, LENGTH, BOUND), BinOp::Lt);
    assert!(flags_the_unproven_read(body));
}

#[test]
fn test_an_entry_reached_on_the_false_edge_of_the_guard_is_flagged() {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    body.basic_blocks[0].terminator = Some(
        crate::mir::optimization::stable_collections::terminator(TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(GUARD)),
            targets: vec![(miri::mir::Discriminant::bool_true(), BasicBlock(3))],
            otherwise: BasicBlock(LOOP_HEADER),
        }),
    );
    assert!(flags_the_unproven_read(body));
}

#[test]
fn test_an_entry_up_to_a_bound_no_greater_than_the_length_is_flagged() {
    let body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Le);
    assert!(flags_the_unproven_read(body));
}

#[test]
fn test_an_entry_under_a_bound_changed_in_the_loop_is_flagged() {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    let grow = Rvalue::BinaryOp(
        BinOp::Add,
        Box::new(Operand::Copy(Place::new(BOUND))),
        Box::new(int(1)),
    );
    body.basic_blocks[LOOP_BODY]
        .statements
        .push(assign(Place::new(BOUND), grow));
    assert!(flags_the_unproven_read(body));
}

#[test]
fn test_an_entry_under_the_length_of_another_list_is_flagged() {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    body.basic_blocks[0].statements[1] =
        assign(Place::new(LENGTH), Rvalue::Len(Place::new(SPARE_LIST)));
    assert!(flags_the_unproven_read(body));
}

#[test]
fn test_an_entry_after_a_call_between_the_guard_and_the_loop_is_flagged() {
    let mut body = loop_under_a_guard(compare(BinOp::Le, BOUND, LENGTH), BinOp::Lt);
    let between = body.basic_blocks.len();
    let user_call = call(
        Operand::function(Symbol::entry(), Span::default()),
        Vec::new(),
        SPARE,
        LOOP_HEADER,
    );
    body.basic_blocks.push(BasicBlockData::new(Some(user_call)));
    if let Some(guard) = body.basic_blocks[0].terminator.as_mut() {
        guard.replace_successor(BasicBlock(LOOP_HEADER), BasicBlock(between));
    }
    assert!(flags_the_unproven_read(body));
}

/// `body` with its records made by the passes, after checking they recorded
/// `expected` entries and the verifier finds each proven.
fn checks_clean(mut body: Body, expected: usize) {
    record_stable_collections(&mut body);
    record_in_bounds(&mut body);
    assert_eq!(body.in_bounds.len(), expected, "{body}");
    assert_eq!(messages(&body), Vec::<String>::new());
}

/// Whether the verifier flags `proof` forged onto `body`, after checking
/// the pass does not record it.
fn flags(body: Body, proof: InBounds) -> bool {
    assert!(!proofs_of(body.clone()).contains(&proof), "{body}");
    let found = messages(&forged(body, proof));
    found.iter().any(|m| m.contains("no guard proves it"))
}

fn read_by(index: Local, position: usize) -> InBounds {
    InBounds {
        index,
        ..read_at(position)
    }
}

/// The guarded test loop `while i <op> n` entered when `n <guard> L`,
/// reading `list[i + offset]`.
fn offset_loop(guard: BinOp, op: BinOp, offset: i64) -> Body {
    let body = loop_under_a_guard(compare(guard, BOUND, LENGTH), op);
    reading_at_an_offset(body, stepped(BinOp::Add, INDEX, offset))
}

#[test]
fn test_an_entry_one_past_an_index_below_a_bound_below_the_length_has_no_violation() {
    checks_clean(offset_loop(BinOp::Lt, BinOp::Lt, 1), 1);
}

#[test]
fn test_an_entry_within_the_gap_below_the_length_has_no_violation() {
    let body = loop_under_a_gap(2, BinOp::Lt);
    checks_clean(reading_at_an_offset(body, stepped(BinOp::Add, INDEX, 2)), 1);
}

#[test]
fn test_an_entry_one_past_an_index_below_a_bound_at_most_the_length_is_flagged() {
    assert!(flags(
        offset_loop(BinOp::Le, BinOp::Lt, 1),
        read_by(OFFSET_INDEX, 1)
    ));
}

#[test]
fn test_an_entry_one_past_an_index_up_to_a_bound_below_the_length_is_flagged() {
    assert!(flags(
        offset_loop(BinOp::Lt, BinOp::Le, 1),
        read_by(OFFSET_INDEX, 1)
    ));
}

#[test]
fn test_an_entry_past_the_gap_below_the_length_is_flagged() {
    let body = loop_under_a_gap(2, BinOp::Lt);
    let body = reading_at_an_offset(body, stepped(BinOp::Add, INDEX, 3));
    assert!(flags(body, read_by(OFFSET_INDEX, 1)));
}

#[test]
fn test_an_entry_at_a_negative_offset_is_flagged() {
    assert!(flags(
        offset_loop(BinOp::Lt, BinOp::Lt, -1),
        read_by(OFFSET_INDEX, 1)
    ));
    let body = loop_under_a_guard(compare(BinOp::Lt, BOUND, LENGTH), BinOp::Lt);
    let body = reading_at_an_offset(body, stepped(BinOp::Sub, INDEX, 1));
    assert!(flags(body, read_by(OFFSET_INDEX, 1)));
}

#[test]
fn test_both_pointers_of_a_reversal_below_the_length_have_no_violation() {
    checks_clean(two_pointer_loop(BinOp::Lt, BinOp::Sub), 2);
}

#[test]
fn test_a_high_pointer_starting_at_the_length_is_flagged() {
    assert!(flags(
        two_pointer_loop(BinOp::Le, BinOp::Sub),
        read_by(BOUND, 1)
    ));
}

#[test]
fn test_a_high_pointer_that_counts_up_is_flagged() {
    assert!(flags(
        two_pointer_loop(BinOp::Lt, BinOp::Add),
        read_by(BOUND, 1)
    ));
}

#[test]
fn test_a_high_pointer_above_a_low_pointer_that_may_be_negative_is_flagged() {
    let mut body = two_pointer_loop(BinOp::Lt, BinOp::Sub);
    body.basic_blocks[0].statements[0] = assign(Place::new(INDEX), Rvalue::Use(int(-1)));
    assert!(flags(body.clone(), read_by(BOUND, 1)));
    assert!(flags(body, read_at(0)));
}

#[test]
fn test_an_entry_under_a_switch_on_a_copy_of_the_comparison_has_no_violation() {
    checks_clean(switching_on_a_copy(Vec::new()), 1);
}

#[test]
fn test_an_entry_under_a_copy_made_after_the_index_changed_is_flagged() {
    let between = vec![assign(Place::new(INDEX), Rvalue::Use(int(9)))];
    assert!(flags(switching_on_a_copy(between), read_at(0)));
}

#[test]
fn test_an_entry_under_a_copy_of_a_copy_of_the_comparison_is_flagged() {
    // `c' = Lt(i, L); c = c'; c = c`: the switch reads a copy of a copy.
    let mut body = switching_on_a_copy(Vec::new());
    body.basic_blocks[LOOP_HEADER].statements.push(assign(
        Place::new(BELOW),
        Rvalue::Use(Operand::Copy(Place::new(BELOW))),
    ));
    assert!(flags(body, read_at(0)));
}

#[test]
fn test_an_entry_reached_by_both_edges_of_a_switch_is_flagged() {
    // `switchInt(c) -> [1: bb2, otherwise: bb2]`: the false edge reaches the
    // read too, so the comparison proves nothing there.
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_HEADER].terminator = Some(terminator(TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(BELOW)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(LOOP_BODY))],
        otherwise: BasicBlock(LOOP_BODY),
    }));
    assert!(flags(body, read_at(0)));
}

#[test]
fn test_an_entry_resting_on_an_order_of_a_local_before_its_last_write_is_flagged() {
    use crate::mir::optimization::in_bounds::{
        bound_replaced_by_the_length, ONE_PAST, READ_ONE_PAST,
    };
    let proof = InBounds {
        block: BasicBlock(READ_ONE_PAST),
        statement_index: 1,
        collection: LIST,
        index: ONE_PAST,
    };
    assert!(flags(bound_replaced_by_the_length(), proof));
}
