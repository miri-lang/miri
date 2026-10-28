// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that records the collections a loop leaves the header
//! of unchanged.
//!
//! Each hand-built body is one loop indexing a list parameter; every test
//! changes one thing about it and checks whether the list is still recorded.
//! The programs check the same through the whole pipeline.

use crate::integration::memory::loop_headers_across_calls::{
    callee_writing_its_parameter, ALIAS_BOUND_IN_THE_LOOP, BORROWED_PARAMETER_COPIED_IN_THE_LOOP,
    CLOSURE_IN_A_FIELD, CLOSURE_PUSHING_ITS_CAPTURE, COPY_ON_WRITE_OF_ANOTHER_LIST,
    COPY_ON_WRITE_OF_AN_ALIAS, INDEX_WRITTEN_THROUGH_OUT, LIST_FROM_AN_ELEMENT_ACCESSOR,
    LIST_FROM_A_MAP_VALUE_ACCESSOR, LIST_FROM_A_METHOD_RETURNING_A_FIELD, LOOP_OVER_A_FIELD,
    OUT_ARGUMENT_ALIAS, OWNED_PARAMETER_TRANSFER, RECURSIVE_CALLEE, REGISTER_MACHINE, SPHERES,
    THROUGH_ANOTHER_BINDING, THROUGH_AN_ELEMENT_POINTING_BACK, THROUGH_A_CLASS_FIELD,
    THROUGH_A_PASSED_CLOSURE, VIRTUAL_CALL, WRITER_THROUGH_A_COUNTED_FIELD,
};
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::analysis::dominators::DominatorTree;
use miri::mir::analysis::loops::natural_loops;
use miri::mir::body::DeviceHandleId;
use miri::mir::optimization::stable_collections::record_stable_collections;
use miri::mir::symbol::Symbol;
use miri::mir::{
    AggregateKind, BasicBlock, BasicBlockData, BinOp, Body, Constant, Discriminant, ExecutionModel,
    Local, LocalDecl, Operand, Place, PlaceElem, Rvalue, StableCollection, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;
use miri::runtime_fns::rt;

pub(crate) const LIST: Local = Local(1);
pub(crate) const INDEX: Local = Local(2);
pub(crate) const LENGTH: Local = Local(3);
pub(crate) const BELOW: Local = Local(4);
pub(crate) const ELEMENT: Local = Local(5);
pub(crate) const SPARE_LIST: Local = Local(6);
pub(crate) const SPARE: Local = Local(7);

pub(crate) const LOOP_HEADER: usize = 1;
pub(crate) const LOOP_BODY: usize = 2;

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

pub(crate) fn int(value: i64) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

pub(crate) fn statement(kind: StatementKind) -> Statement {
    Statement {
        kind,
        span: Span::default(),
    }
}

pub(crate) fn assign(place: Place, rvalue: Rvalue) -> Statement {
    statement(StatementKind::Assign(place, rvalue))
}

pub(crate) fn terminator(kind: TerminatorKind) -> Terminator {
    Terminator::new(kind, Span::default())
}

fn goto(target: usize) -> Terminator {
    terminator(TerminatorKind::Goto {
        target: BasicBlock(target),
    })
}

pub(crate) fn indexed(local: Local) -> Place {
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

pub(crate) fn call(
    func: Operand,
    args: Vec<Operand>,
    destination: Local,
    target: usize,
) -> Terminator {
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
fn test_a_pair_loop_writing_through_a_list_of_structs_records_it_once_for_the_outer_loop() {
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
        panic!("one record, for the outer loop:\n{body}");
    };
    assert!(
        holds_another_loop(&body, record),
        "the record is not over the outer loop:\n{body}"
    );
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

/// Whether `record`'s blocks hold the header of a loop other than its own.
fn holds_another_loop(body: &Body, record: &StableCollection) -> bool {
    let dominators = DominatorTree::compute(body);
    natural_loops(body, &dominators)
        .iter()
        .map(|found| BasicBlock(found.header))
        .any(|header| header != record.header && record.blocks.contains(&header))
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

/// `loop_over_a_list` with the list a local the body builds before the loop
/// rather than a parameter it borrows.
pub(crate) fn loop_over_an_owned_list() -> Body {
    let mut body = loop_over_a_list();
    body.arg_count = 0;
    body.basic_blocks[0].statements.insert(
        0,
        assign(
            Place::new(LIST),
            Rvalue::Aggregate(AggregateKind::List, Vec::new()),
        ),
    );
    body
}

/// A call to a function of the program, handed `args`, ending the loop's step.
pub(crate) fn program_call(args: Vec<Operand>) -> Terminator {
    call(
        Operand::function(Symbol::entry(), Span::default()),
        args,
        SPARE,
        LOOP_HEADER,
    )
}

#[test]
fn test_a_call_naming_nothing_of_an_owned_list_keeps_it_recorded() {
    let body = with_step_ending(loop_over_an_owned_list(), program_call(vec![int(1)]));
    let records = records_of(body);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].local, LIST);
}

#[test]
fn test_a_call_handed_an_owned_list_leaves_it_unrecorded() {
    let handed = program_call(vec![Operand::Copy(Place::new(LIST))]);
    assert!(records_of(with_step_ending(loop_over_an_owned_list(), handed)).is_empty());
}

#[test]
fn test_a_call_handed_a_copy_of_an_owned_list_leaves_it_unrecorded() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements.push(assign(
        Place::new(SPARE_LIST),
        Rvalue::Use(Operand::Copy(Place::new(LIST))),
    ));
    let handed = program_call(vec![Operand::Copy(Place::new(SPARE_LIST))]);
    assert!(records_of(with_step_ending(body, handed)).is_empty());
}

#[test]
fn test_a_call_writing_its_result_into_a_copy_of_an_owned_list_leaves_it_unrecorded() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements.push(assign(
        Place::new(SPARE_LIST),
        Rvalue::Use(Operand::Copy(Place::new(LIST))),
    ));
    let mut into_copy = program_call(Vec::new());
    if let TerminatorKind::Call { destination, .. } = &mut into_copy.kind {
        *destination = Place::new(SPARE_LIST);
    }
    assert!(records_of(with_step_ending(body, into_copy)).is_empty());
}

#[test]
fn test_a_call_handed_a_list_borrowed_from_an_owned_list_leaves_it_unrecorded() {
    let mut body = loop_over_an_owned_list();
    body.borrowed_locals.insert(SPARE_LIST, vec![LIST]);
    let handed = program_call(vec![Operand::Copy(Place::new(SPARE_LIST))]);
    assert!(records_of(with_step_ending(body, handed)).is_empty());
}

#[test]
fn test_a_call_handed_a_string_borrowed_from_an_owned_list_keeps_it_recorded() {
    let mut body = loop_over_an_owned_list();
    let text = body.new_local(LocalDecl::new(ty(TypeKind::String), Span::default()));
    body.borrowed_locals.insert(text, vec![LIST]);
    let handed = program_call(vec![Operand::Copy(Place::new(text))]);
    assert_eq!(records_of(with_step_ending(body, handed)).len(), 1);
}

#[test]
fn test_an_owned_list_borrowed_from_another_is_unrecorded_across_a_call() {
    let mut body = loop_over_an_owned_list();
    body.borrowed_locals.insert(LIST, vec![SPARE_LIST]);
    let unrelated = program_call(vec![int(1)]);
    assert!(records_of(with_step_ending(body, unrelated)).is_empty());
}

#[test]
fn test_an_owned_list_copied_without_a_retain_is_unrecorded_across_a_call() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements[0] = assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Copy(Place::new(SPARE_LIST))),
    );
    let unrelated = program_call(vec![int(1)]);
    assert!(records_of(with_step_ending(body, unrelated)).is_empty());
}

#[test]
fn test_an_owned_list_copied_after_a_retain_stays_recorded_across_a_call() {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements[0] = assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Copy(Place::new(SPARE_LIST))),
    );
    body.basic_blocks[0]
        .statements
        .insert(0, statement(StatementKind::IncRef(Place::new(SPARE_LIST))));
    let unrelated = program_call(vec![int(1)]);
    assert_eq!(records_of(with_step_ending(body, unrelated)).len(), 1);
}

#[test]
fn test_a_parameter_the_body_takes_over_stays_recorded_across_a_call() {
    let mut body = loop_over_a_list();
    body.owned_params = vec![true];
    let unrelated = program_call(vec![int(1)]);
    assert_eq!(records_of(with_step_ending(body, unrelated)).len(), 1);
}

#[test]
fn test_a_borrowed_parameter_is_unrecorded_across_a_call_naming_nothing_of_it() {
    let unrelated = program_call(vec![int(1)]);
    assert!(records_of(with_step_ending(loop_over_a_list(), unrelated)).is_empty());
}

#[test]
fn test_a_virtual_call_leaves_an_owned_list_unrecorded() {
    let virtual_call = terminator(TerminatorKind::VirtualCall {
        vtable_slot: 0,
        args: vec![Operand::Copy(Place::new(SPARE_LIST))],
        arg_modes: Vec::new(),
        param_types: Vec::new(),
        destination: Place::new(SPARE),
        target: Some(BasicBlock(LOOP_HEADER)),
    });
    assert!(records_of(with_step_ending(loop_over_an_owned_list(), virtual_call)).is_empty());
}

#[test]
fn test_a_closure_built_around_an_owned_list_and_called_leaves_it_unrecorded() {
    let mut body = loop_over_an_owned_list();
    let environment = ty(TypeKind::Custom("Environment".to_string(), None));
    let closure = body.new_local(LocalDecl::new(environment.clone(), Span::default()));
    body.basic_blocks[0].statements.push(assign(
        Place::new(closure),
        Rvalue::Aggregate(
            AggregateKind::Class(environment),
            vec![Operand::Copy(Place::new(LIST))],
        ),
    ));
    let through_closure = call(
        Operand::Copy(Place::new(closure)),
        vec![int(1)],
        SPARE,
        LOOP_HEADER,
    );
    assert!(records_of(with_step_ending(body, through_closure)).is_empty());
}

#[test]
fn test_a_call_handed_a_pointer_borrowed_from_an_owned_list_leaves_it_unrecorded() {
    let mut body = loop_over_an_owned_list();
    let pointer = body.new_local(LocalDecl::new(ty(TypeKind::RawPtr), Span::default()));
    body.borrowed_locals.insert(pointer, vec![LIST]);
    let handed = program_call(vec![Operand::Copy(Place::new(pointer))]);
    assert!(records_of(with_step_ending(body, handed)).is_empty());
}

/// `loop_over_an_owned_list` with the list moved out of `SPARE_LIST` rather
/// than built, and its step ending in a call naming nothing of either.
fn loop_over_a_list_moved_out_of_a_spare() -> Body {
    let mut body = loop_over_an_owned_list();
    body.basic_blocks[0].statements[0] = assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    );
    with_step_ending(body, program_call(vec![int(1)]))
}

#[test]
fn test_an_owned_list_moved_out_of_a_local_holding_its_own_reference_stays_recorded() {
    assert_eq!(records_of(loop_over_a_list_moved_out_of_a_spare()).len(), 1);
}

#[test]
fn test_an_owned_list_moved_out_of_a_captured_binding_is_unrecorded_across_a_call() {
    let mut body = loop_over_a_list_moved_out_of_a_spare();
    body.env_capture_locals.push(SPARE_LIST);
    assert!(records_of(body).is_empty());
}

#[test]
fn test_an_owned_list_moved_out_of_an_address_taken_local_is_unrecorded_across_a_call() {
    let mut body = loop_over_a_list_moved_out_of_a_spare();
    let pointer = body.new_local(LocalDecl::new(ty(TypeKind::RawPtr), Span::default()));
    body.basic_blocks[0].statements.insert(
        0,
        assign(Place::new(pointer), Rvalue::Ref(Place::new(SPARE_LIST))),
    );
    assert!(records_of(body).is_empty());
}

#[test]
fn test_an_owned_list_moved_out_of_a_device_resident_local_is_unrecorded_across_a_call() {
    let mut body = loop_over_a_list_moved_out_of_a_spare();
    body.local_decls[SPARE_LIST.0].device_handle = Some(DeviceHandleId(1));
    assert!(records_of(body).is_empty());
}

/// The body of `source` named `suffix`, its records checked to cover no
/// loop.
fn assert_unrecorded(source: &str, suffix: &str) {
    let body = body_named(source, suffix);
    assert!(body.stable_collections.is_empty(), "{body}");
}

#[test]
fn test_a_loop_calling_a_function_handed_its_list_twice_is_unrecorded() {
    assert_unrecorded(&callee_writing_its_parameter("push(9)"), "main");
}

#[test]
fn test_a_loop_calling_a_function_handed_a_holder_of_its_list_is_unrecorded() {
    assert_unrecorded(THROUGH_A_CLASS_FIELD, "main");
    assert_unrecorded(THROUGH_AN_ELEMENT_POINTING_BACK, "main");
}

#[test]
fn test_a_loop_calling_a_closure_capturing_its_list_is_unrecorded() {
    assert_unrecorded(THROUGH_A_PASSED_CLOSURE, "main");
    assert_unrecorded(CLOSURE_PUSHING_ITS_CAPTURE, "main");
    assert_unrecorded(CLOSURE_IN_A_FIELD, "main");
}

#[test]
fn test_a_loop_calling_a_function_handed_another_binding_of_its_list_is_unrecorded() {
    assert_unrecorded(THROUGH_ANOTHER_BINDING, "main");
    assert_unrecorded(ALIAS_BOUND_IN_THE_LOOP, "main");
    assert_unrecorded(OUT_ARGUMENT_ALIAS, "main");
    assert_unrecorded(OWNED_PARAMETER_TRANSFER, "main");
}

#[test]
fn test_a_loop_over_a_parameter_calling_itself_is_unrecorded() {
    assert_unrecorded(RECURSIVE_CALLEE, ".walk");
}

#[test]
fn test_a_loop_making_a_virtual_call_is_unrecorded() {
    assert_unrecorded(VIRTUAL_CALL, "main");
}

/// The local of the one record of the body of `source` named `suffix`.
fn recorded_local(source: &str, suffix: &str) -> (Body, Local) {
    let body = body_named(source, suffix);
    let [record] = body.stable_collections.as_slice() else {
        panic!("one record:\n{body}");
    };
    let local = record.local;
    (body, local)
}

#[test]
fn test_a_loop_over_spheres_calling_functions_records_the_list_and_its_access() {
    let (body, local) = recorded_local(SPHERES, ".shade");
    assert!(
        body.in_bounds.iter().any(|entry| entry.collection == local),
        "the element read is proven in bounds:\n{body}"
    );
}

#[test]
fn test_a_loop_over_a_list_its_calls_reach_only_through_counted_holders_is_recorded() {
    recorded_local(LOOP_OVER_A_FIELD, "main");
    recorded_local(WRITER_THROUGH_A_COUNTED_FIELD, "main");
    recorded_local(BORROWED_PARAMETER_COPIED_IN_THE_LOOP, ".sum");
}

#[test]
fn test_a_loop_over_a_list_a_holder_handed_out_through_a_call_is_recorded() {
    // The accessor's result holds a reference of its own, so a method
    // writing the holder's list copies it and the loop's header stays.
    recorded_local(LIST_FROM_AN_ELEMENT_ACCESSOR, "main");
    recorded_local(LIST_FROM_A_MAP_VALUE_ACCESSOR, "main");
    recorded_local(LIST_FROM_A_METHOD_RETURNING_A_FIELD, "main");
}

#[test]
fn test_an_index_a_call_writes_through_out_is_not_proven_in_bounds() {
    let (body, local) = recorded_local(INDEX_WRITTEN_THROUGH_OUT, "main");
    assert!(
        body.in_bounds.iter().all(|entry| entry.collection != local),
        "{body}"
    );
}

/// A call to the runtime entry point `name`, handed `SPARE_LIST` and writing
/// its result into `destination`, ending the loop's step.
fn copy_of_spare_list(name: &str, destination: Local) -> Terminator {
    call(
        Operand::runtime(name, Span::default()),
        vec![Operand::Move(Place::new(SPARE_LIST))],
        destination,
        LOOP_HEADER,
    )
}

#[test]
fn test_a_copy_on_write_of_another_list_keeps_the_list_recorded() {
    for name in [rt::LIST_COW, rt::LIST_CLONE] {
        let copy = copy_of_spare_list(name, SPARE_LIST);
        let records = records_of(with_step_ending(loop_over_a_list(), copy));
        assert_eq!(records.len(), 1, "{name}: {records:?}");
        assert_eq!(records[0].local, LIST, "{name}");
    }
}

#[test]
fn test_a_copy_on_write_of_another_array_keeps_the_list_recorded() {
    for name in [rt::ARRAY_COW, rt::ARRAY_CLONE] {
        let mut body = loop_over_a_list();
        let array = ty(TypeKind::Array(
            Box::new(type_expression(TypeKind::Int)),
            Box::new(type_expression(TypeKind::Int)),
        ));
        body.local_decls[SPARE_LIST.0] = LocalDecl::new(array, Span::default());
        let copy = copy_of_spare_list(name, SPARE_LIST);
        assert_eq!(records_of(with_step_ending(body, copy)).len(), 1, "{name}");
    }
}

#[test]
fn test_a_copy_of_a_list_whose_elements_may_run_a_clone_method_leaves_the_list_unrecorded() {
    // Copying a list of class instances calls each element's `clone`, code
    // of the program that may reach any collection.
    for name in [rt::LIST_COW, rt::LIST_CLONE] {
        let mut body = loop_over_a_list();
        let instances = list_of(TypeKind::Custom("Plain".to_string(), None));
        body.local_decls[SPARE_LIST.0] = LocalDecl::new(instances, Span::default());
        let copy = copy_of_spare_list(name, SPARE_LIST);
        assert!(
            records_of(with_step_ending(body, copy)).is_empty(),
            "{name}"
        );
    }
}

#[test]
fn test_a_copy_on_write_of_the_list_itself_leaves_it_unrecorded() {
    let cow = call(
        Operand::runtime(rt::LIST_COW, Span::default()),
        vec![Operand::Move(Place::new(LIST))],
        LIST,
        LOOP_HEADER,
    );
    assert!(records_of(with_step_ending(loop_over_a_list(), cow)).is_empty());
}

#[test]
fn test_a_copy_on_write_whose_result_the_list_takes_leaves_it_unrecorded() {
    let mut body = loop_over_a_list();
    body.basic_blocks[LOOP_BODY].terminator = Some(call(
        Operand::runtime(rt::LIST_COW, Span::default()),
        vec![Operand::Move(Place::new(LIST))],
        SPARE_LIST,
        4,
    ));
    let mut back = BasicBlockData::new(Some(goto(LOOP_HEADER)));
    back.statements.push(assign(
        Place::new(LIST),
        Rvalue::Use(Operand::Move(Place::new(SPARE_LIST))),
    ));
    body.basic_blocks.push(back);
    assert!(records_of(body).is_empty());
}

/// The type `kind` spelled as a type argument.
fn type_expression(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
        span: Span::default(),
    }
}

/// The local of `body` the program names `name`.
fn named_local(body: &Body, name: &str) -> Local {
    let index = (body.local_decls.iter())
        .position(|decl| decl.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no local named {name}:\n{body}"));
    Local(index)
}

#[test]
fn test_a_loop_writing_another_list_on_some_iterations_records_the_list_it_reads() {
    let body = body_named(COPY_ON_WRITE_OF_ANOTHER_LIST, ".mix");
    let read = named_local(&body, "a");
    let written = named_local(&body, "b");
    let recorded: Vec<Local> = body.stable_collections.iter().map(|r| r.local).collect();
    assert!(recorded.contains(&read), "{body}");
    assert!(!recorded.contains(&written), "{body}");
}

#[test]
fn test_a_loop_writing_an_alias_of_the_list_it_reads_records_the_list() {
    let body = body_named(COPY_ON_WRITE_OF_AN_ALIAS, ".mix");
    let read = named_local(&body, "a");
    let written = named_local(&body, "b");
    let recorded: Vec<Local> = body.stable_collections.iter().map(|r| r.local).collect();
    assert!(recorded.contains(&read), "{body}");
    assert!(!recorded.contains(&written), "{body}");
}

#[test]
fn test_a_register_machine_records_its_program_but_not_its_registers() {
    let body = body_named(REGISTER_MACHINE, ".run");
    let code = named_local(&body, "code");
    let regs = named_local(&body, "regs");
    let recorded: Vec<Local> = body.stable_collections.iter().map(|r| r.local).collect();
    assert!(recorded.contains(&code), "{body}");
    assert!(!recorded.contains(&regs), "{body}");
}
