// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a runtime entry point writes a collection in place
//! only where every path to the write made it the program's own.
//!
//! Each shape the lowering emits appears twice: once as lowering writes it,
//! with nothing reported, and once with the step that made the write safe
//! removed or undone, with one finding naming the written place and the entry
//! point.

use miri::ast::common::MemberVisibility;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{verify_unshared_writes, VerificationViolation};
use miri::mir::{
    AggregateKind, BinOp, Body, Constant, Discriminant, ExecutionModel, Local, LocalDecl, Operand,
    Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::runtime_fns::rt;
use miri::type_checker::context::{ClassDefinition, FieldInfo, StructDefinition, TypeDefinition};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn type_expr(kind: TypeKind) -> Expression {
    Expression::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
}

fn int() -> TypeKind {
    TypeKind::Int
}

fn list_of(element: TypeKind) -> TypeKind {
    TypeKind::List(Box::new(type_expr(element)))
}

fn list() -> TypeKind {
    list_of(int())
}

fn map_of_lists() -> TypeKind {
    TypeKind::Map(Box::new(type_expr(int())), Box::new(type_expr(list())))
}

fn array() -> TypeKind {
    let size = Expression::new(
        0,
        ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I64(3))),
        span(),
    );
    TypeKind::Array(Box::new(type_expr(int())), Box::new(size))
}

/// `struct Bag` holding one list of ints.
fn bag() -> TypeKind {
    TypeKind::Custom("Bag".to_string(), None)
}

/// `class Crate` holding one list of ints.
fn crate_class() -> TypeKind {
    TypeKind::Custom("Crate".to_string(), None)
}

/// A type the program does not define, so its release may run a drop hook.
fn hooked() -> TypeKind {
    TypeKind::Custom("Resource".to_string(), None)
}

fn definitions() -> HashMap<String, TypeDefinition> {
    let bag = StructDefinition {
        fields: vec![("items".to_string(), ty(list()), MemberVisibility::Public)],
        generics: None,
        module: "test".to_string(),
    };
    let items = FieldInfo {
        ty: ty(list()),
        mutable: true,
        visibility: MemberVisibility::Public,
        initializer: None,
    };
    let crate_class = ClassDefinition {
        name: "Crate".to_string(),
        generics: None,
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![("items".to_string(), items)],
        methods: BTreeMap::new(),
        module: "test".to_string(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    };
    HashMap::from([
        ("Bag".to_string(), TypeDefinition::Struct(bag)),
        ("Crate".to_string(), TypeDefinition::Class(crate_class)),
    ])
}

/// A body whose local `i` has kind `kinds[i]`; local 0 is the return slot.
fn body_of(kinds: &[TypeKind], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks = blocks;
    body.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    body
}

fn local(index: usize) -> Place {
    Place::new(Local(index))
}

fn field(index: usize, at: usize) -> Place {
    Place {
        local: Local(index),
        projection: vec![PlaceElem::Field(at)],
    }
}

fn element(index: usize, by: usize) -> Place {
    Place {
        local: Local(index),
        projection: vec![PlaceElem::Index(Local(by))],
    }
}

fn constant(value: i64) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(int()),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

fn statement(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

fn assign(dest: Place, rvalue: Rvalue) -> Statement {
    statement(StatementKind::Assign(dest, rvalue))
}

fn move_into(dest: usize, source: usize) -> Statement {
    assign(local(dest), Rvalue::Use(Operand::Move(local(source))))
}

fn copy_into(dest: usize, source: usize) -> Statement {
    assign(local(dest), Rvalue::Use(Operand::Copy(local(source))))
}

fn set(dest: usize, value: i64) -> Statement {
    assign(local(dest), Rvalue::Use(constant(value)))
}

fn retain(place: Place) -> Statement {
    statement(StatementKind::IncRef(place))
}

fn release(place: Place) -> Statement {
    statement(StatementKind::DecRef(place))
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

fn returning(statements: Vec<Statement>) -> BasicBlockData {
    block(statements, TerminatorKind::Return)
}

fn goto(target: usize) -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(target),
    }
}

/// `switchInt(answer) -> [0: no, otherwise: yes]`.
fn branch(answer: usize, no: usize, yes: usize) -> TerminatorKind {
    TerminatorKind::SwitchInt {
        discr: Operand::Copy(local(answer)),
        targets: vec![(Discriminant::from(0), BasicBlock(no))],
        otherwise: BasicBlock(yes),
    }
}

fn call(func: Operand, args: Vec<Operand>, dest: Place, target: usize) -> TerminatorKind {
    TerminatorKind::Call {
        func,
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: dest,
        target: Some(BasicBlock(target)),
    }
}

fn runtime(name: &str, args: Vec<Operand>, dest: usize, target: usize) -> TerminatorKind {
    call(Operand::runtime(name, span()), args, local(dest), target)
}

/// A call to a function of the program, handed `args`.
fn program(args: Vec<Operand>, dest: usize, target: usize) -> TerminatorKind {
    call(
        Operand::function(Symbol::entry(), span()),
        args,
        local(dest),
        target,
    )
}

fn copy(place: Place) -> Operand {
    Operand::Copy(place)
}

fn moved(place: Place) -> Operand {
    Operand::Move(place)
}

/// `push(place, 7)`.
fn push(place: Place, dest: usize, target: usize) -> TerminatorKind {
    runtime(rt::LIST_PUSH, vec![copy(place), constant(7)], dest, target)
}

fn findings(body: &Body) -> Vec<VerificationViolation> {
    verify_unshared_writes(body)
}

/// Asserts `body` has exactly one finding, on `place`'s write through `name`.
fn assert_one_finding(body: &Body, place: &str, name: &str) {
    let found = findings(body);
    assert_eq!(found.len(), 1, "{found:?}");
    let message = &found[0].message;
    assert!(
        message.contains(&format!("`{place}`")) && message.contains(name),
        "{message}"
    );
}

fn assert_clean(body: &Body) {
    let found = findings(body);
    assert!(found.is_empty(), "{found:?}");
}

/// Locals: 0 void, 1 `[int]`, 2 `[int]` the copy-on-write result, 3 void,
/// 4 `[int]` spare, 5 void.
fn list_locals() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        list(),
        list(),
        TypeKind::Void,
        list(),
        TypeKind::Void,
    ]
}

/// `_1 = new; _2 = cow(move _1); _1 = move _2; <between>; push(_1, 7)`.
fn copied_then_pushed(between: Vec<Statement>) -> Body {
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            Vec::new(),
            runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2),
        ),
        block(
            [vec![move_into(1, 2)], between].concat(),
            push(local(1), 3, 3),
        ),
        returning(Vec::new()),
    ];
    body_of(&list_locals(), 0, blocks)
}

#[test]
fn a_list_copied_on_write_then_pushed_is_clean() {
    assert_clean(&copied_then_pushed(Vec::new()));
}

#[test]
fn a_list_pushed_with_no_copy_on_write_is_flagged() {
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(Vec::new(), push(local(1), 3, 2)),
        returning(Vec::new()),
    ];
    assert_one_finding(&body_of(&list_locals(), 0, blocks), "_1", rt::LIST_PUSH);
}

#[test]
fn a_list_retained_into_another_name_after_its_copy_is_flagged() {
    let body = copied_then_pushed(vec![retain(local(1)), copy_into(4, 1)]);
    assert_one_finding(&body, "_1", rt::LIST_PUSH);
}

/// `copied_then_pushed` with a call to a function of the program handed
/// `handed` just before the push.
fn copied_then_called(handed: usize) -> Body {
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            Vec::new(),
            runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2),
        ),
        block(
            vec![move_into(1, 2)],
            program(vec![copy(local(handed))], 5, 3),
        ),
        block(Vec::new(), push(local(1), 3, 4)),
        returning(Vec::new()),
    ];
    body_of(&list_locals(), 0, blocks)
}

#[test]
fn a_list_handed_to_a_function_of_the_program_before_its_write_is_flagged() {
    assert_one_finding(&copied_then_called(1), "_1", rt::LIST_PUSH);
}

#[test]
fn a_private_list_not_handed_to_a_function_of_the_program_stays_checked() {
    assert_clean(&copied_then_called(4));
}

/// Locals: 0 void, 1 `Bag` (the receiver), 2 bool, 3 `[int]`, 4 void, 5 void,
/// 6 a value whose release runs a drop hook.
fn bag_locals() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        bag(),
        TypeKind::Boolean,
        list(),
        TypeKind::Void,
        TypeKind::Void,
        hooked(),
    ]
}

/// `self.items.push(7)` on a struct receiver: the check on `_1.0`, the copy
/// path, then `before_write` in the block the push ends.
fn checked_field_then_pushed(no: usize, yes: usize, before_write: Vec<Statement>) -> Body {
    let blocks = vec![
        block(
            Vec::new(),
            runtime(rt::RC_IS_SHARED, vec![copy(field(1, 0))], 2, 1),
        ),
        block(Vec::new(), branch(2, no, yes)),
        block(
            Vec::new(),
            runtime(rt::LIST_CLONE, vec![copy(field(1, 0))], 3, 4),
        ),
        block(before_write, push(field(1, 0), 4, 5)),
        block(
            vec![
                release(field(1, 0)),
                statement(StatementKind::Reassign(
                    field(1, 0),
                    Rvalue::Use(moved(local(3))),
                )),
            ],
            goto(3),
        ),
        returning(Vec::new()),
    ];
    let mut body = body_of(&bag_locals(), 1, blocks);
    body.receiver = Some(Local(1));
    body
}

#[test]
fn a_field_checked_and_copied_on_the_shared_path_is_clean() {
    assert_clean(&checked_field_then_pushed(3, 2, Vec::new()));
}

#[test]
fn a_field_written_from_the_shared_edge_with_no_copy_is_flagged() {
    let mut body = checked_field_then_pushed(2, 3, Vec::new());
    body.basic_blocks[4].terminator = Some(Terminator::new(goto(5), span()));
    assert_one_finding(&body, "_1.0", rt::LIST_PUSH);
}

#[test]
fn a_field_of_a_receiver_after_a_release_that_runs_a_drop_hook_is_flagged() {
    let body = checked_field_then_pushed(3, 2, vec![release(local(6))]);
    assert_one_finding(&body, "_1.0", rt::LIST_PUSH);
}

#[test]
fn a_field_of_a_receiver_after_a_call_to_a_function_of_the_program_is_flagged() {
    let mut body = checked_field_then_pushed(3, 2, Vec::new());
    let write = body.basic_blocks[3].terminator.take();
    body.basic_blocks[3].terminator = Some(Terminator::new(program(Vec::new(), 5, 6), span()));
    body.basic_blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: write,
        is_cleanup: false,
    });
    assert_one_finding(&body, "_1.0", rt::LIST_PUSH);
}

#[test]
fn a_field_of_a_receiver_after_a_runtime_call_that_runs_a_closure_is_flagged() {
    let mut body = checked_field_then_pushed(3, 2, Vec::new());
    let write = body.basic_blocks[3].terminator.take();
    let sort = runtime(rt::LIST_SORT_BY, vec![copy(local(3)), copy(local(6))], 5, 6);
    body.basic_blocks[3].terminator = Some(Terminator::new(sort, span()));
    body.basic_blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: write,
        is_cleanup: false,
    });
    let found = findings(&body);
    assert!(
        found
            .iter()
            .any(|violation| violation.message.contains("`_1.0`")),
        "{found:?}"
    );
}

/// Locals: 0 void, 1 `[[int]]`, 2 `[[int]]`, 3 void, 4 int, 5 int, 6 bool,
/// 7 void.
fn nested_locals() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        list_of(list()),
        list_of(list()),
        TypeKind::Void,
        int(),
        int(),
        TypeKind::Boolean,
        TypeKind::Void,
    ]
}

/// `nested[i].push(7)`, the element checked unshared on its "no" edge, then
/// `between` before the push.
fn checked_element_then_pushed(between: Vec<Statement>) -> Body {
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            vec![set(4, 0)],
            runtime(rt::RC_IS_SHARED, vec![copy(element(1, 4))], 6, 2),
        ),
        block(Vec::new(), branch(6, 3, 4)),
        block(between, push(element(1, 4), 7, 5)),
        block(Vec::new(), TerminatorKind::Unreachable),
        returning(Vec::new()),
    ];
    body_of(&nested_locals(), 0, blocks)
}

#[test]
fn an_element_checked_unshared_then_pushed_is_clean() {
    assert_clean(&checked_element_then_pushed(Vec::new()));
}

#[test]
fn an_element_whose_index_is_reassigned_after_its_check_is_flagged() {
    let bump = assign(
        local(4),
        Rvalue::BinaryOp(BinOp::Add, Box::new(copy(local(4))), Box::new(constant(1))),
    );
    let body = checked_element_then_pushed(vec![bump]);
    assert_one_finding(&body, "_1[_4]", rt::LIST_PUSH);
}

#[test]
fn an_element_whose_sibling_is_retained_after_its_check_is_flagged() {
    let body = checked_element_then_pushed(vec![set(5, 1), retain(element(1, 5))]);
    assert_one_finding(&body, "_1[_4]", rt::LIST_PUSH);
}

/// A loop pushing onto `_1`, copied on write once before it, whose body
/// ends with `tail` before jumping back.
fn hoisted_copy_loop(tail: TerminatorKind) -> Body {
    let mut locals = list_locals();
    locals.push(TypeKind::Boolean);
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            Vec::new(),
            runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2),
        ),
        block(vec![move_into(1, 2)], goto(3)),
        block(vec![set(6, 1)], branch(6, 6, 4)),
        block(Vec::new(), push(local(1), 3, 5)),
        block(Vec::new(), tail),
        returning(Vec::new()),
    ];
    body_of(&locals, 0, blocks)
}

#[test]
fn a_copy_on_write_hoisted_out_of_a_loop_covers_the_push_inside_it() {
    assert_clean(&hoisted_copy_loop(goto(3)));
}

#[test]
fn a_loop_handing_its_list_to_a_function_of_the_program_is_flagged() {
    let body = hoisted_copy_loop(program(vec![copy(local(1))], 5, 3));
    assert_one_finding(&body, "_1", rt::LIST_PUSH);
}

#[test]
fn a_runtime_write_keeps_its_own_place_unshared_for_the_next_write() {
    // A runtime write keeps nothing it is handed
    // (`tests/runtime_fns.rs::a_runtime_write_keeps_nothing_it_is_handed`), so
    // the push leaves `_1` as unshared as it found it, and the sort after it
    // needs no check of its own.
    let mut locals = list_locals();
    locals.push(TypeKind::RawPtr);
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            Vec::new(),
            runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2),
        ),
        block(vec![move_into(1, 2)], push(local(1), 3, 3)),
        block(
            Vec::new(),
            runtime(rt::LIST_SORT_BY, vec![copy(local(1)), copy(local(6))], 5, 4),
        ),
        returning(Vec::new()),
    ];
    assert_clean(&body_of(&locals, 0, blocks));
}

/// `list_clear(self)` on a list receiver, after `before`.
fn receiver_cleared(before: Vec<TerminatorKind>) -> Body {
    let count = before.len();
    let mut blocks: Vec<BasicBlockData> = before
        .into_iter()
        .enumerate()
        .map(|(at, kind)| block(Vec::new(), retarget(kind, at + 1)))
        .collect();
    blocks.push(block(
        Vec::new(),
        runtime(rt::LIST_CLEAR, vec![copy(local(1))], 3, count + 1),
    ));
    blocks.push(returning(Vec::new()));
    let mut body = body_of(&list_locals(), 1, blocks);
    body.receiver = Some(Local(1));
    body
}

fn retarget(mut kind: TerminatorKind, to: usize) -> TerminatorKind {
    if let TerminatorKind::Call { target, .. } = &mut kind {
        *target = Some(BasicBlock(to));
    }
    kind
}

fn receiver_unique() -> TerminatorKind {
    runtime(rt::RECEIVER_UNIQUE, vec![copy(local(1))], 5, 0)
}

#[test]
fn a_receiver_found_unique_then_cleared_is_clean() {
    assert_clean(&receiver_cleared(vec![receiver_unique()]));
}

#[test]
fn a_receiver_cleared_with_no_uniqueness_check_is_flagged() {
    assert_one_finding(&receiver_cleared(Vec::new()), "_1", rt::LIST_CLEAR);
}

#[test]
fn a_receiver_handed_to_a_function_of_the_program_after_its_check_is_flagged() {
    let body = receiver_cleared(vec![receiver_unique(), program(vec![copy(local(1))], 5, 0)]);
    assert_one_finding(&body, "_1", rt::LIST_CLEAR);
}

/// Locals of the map-entry shape: 0 void, 1 `Map<int, [int]>`, 2 the map's
/// copy, 3 the key, 4 the looked-up entry, 5 bool, 6 the entry's clone,
/// 7 the key re-spelled, 8 void, 9 the entry looked up again, 10 void.
fn map_locals() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        map_of_lists(),
        map_of_lists(),
        int(),
        list(),
        TypeKind::Boolean,
        list(),
        int(),
        TypeKind::Void,
        list(),
        TypeKind::Void,
    ]
}

/// `m[1].push(7)`: the entry looked up and checked, cloned and stored back
/// under a re-spelled key on its shared path, then looked up again and
/// written. `before_lookup` runs just before the second lookup.
fn map_entry_pushed(before_lookup: Vec<Statement>) -> Body {
    let get = |dest: usize, target: usize| {
        runtime(
            rt::MAP_GET_CHECKED,
            vec![copy(local(1)), copy(local(3))],
            dest,
            target,
        )
    };
    let blocks = vec![
        block(Vec::new(), runtime(rt::MAP_NEW, Vec::new(), 1, 1)),
        block(
            Vec::new(),
            runtime(rt::MAP_COW, vec![moved(local(1))], 2, 2),
        ),
        block(vec![move_into(1, 2), set(3, 1)], get(4, 3)),
        block(
            Vec::new(),
            runtime(rt::RC_IS_SHARED, vec![copy(local(4))], 5, 4),
        ),
        block(Vec::new(), branch(5, 7, 5)),
        block(
            Vec::new(),
            runtime(rt::LIST_CLONE, vec![copy(local(4))], 6, 6),
        ),
        block(
            vec![copy_into(7, 3)],
            runtime(
                rt::MAP_SET,
                vec![copy(local(1)), copy(local(7)), copy(local(6))],
                8,
                7,
            ),
        ),
        block(before_lookup, get(9, 8)),
        block(Vec::new(), push(local(9), 10, 9)),
        returning(Vec::new()),
    ];
    body_of(&map_locals(), 0, blocks)
}

#[test]
fn a_map_entry_checked_or_stored_back_under_a_re_spelled_key_is_clean() {
    assert_clean(&map_entry_pushed(Vec::new()));
}

#[test]
fn a_map_entry_whose_key_is_reassigned_before_the_lookup_is_flagged() {
    assert_one_finding(&map_entry_pushed(vec![set(3, 2)]), "_9", rt::LIST_PUSH);
}

#[test]
fn a_map_entry_written_with_no_check_is_flagged() {
    let mut body = map_entry_pushed(Vec::new());
    body.basic_blocks[2].terminator = Some(Terminator::new(
        runtime(
            rt::MAP_GET_CHECKED,
            vec![copy(local(1)), copy(local(3))],
            9,
            8,
        ),
        span(),
    ));
    assert_one_finding(&body, "_9", rt::LIST_PUSH);
}

/// `_4 = move _1; goto; _1 = move _4; push(_1)`, with `_1` copied on write
/// first when `is_copied` holds.
fn round_trip(is_copied: bool) -> Body {
    let made = if is_copied {
        runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2)
    } else {
        runtime(rt::LIST_NEW, vec![constant(8)], 2, 2)
    };
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(Vec::new(), made),
        block(vec![move_into(1, 2), move_into(4, 1)], goto(3)),
        block(vec![move_into(1, 4)], push(local(1), 3, 4)),
        returning(Vec::new()),
    ];
    body_of(&list_locals(), 0, blocks)
}

#[test]
fn a_list_moved_out_and_straight_back_stays_checked() {
    assert_clean(&round_trip(true));
}

#[test]
fn a_list_moved_out_and_back_that_was_never_checked_is_flagged() {
    assert_one_finding(&round_trip(false), "_1", rt::LIST_PUSH);
}

#[test]
fn a_list_stored_as_an_element_is_not_a_write() {
    let mut locals = list_locals();
    locals[1] = list_of(list());
    locals[2] = list_of(list());
    let blocks = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            Vec::new(),
            runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2),
        ),
        block(vec![move_into(1, 2)], goto(3)),
        block(
            vec![retain(local(4))],
            runtime(rt::LIST_PUSH, vec![copy(local(1)), copy(local(4))], 3, 4),
        ),
        returning(Vec::new()),
    ];
    assert_clean(&body_of(&locals, 0, blocks));
}

#[test]
fn a_list_at_the_written_position_of_the_same_entry_point_is_flagged() {
    let blocks = vec![
        block(
            vec![retain(local(4))],
            runtime(rt::LIST_PUSH, vec![copy(local(4)), constant(7)], 3, 1),
        ),
        returning(Vec::new()),
    ];
    assert_one_finding(&body_of(&list_locals(), 0, blocks), "_4", rt::LIST_PUSH);
}

/// A body whose only reachable block returns, and whose `bb1` pushes onto an
/// unchecked list; `bb0` jumps to it when `is_reached` holds.
fn stray_write(is_reached: bool) -> Body {
    let first = if is_reached {
        goto(1)
    } else {
        TerminatorKind::Return
    };
    let blocks = vec![
        block(Vec::new(), first),
        block(Vec::new(), push(local(4), 3, 2)),
        returning(Vec::new()),
    ];
    body_of(&list_locals(), 0, blocks)
}

#[test]
fn a_write_in_an_unreachable_block_is_not_reported() {
    assert_clean(&stray_write(false));
}

#[test]
fn the_same_write_once_reachable_is_reported() {
    assert_one_finding(&stray_write(true), "_4", rt::LIST_PUSH);
}

const UNKNOWN: &str = "miri_rt_user_fill";

/// An entry point no table classifies, handed `_1` after `made` defined it.
fn unknown_writer(made: Vec<BasicBlockData>) -> Body {
    let count = made.len();
    let mut blocks = made;
    blocks.push(block(
        Vec::new(),
        runtime(UNKNOWN, vec![copy(local(1))], 3, count + 1),
    ));
    blocks.push(returning(Vec::new()));
    body_of(&list_locals(), 0, blocks)
}

#[test]
fn an_unknown_entry_point_handed_an_unchecked_list_is_flagged() {
    let made = vec![block(
        Vec::new(),
        runtime(rt::LIST_NEW, vec![constant(8)], 1, 1),
    )];
    assert_one_finding(&unknown_writer(made), "_1", UNKNOWN);
}

#[test]
fn an_unknown_entry_point_handed_a_copied_list_is_clean() {
    let made = vec![
        block(Vec::new(), runtime(rt::LIST_NEW, vec![constant(8)], 1, 1)),
        block(
            Vec::new(),
            runtime(rt::LIST_COW, vec![moved(local(1))], 2, 2),
        ),
        block(vec![move_into(1, 2)], goto(3)),
    ];
    assert_clean(&unknown_writer(made));
}

/// `_1 = <aggregate of kind>(..); fill(_1)`, `_1` of `kind_of_one`.
fn aggregate_written(kind: AggregateKind, kind_of_one: TypeKind, operands: Vec<Operand>) -> Body {
    let locals = vec![TypeKind::Void, kind_of_one, list(), TypeKind::Void];
    let blocks = vec![
        block(
            vec![assign(local(1), Rvalue::Aggregate(kind, operands))],
            runtime(UNKNOWN, vec![copy(local(1))], 3, 1),
        ),
        returning(Vec::new()),
    ];
    body_of(&locals, 0, blocks)
}

#[test]
fn a_struct_just_built_is_written_in_place_cleanly() {
    let body = aggregate_written(
        AggregateKind::Struct(ty(bag())),
        bag(),
        vec![moved(local(2))],
    );
    assert_clean(&body);
}

#[test]
fn an_array_just_built_may_be_immortal_and_is_flagged() {
    let body = aggregate_written(
        AggregateKind::Array,
        array(),
        vec![constant(1), constant(2), constant(3)],
    );
    assert_one_finding(&body, "_1", UNKNOWN);
}

/// Locals: 0 void, 1 `Crate`, 2 bool, 3 `[int]`, 4 void, 5 void, 6 `Crate`.
fn crate_locals() -> Vec<TypeKind> {
    vec![
        TypeKind::Void,
        crate_class(),
        TypeKind::Boolean,
        list(),
        TypeKind::Void,
        TypeKind::Void,
        crate_class(),
    ]
}

/// A class instance just built, its field checked unshared, then `between`
/// and `call` before the field is pushed onto.
fn crate_field_pushed(between: Vec<Statement>, call: Option<TerminatorKind>) -> Body {
    let built = assign(
        local(1),
        Rvalue::Aggregate(
            AggregateKind::Class(ty(crate_class())),
            vec![moved(local(3))],
        ),
    );
    let push_block = if call.is_some() { 4 } else { 3 };
    let mut blocks = vec![
        block(
            vec![built],
            runtime(rt::RC_IS_SHARED, vec![copy(field(1, 0))], 2, 1),
        ),
        block(Vec::new(), branch(2, 3, 2)),
        block(Vec::new(), TerminatorKind::Unreachable),
        block(
            between,
            call.map_or_else(|| goto(push_block), |kind| retarget(kind, push_block)),
        ),
    ];
    if push_block == 4 {
        blocks.push(block(Vec::new(), push(field(1, 0), 4, 5)));
    } else {
        blocks[3].terminator = Some(Terminator::new(push(field(1, 0), 4, 4), span()));
    }
    blocks.push(returning(Vec::new()));
    body_of(&crate_locals(), 0, blocks)
}

#[test]
fn a_class_instance_no_other_name_holds_keeps_its_checked_field_across_a_call() {
    let body = crate_field_pushed(Vec::new(), Some(program(Vec::new(), 5, 0)));
    assert_clean(&body);
}

#[test]
fn a_class_instance_another_name_holds_loses_its_checked_field_at_a_call() {
    let body = crate_field_pushed(
        vec![retain(local(1)), copy_into(6, 1)],
        Some(program(Vec::new(), 5, 0)),
    );
    assert_one_finding(&body, "_1.0", rt::LIST_PUSH);
}

#[test]
fn another_name_of_a_class_instance_does_not_share_the_fields_block() {
    let body = crate_field_pushed(vec![retain(local(1)), copy_into(6, 1)], None);
    assert_clean(&body);
}

#[test]
fn an_empty_map_just_built_is_written_in_place_cleanly() {
    let locals = vec![TypeKind::Void, map_of_lists(), list(), TypeKind::Void];
    let blocks = vec![
        block(
            vec![
                assign(local(1), Rvalue::Aggregate(AggregateKind::Map, Vec::new())),
                retain(local(2)),
            ],
            runtime(
                rt::MAP_SET,
                vec![copy(local(1)), constant(1), copy(local(2))],
                3,
                1,
            ),
        ),
        returning(Vec::new()),
    ];
    assert_clean(&body_of(&locals, 0, blocks));
}

#[test]
fn a_map_with_elements_built_by_aggregate_is_not_taken_as_fresh() {
    let locals = vec![TypeKind::Void, map_of_lists(), list(), TypeKind::Void];
    let blocks = vec![
        block(
            vec![assign(
                local(1),
                Rvalue::Aggregate(AggregateKind::Map, vec![constant(1), copy(local(2))]),
            )],
            runtime(
                rt::MAP_SET,
                vec![copy(local(1)), constant(1), copy(local(2))],
                3,
                1,
            ),
        ),
        returning(Vec::new()),
    ];
    assert_one_finding(&body_of(&locals, 0, blocks), "_1", rt::MAP_SET);
}

#[test]
fn the_host_array_a_readback_fills_is_not_checked() {
    let locals = vec![TypeKind::Void, array(), TypeKind::Void];
    let blocks = vec![
        block(
            vec![assign(
                local(1),
                Rvalue::Aggregate(AggregateKind::Array, vec![constant(0)]),
            )],
            runtime("miri_gpu_readback", vec![constant(3), copy(local(1))], 2, 1),
        ),
        returning(Vec::new()),
    ];
    assert_clean(&body_of(&locals, 0, blocks));
}
