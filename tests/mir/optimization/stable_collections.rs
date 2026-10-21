// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that records the collections a loop leaves the header
//! of unchanged.
//!
//! Each hand-built body is one loop indexing a list parameter; every test
//! changes one thing about it and checks whether the list is still recorded.
//! The programs check the same through the whole pipeline.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::body::DeviceHandleId;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Constant, Discriminant, ExecutionModel, Local,
    LocalDecl, Operand, Place, PlaceElem, Rvalue, StableCollection, Statement, StatementKind,
    Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;
use miri::runtime_fns::rt;

const LIST: Local = Local(1);
const INDEX: Local = Local(2);
const LENGTH: Local = Local(3);
const BELOW: Local = Local(4);
const ELEMENT: Local = Local(5);
const SPARE_LIST: Local = Local(6);
const SPARE: Local = Local(7);

const LOOP_HEADER: usize = 1;
const LOOP_BODY: usize = 2;

pub(crate) fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

pub(crate) fn list_of(element: TypeKind) -> Type {
    let element = Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(ty(element)), false),
        span: Span::default(),
    };
    ty(TypeKind::List(Box::new(element)))
}

fn int(value: i64) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

fn statement(kind: StatementKind) -> Statement {
    Statement {
        kind,
        span: Span::default(),
    }
}

fn assign(place: Place, rvalue: Rvalue) -> Statement {
    statement(StatementKind::Assign(place, rvalue))
}

fn terminator(kind: TerminatorKind) -> Terminator {
    Terminator::new(kind, Span::default())
}

fn goto(target: usize) -> Terminator {
    terminator(TerminatorKind::Goto {
        target: BasicBlock(target),
    })
}

fn indexed(local: Local) -> Place {
    Place {
        local,
        projection: vec![PlaceElem::Index(INDEX)],
    }
}

/// `fn f(list [int]) int`: `i = 0; while i < list.length(): x = list[i]; i += 1`.
pub(crate) fn loop_over_a_list() -> Body {
    let mut body = Body::new(1, Span::default(), ExecutionModel::Cpu);
    body.local_decls = [
        ty(TypeKind::Int),
        list_of(TypeKind::Int),
        ty(TypeKind::Int),
        ty(TypeKind::Int),
        ty(TypeKind::Boolean),
        ty(TypeKind::Int),
        list_of(TypeKind::Int),
        ty(TypeKind::Int),
    ]
    .into_iter()
    .map(|declared| LocalDecl::new(declared, Span::default()))
    .collect();

    let mut entry = BasicBlockData::new(Some(goto(LOOP_HEADER)));
    entry
        .statements
        .push(assign(Place::new(INDEX), Rvalue::Use(int(0))));

    let mut header = BasicBlockData::new(Some(terminator(TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(BELOW)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(LOOP_BODY))],
        otherwise: BasicBlock(3),
    })));
    header.statements.extend([
        assign(Place::new(LENGTH), Rvalue::Len(Place::new(LIST))),
        assign(
            Place::new(BELOW),
            Rvalue::BinaryOp(
                BinOp::Lt,
                Box::new(Operand::Copy(Place::new(INDEX))),
                Box::new(Operand::Copy(Place::new(LENGTH))),
            ),
        ),
    ]);

    let mut step = BasicBlockData::new(Some(goto(LOOP_HEADER)));
    step.statements.extend([
        assign(
            Place::new(ELEMENT),
            Rvalue::Use(Operand::Copy(indexed(LIST))),
        ),
        assign(
            Place::new(INDEX),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(INDEX))),
                Box::new(int(1)),
            ),
        ),
    ]);

    let exit = BasicBlockData::new(Some(terminator(TerminatorKind::Return)));
    body.basic_blocks = vec![entry, header, step, exit];
    body
}

/// The records `body` gets.
fn records_of(mut body: Body) -> Vec<StableCollection> {
    record_stable_collections(&mut body);
    body.stable_collections
}

fn call(func: Operand, args: Vec<Operand>, destination: Local, target: usize) -> Terminator {
    terminator(TerminatorKind::Call {
        func,
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: Place::new(destination),
        target: Some(BasicBlock(target)),
    })
}

/// `body` with the loop's step ending in `ending`.
fn with_step_ending(mut body: Body, ending: Terminator) -> Body {
    body.basic_blocks[LOOP_BODY].terminator = Some(ending);
    body
}

/// `body` with `extra` appended to the loop's step.
fn with_step_statement(mut body: Body, extra: Statement) -> Body {
    body.basic_blocks[LOOP_BODY].statements.push(extra);
    body
}

#[test]
fn test_a_list_a_loop_only_reads_is_recorded_with_a_preheader_of_its_own() {
    let mut body = loop_over_a_list();
    assert!(record_stable_collections(&mut body));
    let preheader = body.basic_blocks.len() - 1;
    assert_eq!(
        body.stable_collections,
        vec![StableCollection {
            local: LIST,
            preheader: BasicBlock(preheader),
            header: BasicBlock(LOOP_HEADER),
            blocks: vec![BasicBlock(LOOP_HEADER), BasicBlock(LOOP_BODY)],
        }]
    );
    assert_eq!(
        body.basic_blocks[0]
            .terminator
            .as_ref()
            .map(|t| t.successors()),
        Some(vec![BasicBlock(preheader)]),
        "the entry into the loop goes through the preheader"
    );
    assert_eq!(
        body.basic_blocks[preheader]
            .terminator
            .as_ref()
            .map(|t| t.successors()),
        Some(vec![BasicBlock(LOOP_HEADER)])
    );
    assert!(body.to_string().contains(&format!(
        "stable _1 from bb{preheader} over bb1: [bb1, bb2];"
    )));
}

#[test]
fn test_a_push_in_the_loop_leaves_the_list_unrecorded() {
    let push = call(
        Operand::runtime(rt::LIST_PUSH, Span::default()),
        vec![Operand::Copy(Place::new(LIST)), int(1)],
        SPARE,
        LOOP_HEADER,
    );
    assert!(records_of(with_step_ending(loop_over_a_list(), push)).is_empty());
}

#[test]
fn test_a_call_to_a_function_of_the_program_leaves_the_list_unrecorded() {
    let user_call = call(
        Operand::function(Symbol::entry(), Span::default()),
        Vec::new(),
        SPARE,
        LOOP_HEADER,
    );
    assert!(records_of(with_step_ending(loop_over_a_list(), user_call)).is_empty());
}

#[test]
fn test_asking_whether_the_list_is_shared_keeps_it_recorded() {
    let ask = call(
        Operand::runtime(rt::RC_IS_SHARED, Span::default()),
        vec![Operand::Copy(Place::new(LIST))],
        BELOW,
        LOOP_HEADER,
    );
    assert_eq!(
        records_of(with_step_ending(loop_over_a_list(), ask)).len(),
        1
    );
}

#[test]
fn test_rebinding_the_list_in_the_loop_leaves_it_unrecorded() {
    let rebind = assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    );
    assert!(records_of(with_step_statement(loop_over_a_list(), rebind)).is_empty());
}

#[test]
fn test_moving_the_list_out_and_straight_back_keeps_it_recorded() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_BODY].statements.push(assign(
        Place::new(SPARE_LIST),
        Rvalue::Use(Operand::Move(Place::new(LIST))),
    ));
    body.basic_blocks[LOOP_BODY].terminator = Some(goto(4));
    let mut back = BasicBlockData::new(Some(goto(LOOP_HEADER)));
    back.statements.push(assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    ));
    body.basic_blocks.push(back);
    assert_eq!(records_of(body).len(), 1);
}

#[test]
fn test_moving_back_a_list_moved_out_before_the_loop_leaves_it_unrecorded() {
    let mut body = loop_over_a_list();
    body.basic_blocks[0].statements.push(assign(
        Place::new(SPARE_LIST),
        Rvalue::Use(Operand::Move(Place::new(LIST))),
    ));
    let back = assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    );
    body.basic_blocks[LOOP_BODY].statements.push(back);
    assert!(records_of(body).is_empty());
}

#[test]
fn test_a_release_that_may_run_a_drop_hook_leaves_the_list_unrecorded() {
    let mut body = loop_over_a_list();
    let resource = body.new_local(LocalDecl::new(
        ty(TypeKind::Custom("Resource".to_string(), None)),
        Span::default(),
    ));
    let release = statement(StatementKind::DecRef(Place::new(resource)));
    assert!(records_of(with_step_statement(body, release)).is_empty());
}

#[test]
fn test_releasing_a_string_keeps_the_list_recorded() {
    let mut body = loop_over_a_list();
    let text = body.new_local(LocalDecl::new(ty(TypeKind::String), Span::default()));
    let release = statement(StatementKind::DecRef(Place::new(text)));
    assert_eq!(records_of(with_step_statement(body, release)).len(), 1);
}

#[test]
fn test_writing_an_element_that_may_run_a_drop_hook_leaves_the_list_unrecorded() {
    let mut body = loop_over_a_list();
    let resource = TypeKind::Custom("Resource".to_string(), None);
    let resources = body.new_local(LocalDecl::new(list_of(resource.clone()), Span::default()));
    let fresh = body.new_local(LocalDecl::new(ty(resource), Span::default()));
    let write = assign(
        indexed(resources),
        Rvalue::Use(Operand::Move(Place::new(fresh))),
    );
    assert!(records_of(with_step_statement(body, write)).is_empty());
}

#[test]
fn test_writing_an_int_element_keeps_the_list_recorded() {
    let write = assign(indexed(LIST), Rvalue::Use(int(7)));
    assert_eq!(
        records_of(with_step_statement(loop_over_a_list(), write)).len(),
        1
    );
}

#[test]
fn test_a_store_through_a_dereference_leaves_the_list_unrecorded() {
    let mut body = loop_over_a_list();
    let pointer = body.new_local(LocalDecl::new(ty(TypeKind::RawPtr), Span::default()));
    let store = assign(
        Place {
            local: pointer,
            projection: vec![PlaceElem::Deref],
        },
        Rvalue::Use(int(0)),
    );
    assert!(records_of(with_step_statement(body, store)).is_empty());
}

#[test]
fn test_a_list_whose_address_is_taken_is_not_recorded() {
    let mut body = loop_over_a_list();
    let pointer = body.new_local(LocalDecl::new(ty(TypeKind::RawPtr), Span::default()));
    body.basic_blocks[0]
        .statements
        .push(assign(Place::new(pointer), Rvalue::Ref(Place::new(LIST))));
    assert!(records_of(body).is_empty());
}

#[test]
fn test_a_captured_list_is_not_recorded() {
    let mut body = loop_over_a_list();
    body.env_capture_locals.push(LIST);
    assert!(records_of(body).is_empty());
}

#[test]
fn test_a_device_resident_list_is_not_recorded() {
    let mut body = loop_over_a_list();
    body.local_decls[LIST.0].device_handle = Some(DeviceHandleId(1));
    assert!(records_of(body).is_empty());
}

#[test]
fn test_a_gpu_body_records_nothing() {
    let mut body = loop_over_a_list();
    body.execution_model = ExecutionModel::GpuKernel;
    assert!(!record_stable_collections(&mut body));
    assert!(body.stable_collections.is_empty());
    assert_eq!(body.basic_blocks.len(), 4, "a GPU body gains no block");
}

#[test]
fn test_ending_the_storage_of_the_list_in_the_loop_leaves_it_unrecorded() {
    let dead = statement(StatementKind::StorageDead(Place::new(LIST)));
    assert!(records_of(with_step_statement(loop_over_a_list(), dead)).is_empty());
}

/// The bodies `source` compiles to, by the suffix of their link name.
fn body_named(source: &str, suffix: &str) -> Body {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("pipeline should lower the program")
        .into_iter()
        .find(|(name, _)| name.ends_with(suffix))
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("no body named *{suffix}"))
}

const BODIES: &str = r#"
use system.io
use system.collections.list

struct Body
    x float
    vx float
    mass float

class Plain
    var id int

class Noisy
    var id int

    fn drop(self)
        println("dropped")
"#;

fn program(function: &str) -> String {
    format!("{BODIES}\n{function}\n\nfn main()\n    println(\"ok\")\n")
}

#[test]
fn test_a_pair_loop_reading_a_list_of_structs_records_it_once_for_the_outer_loop() {
    let source = program(
        r#"
fn energy(bodies [Body]) float
    var e = 0.0
    let n = bodies.length()
    var i = 0
    while i < n
        e = e + bodies[i].mass * bodies[i].vx
        var j = i + 1
        while j < n
            e = e - bodies[i].mass * bodies[j].x
            j += 1
        i += 1
    e
"#,
    );
    let body = body_named(&source, ".energy");
    assert_eq!(
        body.stable_collections.len(),
        1,
        "one record, for the outer loop:\n{body}"
    );
    let record = &body.stable_collections[0];
    assert_eq!(record.local, Local(1));
}

#[test]
fn test_a_push_in_a_program_loop_leaves_its_list_unrecorded() {
    let source = program(
        r#"
fn grow(n int) int
    var l = List([1, 2, 3])
    var i = 0
    var total = 0
    while i < n
        total = total + l[i % 3]
        l.push(i)
        i += 1
    total
"#,
    );
    let body = body_named(&source, ".grow");
    assert!(body.stable_collections.is_empty(), "{body}");
}

#[test]
fn test_a_call_in_a_program_loop_leaves_its_list_unrecorded() {
    let source = program(
        r#"
fn helper(x int) int
    x + 1

fn sum(l [int]) int
    var i = 0
    var total = 0
    while i < l.length()
        total = total + helper(l[i])
        i += 1
    total
"#,
    );
    let body = body_named(&source, ".sum");
    assert!(body.stable_collections.is_empty(), "{body}");
}

/// A loop building and dropping an instance of `class` every iteration while
/// it reads a list.
fn releasing_loop(class: &str) -> String {
    program(&format!(
        r#"
fn count(start [int]) int
    let l = start
    var i = 0
    var total = 0
    while i < l.length()
        let c = {class}(id: l[i])
        total = total + c.id
        i += 1
    total
"#
    ))
}

#[test]
fn test_releasing_an_instance_with_a_drop_hook_in_the_loop_leaves_its_list_unrecorded() {
    let body = body_named(&releasing_loop("Noisy"), ".count");
    assert!(body.stable_collections.is_empty(), "{body}");
}

#[test]
fn test_releasing_an_instance_without_a_drop_hook_keeps_its_list_recorded() {
    let body = body_named(&releasing_loop("Plain"), ".count");
    assert_eq!(body.stable_collections.len(), 1, "{body}");
}

#[test]
fn test_a_pair_loop_writing_through_a_list_of_structs_records_it_and_calls_nothing() {
    let source = program(
        r#"
fn step(start [Body], dt float) [Body]
    var bodies = start
    let n = bodies.length()
    var i = 0
    while i < n
        var j = i + 1
        while j < n
            let dx = bodies[i].x - bodies[j].x
            bodies[i].vx = bodies[i].vx - dx * bodies[j].mass * dt
            bodies[j].vx = bodies[j].vx + dx * bodies[i].mass * dt
            j += 1
        i += 1
    bodies
"#,
    );
    let body = body_named(&source, ".step");
    let [record] = body.stable_collections.as_slice() else {
        panic!("one record, for the inner loop:\n{body}");
    };
    let unreachable = body.find_unreachable_blocks();
    let calls_inside = record
        .blocks
        .iter()
        .filter(|block| !unreachable.contains(&block.0))
        .filter(|block| {
            matches!(
                body.basic_blocks[block.0]
                    .terminator
                    .as_ref()
                    .map(|t| &t.kind),
                Some(TerminatorKind::Call { .. })
            )
        })
        .count();
    assert_eq!(calls_inside, 0, "the pair loop still calls:\n{body}");
}

const HOLDER: &str = r#"
use system.io
use system.collections.list

class Holder
    var items [int]
"#;

fn holder_program(function: &str) -> String {
    format!("{HOLDER}\n{function}\n\nfn main()\n    println(\"ok\")\n")
}

#[test]
fn test_a_borrowed_list_whose_owner_a_loop_replaces_is_not_recorded() {
    // The store into `hh.items` may release the list `l` borrows, so its
    // header cannot be read once for the whole loop.
    let source = holder_program(
        r#"
fn go(h Holder, l [int], repl [int]) int
    var hh = h
    var i = 0
    var t = 0
    while i < l.length()
        t = t + l[i]
        hh.items = repl
        i += 1
    t
"#,
    );
    let body = body_named(&source, ".go");
    assert!(body.stable_collections.is_empty(), "{body}");
}

#[test]
fn test_a_list_the_body_holds_a_reference_to_stays_recorded_when_its_owner_is_replaced() {
    let source = holder_program(
        r#"
fn go(start Holder, repl [int]) int
    var h = start
    let l = h.items
    var i = 0
    var t = 0
    while i < l.length()
        t = t + l[i]
        h.items = repl
        i += 1
    t
"#,
    );
    let body = body_named(&source, ".go");
    assert_eq!(body.stable_collections.len(), 1, "{body}");
}

#[test]
fn test_a_borrowed_list_read_beside_string_releases_stays_recorded() {
    let source = holder_program(
        r#"
fn go(l [int], names [String]) int
    var i = 0
    var t = 0
    while i < l.length()
        let s = names[0]
        t = t + l[i]
        i += 1
    t
"#,
    );
    let body = body_named(&source, ".go");
    assert!(
        body.stable_collections
            .iter()
            .any(|record| record.local == Local(1)),
        "{body}"
    );
}

#[test]
fn test_a_borrowed_list_over_a_loop_storing_a_list_into_another_is_not_recorded() {
    let mut body = loop_over_a_list();
    let rows = body.new_local(LocalDecl::new(
        list_of(list_of(TypeKind::Int).kind),
        Span::default(),
    ));
    let write = assign(
        indexed(rows),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    );
    let records = records_of(with_step_statement(body, write));
    assert!(
        records.iter().all(|record| record.local != LIST),
        "{records:?}"
    );
}

#[test]
fn test_rebinding_a_value_with_a_drop_hook_in_the_loop_leaves_the_list_unrecorded() {
    // A whole-local reassignment releases the old value only through the
    // explicit `DecRef` reference counting puts before it, which the pass
    // reads; the reassignment itself is not checked for a hook.
    let mut body = loop_over_a_list();
    let resource = TypeKind::Custom("Resource".to_string(), None);
    let held = body.new_local(LocalDecl::new(ty(resource.clone()), Span::default()));
    let fresh = body.new_local(LocalDecl::new(ty(resource), Span::default()));
    body.basic_blocks[LOOP_BODY].statements.extend([
        statement(StatementKind::DecRef(Place::new(held))),
        statement(StatementKind::Reassign(
            Place::new(held),
            Rvalue::Use(Operand::Move(Place::new(fresh))),
        )),
    ]);
    assert!(records_of(body).is_empty());
}
