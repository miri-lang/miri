// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check on locals a body records as borrowing from a place it
//! owns or is lent. Each body reads a string out of a list — one the body
//! owns, or a parameter — into `_3` and records `_3` as borrowing it; a body
//! where nothing touches the list while `_3` holds the string passes, and
//! each body that changes the list, or asks whether `_3` is shared, inside
//! that range is refused. A parameter's list may be reached by other code, so
//! a borrow of its element is refused across anything that may release a
//! value another name holds.

use super::{
    block, body_of, collection_ty, goto, messages, place, ret, runtime_call, stmt, string_ty,
    void_ty,
};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{verify_body, verify_local_borrows};
use miri::mir::{
    ArgMode, Body, Local, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use miri::runtime_fns::rt;
use miri::type_checker::ModuleId;
use std::collections::{HashMap, HashSet};

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
    let violations = verify_local_borrows(body, None);
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

/// The statements reading `_1[_2]` into `_3`, for a body whose list `_1`
/// is a parameter.
fn parameter_element() -> Vec<Statement> {
    let zero = super::constant(int_ty(), Literal::Integer(IntegerLiteral::I64(0)));
    vec![
        stmt(StatementKind::Assign(place(2), Rvalue::Use(zero))),
        super::storage_live(3),
        stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::Copy(element())),
        )),
    ]
}

/// A body whose list `_1` is a parameter it borrows, with `_3` recorded as
/// borrowing `_1[_2]`: block 0 reads the element and ends in `terminator`,
/// block 1 runs `middle` and ends the borrow.
fn parameter_borrowing_body(terminator: Terminator, middle: Vec<Statement>) -> Body {
    let mut end = middle;
    end.push(super::storage_dead(3));
    let mut body = borrowing_body(vec![
        block(parameter_element(), terminator),
        block(end, ret()),
    ]);
    body.arg_count = 1;
    body
}

/// A call to the function of the program `callee` with no arguments,
/// returning to block 1.
fn unrelated_call() -> Terminator {
    unrelated_call_returning_to(1)
}

/// A call to the function of the program `callee` with no arguments,
/// returning to block `target`.
fn unrelated_call_returning_to(target: usize) -> Terminator {
    let mut call = call(function_named("callee"), Vec::new());
    if let TerminatorKind::Call { target: next, .. } = &mut call.kind {
        *next = Some(BasicBlock(target));
    }
    call
}

/// The caller's reference keeps a parameter's list alive, and nothing runs
/// while `_3` holds its element, so the borrow holds.
#[test]
fn a_borrow_of_a_parameter_element_holds_while_nothing_runs() {
    let body = parameter_borrowing_body(goto(1), Vec::new());
    assert_held(&body, "an element of a parameter, nothing running");
}

/// Other code may reach the list a parameter holds, so a call not known to
/// release nothing another name holds ends the borrow: with no heap facts
/// every call to the program is such a call.
#[test]
fn a_borrow_of_a_parameter_element_across_a_call_with_no_heap_facts_is_refused() {
    let body = parameter_borrowing_body(unrelated_call(), Vec::new());
    assert_refused(&body, "a call with no heap facts");
}

/// The same call found quiet keeps the borrow; facts that do not name it
/// leave it loud.
#[test]
fn a_borrow_of_a_parameter_element_holds_only_across_a_call_found_quiet() {
    let body = parameter_borrowing_body(unrelated_call(), Vec::new());
    let quiet = HashSet::from([BasicBlock(0)]);
    let violations = verify_local_borrows(&body, Some(&quiet));
    assert!(violations.is_empty(), "got: {}", messages(&violations));
    let elsewhere = HashSet::from([BasicBlock(1)]);
    let violations = verify_local_borrows(&body, Some(&elsewhere));
    assert!(
        violations
            .iter()
            .any(|violation| violation.local == Local(3)),
        "a call the facts do not name must refuse the borrow, got: {}",
        messages(&violations)
    );
}

/// A release that may run a drop hook can run code reaching the list, so it
/// ends a borrow of a parameter's element.
#[test]
fn a_borrow_of_a_parameter_element_across_a_hooked_release_is_refused() {
    let mut body = parameter_borrowing_body(goto(1), vec![super::decref(4)]);
    body.local_decls[4].ty = Type::new(
        TypeKind::Custom("Hooked".to_string(), None),
        miri::error::syntax::Span::new(0, 0),
    );
    assert_refused(&body, "a release whose type is not known to run no hook");
}

/// The end of a borrowed parameter's binding leaves the caller's reference
/// in place; the end of a local owner's storage releases what it holds.
#[test]
fn the_storage_end_of_a_parameter_owner_keeps_the_borrow_and_of_a_local_owner_refuses_it() {
    let body = parameter_borrowing_body(goto(1), vec![super::storage_dead(1)]);
    assert_held(&body, "the end of a parameter's binding");
    let local = single_block(vec![super::storage_dead(1)]);
    assert_refused(&local, "the end of a local owner's storage");
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
    let violations = verify_local_borrows(&body, None);
    assert!(violations.is_empty(), "got: {}", messages(&violations));
}

/// Locals as in [`borrowing_body`], but `_1` is a list of lists of integers
/// and `_3` the row read out of it — a value a callee writing its parameter
/// copies on entry. `_4` is a second list of integers.
fn row_borrowing_body(call: Terminator) -> Body {
    let row = collection_ty("List", &[TypeKind::Int]);
    let mut body = body_of(
        &[
            void_ty(),
            collection_ty("List", &[row.kind.clone()]),
            int_ty(),
            row.clone(),
            row,
        ],
        0,
        vec![
            block(list_and_index(), call),
            block(end_of_borrow_and_list(), ret()),
        ],
    );
    body.borrowed_locals = HashMap::from([(Local(3), vec![Local(1)])]);
    body
}

/// A call to `callee` handing it `args`, each with its mode, returning to
/// block 1.
fn call(callee: Operand, args: Vec<(Operand, ArgMode)>) -> Terminator {
    let (args, arg_modes) = args.into_iter().unzip();
    Terminator {
        kind: TerminatorKind::Call {
            func: callee,
            args,
            arg_modes,
            arg_handles: Vec::new(),
            destination: place(0),
            target: Some(BasicBlock(1)),
        },
        span: miri::error::syntax::Span::new(0, 0),
    }
}

fn function_named(name: &str) -> Operand {
    Operand::function(
        Symbol::declared_function(&ModuleId::Program, name),
        miri::error::syntax::Span::new(0, 0),
    )
}

fn method_named(name: &str) -> Operand {
    let int = int_ty();
    Operand::function(
        Symbol::method("List", [&int], name, []),
        miri::error::syntax::Span::new(0, 0),
    )
}

fn lent(local: usize) -> (Operand, ArgMode) {
    (Operand::Copy(place(local)), ArgMode::Borrowed)
}

fn assert_held(body: &Body, what: &str) {
    let violations = verify_local_borrows(body, None);
    assert!(violations.is_empty(), "{what}: {}", messages(&violations));
}

/// A function writing a list parameter writes a copy it binds on entry, which
/// takes a reference of its own, so a borrowed row lent to it stays valid.
#[test]
fn a_borrowed_row_lent_to_a_function_holds() {
    let body = row_borrowing_body(call(function_named("callee"), vec![lent(3)]));
    assert_held(&body, "lending the row to a function");
}

/// A method's receiver is not copied on entry: a write through it checks
/// that it arrived unique, which a borrowed row's count — its owner's —
/// would answer yes to.
#[test]
fn a_borrowed_row_handed_to_a_method_as_its_receiver_is_refused() {
    let body = row_borrowing_body(call(method_named("sort"), vec![lent(3)]));
    assert_refused(&body, "the row as a method's receiver");
}

/// Past the receiver, a method's parameters are copied on entry like a
/// function's.
#[test]
fn a_borrowed_row_lent_to_a_method_past_its_receiver_holds() {
    let body = row_borrowing_body(call(method_named("append"), vec![lent(4), lent(3)]));
    assert_held(&body, "lending the row to a method past its receiver");
}

/// An owned argument and a closure's are refused: the callee takes over the
/// first, and what a closure binds on entry is not known at the call.
#[test]
fn a_borrowed_row_passed_owned_or_to_a_closure_is_refused() {
    let owned = row_borrowing_body(call(
        function_named("callee"),
        vec![(Operand::Copy(place(3)), ArgMode::Owned)],
    ));
    assert_refused(&owned, "the row passed owned");
    let closure = row_borrowing_body(call(Operand::Copy(place(4)), vec![lent(3)]));
    assert_refused(&closure, "the row lent to a closure");
}

/// A string is not copied on entry by a callee writing it, so a borrowed
/// string lent to a function is refused.
#[test]
fn a_borrowed_string_lent_to_a_function_is_refused() {
    let body = borrowing_body(vec![
        block(
            list_and_index(),
            call(function_named("callee"), vec![lent(3)]),
        ),
        block(end_of_borrow_and_list(), ret()),
    ]);
    assert_refused(&body, "lending the string to a function");
}

/// Locals: 0 the return slot, 1 a map of lists of strings the body owns, 2
/// the key and index, 3 the string read out of the entry, 4 the entry the
/// map hands back as a borrow. Block 0 looks the entry up, block 1 reads
/// `_4[_2]` into `_3`, runs `middle` and ends in `terminator`, block 2 ends
/// the borrow and the map. `_3` is recorded as borrowing from `recorded`.
fn entry_borrowing_body(
    middle: Vec<Statement>,
    terminator: Terminator,
    recorded: Vec<Local>,
) -> Body {
    let list = collection_ty("List", &[TypeKind::String]);
    let zero = super::constant(int_ty(), Literal::Integer(IntegerLiteral::I64(0)));
    let lookup = vec![
        super::storage_live(1),
        super::assign_fresh(1),
        stmt(StatementKind::Assign(place(2), Rvalue::Use(zero))),
    ];
    let mut read = vec![
        super::storage_live(3),
        stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::Copy(Place {
                local: Local(4),
                projection: vec![PlaceElem::Index(Local(2))],
            })),
        )),
    ];
    read.extend(middle);
    let mut body = body_of(
        &[
            void_ty(),
            collection_ty("Map", &[TypeKind::Int, list.kind.clone()]),
            int_ty(),
            string_ty(),
            list,
        ],
        0,
        vec![
            block(
                lookup,
                runtime_call(
                    rt::MAP_GET_CHECKED,
                    vec![Operand::Copy(place(1)), Operand::Copy(place(2))],
                    4,
                    1,
                ),
            ),
            block(read, terminator),
            block(end_of_borrow_and_list(), ret()),
        ],
    );
    body.borrowed_locals = HashMap::from([(Local(3), recorded)]);
    body
}

/// An entry a map hands back holds no reference of its own: the map does.
/// A borrow read out of it relies on the map, which must be recorded.
#[test]
fn a_borrow_out_of_a_map_entry_must_record_the_map() {
    let body = entry_borrowing_body(Vec::new(), goto(2), vec![Local(4)]);
    assert_refused(&body, "a record missing the map the entry is held by");
    let body = entry_borrowing_body(Vec::new(), goto(2), vec![Local(4), Local(1)]);
    assert_held(&body, "a borrow out of an entry while nothing runs");
}

/// What else holds the map may replace or empty it, so a call not known
/// quiet ends a borrow read out of its entry.
#[test]
fn a_borrow_out_of_a_map_entry_across_a_call_not_found_quiet_is_refused() {
    let body = entry_borrowing_body(
        Vec::new(),
        unrelated_call_returning_to(2),
        vec![Local(4), Local(1)],
    );
    assert_refused(&body, "a call with no heap facts");
    let quiet = HashSet::from([BasicBlock(1)]);
    let violations = verify_local_borrows(&body, Some(&quiet));
    assert!(violations.is_empty(), "got: {}", messages(&violations));
}

/// Rebinding the map releases the entry the borrow reads.
#[test]
fn a_borrow_out_of_a_map_entry_whose_map_is_rebound_is_refused() {
    let rebind = stmt(StatementKind::Reassign(place(1), super::fresh_string()));
    let body = entry_borrowing_body(vec![rebind], goto(2), vec![Local(4), Local(1)]);
    assert_refused(&body, "rebinding the map inside the range");
}
