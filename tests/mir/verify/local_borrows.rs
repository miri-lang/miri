// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check on locals a body records as borrowing from a place it
//! owns. Each body reads a string out of a list the body owns into `_3` and
//! records `_3` as borrowing it; a body where nothing touches the list while
//! `_3` holds the string passes, and each body that changes the list, or asks
//! whether `_3` is shared, inside that range is refused.

use super::{
    block, body_of, collection_ty, goto, messages, place, ret, runtime_call, stmt, string_ty,
    void_ty,
};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::mir::block::BasicBlockData;
use miri::mir::place::PlaceElem;
use miri::mir::verify::{verify_body, verify_local_borrows};
use miri::mir::{Body, Local, Operand, Place, Rvalue, Statement, StatementKind};
use miri::runtime_fns::rt;
use std::collections::HashMap;

fn int_ty() -> Type {
    Type::new(TypeKind::Int, miri::error::syntax::Span::new(0, 0))
}

fn element() -> Place {
    Place {
        local: Local(1),
        projection: vec![PlaceElem::Index(Local(2))],
    }
}

/// `_1`, the list, gets a value of its own, and `_2` the index.
fn list_and_index() -> Vec<Statement> {
    let zero = super::constant(int_ty(), Literal::Integer(IntegerLiteral::I64(0)));
    vec![
        super::storage_live(1),
        super::assign_fresh(1),
        stmt(StatementKind::Assign(place(2), Rvalue::Use(zero))),
        super::storage_live(3),
        stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::Copy(element())),
        )),
    ]
}

/// The end of `_3`'s storage, then the list's release and end.
fn end_of_borrow_and_list() -> Vec<Statement> {
    vec![
        super::storage_dead(3),
        super::decref(1),
        super::storage_dead(1),
    ]
}

/// Locals: 0 the return slot, 1 a list of strings the body owns, 2 the index,
/// 3 the string read out of the list, 4 a call's result. `_3` is recorded as
/// borrowing `_1[_2]`.
fn borrowing_body(blocks: Vec<BasicBlockData>) -> Body {
    let mut body = body_of(
        &[
            void_ty(),
            collection_ty("List", &[TypeKind::String]),
            int_ty(),
            string_ty(),
            collection_ty("List", &[TypeKind::String]),
        ],
        0,
        blocks,
    );
    body.borrowed_locals = HashMap::from([(Local(3), vec![Local(1)])]);
    body
}

fn single_block(middle: Vec<Statement>) -> Body {
    let mut statements = list_and_index();
    statements.extend(middle);
    statements.extend(end_of_borrow_and_list());
    borrowing_body(vec![block(statements, ret())])
}

fn assert_refused(body: &Body, what: &str) {
    let violations = verify_local_borrows(body);
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(3)),
        "{what} must refuse the borrow, got: {}",
        messages(&violations)
    );
}

#[test]
fn a_borrow_whose_list_stays_holds_no_reference_and_is_no_leak() {
    let body = single_block(Vec::new());
    let violations = verify_body(&body);
    assert!(violations.is_empty(), "got: {}", messages(&violations));
}

/// Without the record the same body leaks the string it read, so the check
/// that passes above is the one that decides.
#[test]
fn the_same_read_unrecorded_is_a_leak() {
    let mut body = single_block(Vec::new());
    body.borrowed_locals.clear();
    let violations = verify_body(&body);
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(3)),
        "got: {}",
        messages(&violations)
    );
}

#[test]
fn a_borrow_whose_list_is_released_while_it_holds_is_refused() {
    let body = single_block(vec![super::decref(1)]);
    assert_refused(&body, "releasing the list inside the range");
    assert!(
        !verify_body(&body).is_empty(),
        "verify_body reports the refused borrow"
    );
}

#[test]
fn a_borrow_whose_list_is_rebound_while_it_holds_is_refused() {
    let body = single_block(vec![stmt(StatementKind::Reassign(
        place(1),
        super::fresh_string(),
    ))]);
    assert_refused(&body, "rebinding the list inside the range");
}

/// The list goes to its copy-on-write entry point in a block between the
/// read and the end of the borrow's storage.
#[test]
fn a_borrow_whose_list_is_moved_into_a_call_while_it_holds_is_refused() {
    let body = borrowing_body(vec![
        block(
            list_and_index(),
            runtime_call(rt::LIST_COW, vec![Operand::Move(place(1))], 4, 1),
        ),
        block(
            {
                let mut statements = vec![stmt(StatementKind::Assign(
                    place(1),
                    Rvalue::Use(Operand::Move(place(4))),
                ))];
                statements.extend(end_of_borrow_and_list());
                statements
            },
            ret(),
        ),
    ]);
    assert_refused(&body, "moving the list into a call inside the range");
}

/// A borrowed list element asked whether it is shared would answer no and
/// be written in place, inside the list it was read from.
#[test]
fn a_borrow_asked_whether_it_is_shared_is_refused() {
    let body = borrowing_body(vec![
        block(
            list_and_index(),
            runtime_call(rt::RC_IS_SHARED, vec![Operand::Copy(place(3))], 0, 1),
        ),
        block(end_of_borrow_and_list(), ret()),
    ]);
    assert_refused(&body, "a uniqueness check on the borrow");
}

/// A path from the read that reaches the end of the body before the borrow's
/// storage ends gives the borrow no end to rely on.
#[test]
fn a_borrow_reaching_the_return_before_its_storage_ends_is_refused() {
    let body = borrowing_body(vec![
        block(list_and_index(), super::branch(1, 2)),
        block(end_of_borrow_and_list(), ret()),
        block(Vec::new(), goto(3)),
        block(Vec::new(), ret()),
    ]);
    assert_refused(&body, "a path to the return that skips the borrow's end");
}

/// A list a parameter holds may be emptied through another name, so an
/// element of it is never borrowed.
#[test]
fn a_borrow_of_a_parameter_element_is_refused() {
    let zero = super::constant(int_ty(), Literal::Integer(IntegerLiteral::I64(0)));
    let mut body = borrowing_body(vec![block(
        vec![
            stmt(StatementKind::Assign(place(2), Rvalue::Use(zero))),
            super::storage_live(3),
            stmt(StatementKind::Assign(
                place(3),
                Rvalue::Use(Operand::Copy(element())),
            )),
            super::storage_dead(3),
        ],
        ret(),
    )]);
    body.arg_count = 1;
    assert_refused(&body, "an element of a parameter");
}

/// The record must name every owner the body shows: RC elision keeps the
/// retains of exactly the recorded owners.
#[test]
fn a_borrow_whose_owner_is_not_recorded_is_refused() {
    let mut body = single_block(Vec::new());
    body.borrowed_locals = HashMap::from([(Local(3), vec![Local(4)])]);
    assert_refused(&body, "a record missing the owner");
}

/// A callee writes a parameter in place when nothing else holds it, so
/// lending the list to a Miri function while the borrow holds may free the
/// borrowed string.
#[test]
fn a_borrow_whose_list_is_lent_to_a_function_while_it_holds_is_refused() {
    let body = borrowing_body(vec![
        block(list_and_index(), super::call_returning_void(1, 0, 1)),
        block(end_of_borrow_and_list(), ret()),
    ]);
    assert_refused(&body, "lending the list to a function inside the range");
}

/// A runtime entry point that only reads the list leaves the borrow valid.
#[test]
fn a_borrow_whose_list_is_lent_to_a_reader_holds() {
    let body = borrowing_body(vec![
        block(
            list_and_index(),
            runtime_call(rt::LIST_LEN, vec![Operand::Copy(place(1))], 0, 1),
        ),
        block(end_of_borrow_and_list(), ret()),
    ]);
    let violations = verify_local_borrows(&body);
    assert!(violations.is_empty(), "got: {}", messages(&violations));
}
