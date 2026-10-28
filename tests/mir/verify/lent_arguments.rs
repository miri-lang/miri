// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a call which may release what its caller lends it
//! is never lent a value held only through a class field.
//!
//! The caller passes `h.node` beside `h`. Lent unretained to a callee that
//! replaces the node, it is reported; lent to a callee that only reads, or
//! read first into a local holding a reference of its own, it is not.

use crate::mir::analysis::heap_effects::{
    bag, block, body_of, call_into, call_returning_to, copy, definitions, field,
    field_writing_method, function, holder, int_list, keyed, local, node, read_only_function,
    registry, returning, runtime_call, set_of, shelf, span, statement, store, tally,
};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::TypeKind;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::DropHookReach;
use miri::mir::verify::{verify_lent_arguments, VerificationViolation};
use miri::mir::{BasicBlock, Body, Operand, Statement, StatementKind, TerminatorKind};
use miri::mir::{Constant, Local};
use miri::runtime_fns::rt;

/// `main`, calling `callee(h, arg)` with `h` its local 1.
fn caller(callee: &str, arg: Operand, setup: Vec<Statement>) -> Body {
    body_of(
        &[TypeKind::Void, holder(), node()],
        0,
        vec![
            block(
                setup,
                TerminatorKind::Call {
                    func: Operand::function(function(callee), span()),
                    args: vec![copy(local(1)), arg],
                    arg_modes: Vec::new(),
                    arg_handles: Vec::new(),
                    destination: local(0),
                    target: Some(BasicBlock(1)),
                },
            ),
            returning(Vec::new()),
        ],
    )
}

fn violations(callee: Body, main: Body) -> Vec<(Symbol, VerificationViolation)> {
    let program = vec![(function("callee"), callee), (function("main"), main)];
    let definitions = definitions();
    verify_lent_arguments(
        &program,
        Symbol::link_name,
        &DropHookReach::new(&definitions),
    )
}

#[test]
fn a_field_lent_unretained_to_a_callee_that_may_release_it_is_reported() {
    let found = violations(
        field_writing_method_taking_node(),
        caller("callee", copy(field(1, 0)), Vec::new()),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, function("main"));
    assert!(
        found[0].1.message.contains("`_1.0`"),
        "{}",
        found[0].1.message
    );
}

#[test]
fn a_field_lent_to_a_callee_that_only_reads_is_not_reported() {
    let found = violations(
        read_only_function(),
        caller("callee", copy(field(1, 0)), Vec::new()),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_field_read_into_a_retained_local_first_is_not_reported() {
    let setup = vec![
        statement(StatementKind::IncRef(field(1, 0))),
        store(local(2), field(1, 0)),
    ];
    let found = violations(
        field_writing_method_taking_node(),
        caller("callee", copy(local(2)), setup),
    );
    assert!(found.is_empty(), "{found:?}");
}

/// `fn callee(h Holder, n Node)`: replaces the node `h` holds.
fn field_writing_method_taking_node() -> Body {
    let mut body = field_writing_method();
    body.arg_count = 2;
    body
}

/// `fn callee(self [int], f fn())`: a method on a list that calls `f`, so it
/// may release anything. With `clears`, it first clears its receiver in
/// place, which its caller made unique for it.
fn closure_calling_list_method(clears: bool) -> Body {
    let kinds = [TypeKind::Void, int_list(), TypeKind::Identifier];
    let call_closure = |target| call_into(copy(local(2)), Vec::new(), local(0), target);
    let blocks = if clears {
        vec![
            block(
                Vec::new(),
                runtime_call(rt::LIST_CLEAR, vec![copy(local(1))]),
            ),
            block(Vec::new(), call_closure(2)),
            returning(Vec::new()),
        ]
    } else {
        vec![block(Vec::new(), call_closure(1)), returning(Vec::new())]
    };
    let mut body = body_of(&kinds, 2, blocks);
    body.receiver = Some(Local(1));
    body
}

/// `main`, calling `callee(s.items, f)` with `s` a `Shelf`, its local 1.
fn shelf_caller() -> Body {
    body_of(
        &[TypeKind::Void, shelf(), TypeKind::Identifier],
        0,
        vec![
            block(
                Vec::new(),
                call_returning_to("callee", vec![copy(field(1, 0)), copy(local(2))], 1),
            ),
            returning(Vec::new()),
        ],
    )
}

#[test]
fn a_field_receiver_lent_unretained_to_a_method_that_only_reads_it_is_reported() {
    let found = violations(closure_calling_list_method(false), shelf_caller());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].1.message.contains("`_1.0`"),
        "{}",
        found[0].1.message
    );
}

#[test]
fn a_field_receiver_lent_to_a_method_writing_it_in_place_is_not_reported() {
    let found = violations(closure_calling_list_method(true), shelf_caller());
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_map_entry_of_a_field_lent_unretained_to_a_loud_callee_is_reported() {
    let key = Operand::Constant(Box::new(Constant {
        span: span(),
        ty: miri::ast::types::Type::new(TypeKind::Int, span()),
        literal: Literal::Integer(IntegerLiteral::I64(1)),
    }));
    let get = call_into(
        Operand::function(Symbol::runtime(rt::MAP_GET_CHECKED), span()),
        vec![copy(field(1, 0)), key],
        local(2),
        1,
    );
    let main = body_of(
        &[TypeKind::Void, registry(), node()],
        0,
        vec![
            block(Vec::new(), get),
            block(
                Vec::new(),
                call_returning_to("callee", vec![copy(local(1)), copy(local(2))], 2),
            ),
            returning(Vec::new()),
        ],
    );
    let found = violations(field_writing_method_taking_node(), main);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].1.message.contains("`_2`"),
        "{}",
        found[0].1.message
    );
}

/// `fn contains(self Set<T>, x T)`, shared by every element type: the
/// runtime matches `x` against the elements through whatever rule the set's
/// element type registered.
fn shared_set_contains() -> Body {
    let element = TypeKind::Generic(
        "T".to_string(),
        None,
        miri::ast::types::TypeDeclarationKind::None,
    );
    let mut body = body_of(
        &[TypeKind::Boolean, set_of(element.clone()), element],
        2,
        vec![
            block(
                Vec::new(),
                runtime_call(rt::SET_CONTAINS, vec![copy(local(1)), copy(local(2))]),
            ),
            returning(Vec::new()),
        ],
    );
    body.receiver = Some(Local(1));
    body.type_params.insert("T".to_string());
    body
}

/// `main`, calling `callee(c.s, x)` with `c` a `holder_kind`, its local 1,
/// and `x` an `element`.
fn set_searching_caller(holder_kind: TypeKind, element: TypeKind) -> Body {
    body_of(
        &[TypeKind::Boolean, holder_kind, element],
        0,
        vec![
            block(
                Vec::new(),
                call_returning_to("callee", vec![copy(field(1, 0)), copy(local(2))], 1),
            ),
            returning(Vec::new()),
        ],
    )
}

#[test]
fn a_field_set_lent_unretained_to_a_search_running_program_code_is_reported() {
    let found = violations(shared_set_contains(), set_searching_caller(bag(), keyed()));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].1.message.contains("`_1.0`"),
        "{}",
        found[0].1.message
    );
}

#[test]
fn a_field_set_of_ints_lent_to_a_search_is_not_reported() {
    let found = violations(
        shared_set_contains(),
        set_searching_caller(tally(), TypeKind::Int),
    );
    assert!(found.is_empty(), "{found:?}");
}
