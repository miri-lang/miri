// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a local a `move` handed on is neither read nor
//! released again until something re-binds it.
//!
//! Each broken fixture is the defect the rule exists for, built by hand; each
//! accepted one is the shape the passes producing a `move` actually leave, so
//! the rule is seen telling the two apart rather than reporting nothing.

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::symbol::Symbol;
use miri::mir::verify::{verify_moved_out, VerificationViolation};
use miri::mir::{
    Body, Constant, Discriminant, EndCause, ExecutionModel, FlagOf, Local, LocalDecl, Operand,
    Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::ModuleId;

fn span() -> Span {
    Span::new(0, 0)
}

fn string_ty() -> Type {
    Type::new(TypeKind::String, span())
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn stmt(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

fn constant(ty: Type, literal: Literal) -> Operand {
    Operand::Constant(Box::new(Constant {
        ty,
        literal,
        span: span(),
    }))
}

fn assign_fresh(local: usize) -> Statement {
    let fresh = constant(string_ty(), Literal::String("x".to_string()));
    stmt(StatementKind::Assign(place(local), Rvalue::Use(fresh)))
}

fn assign(dest: usize, operand: Operand) -> Statement {
    stmt(StatementKind::Assign(place(dest), Rvalue::Use(operand)))
}

fn moved(local: usize) -> Operand {
    Operand::Move(place(local))
}

fn copied(local: usize) -> Operand {
    Operand::Copy(place(local))
}

fn decref(local: usize) -> Statement {
    stmt(StatementKind::DecRef(place(local)))
}

fn storage_dead(local: usize) -> Statement {
    stmt(StatementKind::storage_dead(place(local)))
}

fn consumed_end(local: usize) -> Statement {
    stmt(StatementKind::StorageDead(place(local), EndCause::Consumed))
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

fn goto(target: usize) -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(target),
    }
}

fn branch(then_block: usize, else_block: usize) -> TerminatorKind {
    TerminatorKind::SwitchInt {
        discr: constant(Type::new(TypeKind::Boolean, span()), Literal::Boolean(true)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(then_block))],
        otherwise: BasicBlock(else_block),
    }
}

/// A call to a program function handing back a string into `destination`.
fn call(args: Vec<Operand>, destination: usize, target: usize) -> TerminatorKind {
    let mut func = Operand::function(
        Symbol::declared_function(&ModuleId::Program, "callee"),
        span(),
    );
    if let Operand::Function(function) = &mut func {
        function.ty = string_ty();
    }
    TerminatorKind::Call {
        func,
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        flags: miri::mir::CallFlags::default(),
        destination: place(destination),
        target: Some(BasicBlock(target)),
    }
}

/// A body of `blocks` whose local 0 is the void return slot and locals 1 to
/// 3 are strings.
fn body_of(blocks: Vec<BasicBlockData>, model: ExecutionModel) -> Body {
    let mut body = Body::new(0, span(), model);
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span()), span()));
    for _ in 1..=3 {
        body.new_local(LocalDecl::new(string_ty(), span()));
    }
    body.basic_blocks.extend(blocks);
    body
}

fn host_body(blocks: Vec<BasicBlockData>) -> Body {
    body_of(blocks, ExecutionModel::Cpu)
}

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// `body` raises exactly one finding, against `_1`, carrying every fragment.
fn assert_one_finding(body: &Body, fragments: &[&str]) {
    let violations = verify_moved_out(body);
    let text = messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    assert_eq!(violations[0].local, Local(1), "{text}");
    for fragment in fragments {
        assert!(text.contains(fragment), "expected `{fragment}` in: {text}");
    }
}

fn assert_clean(body: &Body) {
    let violations = verify_moved_out(body);
    assert!(violations.is_empty(), "{}", messages(&violations));
}

#[test]
fn a_release_after_a_move_is_flagged() {
    let body = host_body(vec![block(
        vec![assign_fresh(1), assign(2, moved(1)), decref(1), decref(2)],
        TerminatorKind::Return,
    )]);
    assert_one_finding(&body, &["bb0[1]", "bb0[2]", "released"]);
}

#[test]
fn a_release_after_a_join_one_of_whose_paths_moved_is_flagged() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], branch(1, 2)),
        block(vec![assign(2, moved(1)), decref(2)], goto(3)),
        block(Vec::new(), goto(3)),
        block(vec![decref(1)], TerminatorKind::Return),
    ]);
    assert_one_finding(&body, &["bb1[0]", "bb3[0]"]);
}

#[test]
fn a_release_after_the_moved_local_is_re_bound_is_accepted() {
    let body = host_body(vec![block(
        vec![
            assign_fresh(1),
            assign(2, moved(1)),
            assign_fresh(1),
            decref(1),
            decref(2),
        ],
        TerminatorKind::Return,
    )]);
    assert_clean(&body);
}

#[test]
fn a_read_after_a_call_took_the_local_by_move_is_flagged() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], call(vec![moved(1)], 3, 1)),
        block(
            vec![assign(2, copied(1)), decref(3)],
            TerminatorKind::Return,
        ),
    ]);
    assert_one_finding(&body, &["bb0 terminator", "bb1[0]", "read"]);
}

/// One reference handed on twice by one call: each parameter would own it.
#[test]
fn a_local_moved_twice_in_one_call_is_flagged() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], call(vec![moved(1), moved(1)], 3, 1)),
        block(vec![decref(3)], TerminatorKind::Return),
    ]);
    assert_one_finding(&body, &["bb0 terminator", "moved again"]);
}

/// The same call reading the local once by copy and once by move hands its
/// reference on once.
#[test]
fn a_local_moved_once_beside_a_copy_in_one_call_is_accepted() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], call(vec![copied(1), moved(1)], 3, 1)),
        block(vec![decref(3)], TerminatorKind::Return),
    ]);
    assert_clean(&body);
}

#[test]
fn a_copy_on_write_handing_the_value_back_is_accepted() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], call(vec![moved(1)], 3, 1)),
        block(vec![assign(1, moved(3)), decref(1)], TerminatorKind::Return),
    ]);
    assert_clean(&body);
}

#[test]
fn a_second_read_before_the_copy_on_write_hands_the_value_back_is_flagged() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], call(vec![moved(1)], 3, 1)),
        block(
            vec![assign(2, copied(1)), assign(1, moved(3)), decref(1)],
            TerminatorKind::Return,
        ),
    ]);
    assert_one_finding(&body, &["bb0 terminator", "bb1[0]"]);
}

#[test]
fn a_call_writing_its_result_into_the_moved_local_re_binds_it() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], call(vec![moved(1)], 1, 1)),
        block(vec![decref(1)], TerminatorKind::Return),
    ]);
    assert_clean(&body);
}

#[test]
fn the_end_of_a_moved_local_s_storage_is_not_a_release() {
    let body = host_body(vec![block(
        vec![
            assign_fresh(1),
            assign(2, moved(1)),
            storage_dead(1),
            decref(2),
        ],
        TerminatorKind::Return,
    )]);
    assert_clean(&body);
}

#[test]
fn a_move_on_one_turn_of_a_loop_is_seen_by_the_next_turn() {
    let body = host_body(vec![
        block(vec![assign_fresh(1)], goto(1)),
        block(vec![assign(2, copied(1))], branch(2, 3)),
        block(vec![assign(3, moved(1)), decref(3)], goto(1)),
        block(vec![decref(1)], TerminatorKind::Return),
    ]);
    let violations = verify_moved_out(&body);
    let text = messages(&violations);
    assert!(text.contains("bb2[0]") && text.contains("bb1[0]"), "{text}");
}

#[test]
fn a_body_running_on_a_gpu_is_not_checked() {
    let body = body_of(
        vec![block(
            vec![assign_fresh(1), assign(2, moved(1)), decref(1)],
            TerminatorKind::Return,
        )],
        ExecutionModel::GpuKernel,
    );
    assert_clean(&body);
}

#[test]
fn a_move_of_a_projection_is_flagged() {
    let mut projected = place(1);
    projected.projection.push(miri::mir::PlaceElem::Field(0));
    let body = host_body(vec![block(
        vec![assign_fresh(1), assign(2, Operand::Move(projected))],
        TerminatorKind::Return,
    )]);
    assert_one_finding(&body, &["through a projection", "bb0[1]"]);
}

/// A drop-flagged end: `_1` is moved on one arm and its storage ends there,
/// clearing its flag, `_4`, which is set where `_1` is bound. The release
/// behind the set flag is reached only by the path that did not move it,
/// unless `clears_where_moved` is false.
fn flag_guarded_release(flag_marks_one: bool, clears_where_moved: bool) -> Body {
    let flag = 4;
    let set = |value: bool| {
        let operand = constant(
            Type::new(TypeKind::Boolean, span()),
            Literal::Boolean(value),
        );
        assign(flag, operand)
    };
    let mut body = host_body(vec![
        block(vec![assign_fresh(1), set(true)], branch(1, 2)),
        block(
            [assign(2, moved(1)), consumed_end(1)]
                .into_iter()
                .chain(clears_where_moved.then(|| set(false)))
                .collect(),
            goto(3),
        ),
        block(vec![], goto(3)),
        block(
            vec![],
            TerminatorKind::SwitchInt {
                discr: copied(flag),
                targets: vec![(Discriminant::bool_false(), BasicBlock(5))],
                otherwise: BasicBlock(4),
            },
        ),
        block(vec![decref(1), storage_dead(1)], goto(5)),
        block(vec![], TerminatorKind::Return),
    ]);
    let mut decl = LocalDecl::new(Type::new(TypeKind::Boolean, span()), span());
    if flag_marks_one {
        decl.flag_of = Some(FlagOf::Drop(Local(1)));
    }
    body.new_local(decl);
    body
}

#[test]
fn a_release_behind_the_set_drop_flag_of_a_moved_local_is_accepted() {
    assert_clean(&flag_guarded_release(true, true));
}

#[test]
fn a_release_behind_a_flag_that_guards_nothing_is_still_flagged() {
    assert_one_finding(&flag_guarded_release(false, true), &["released", "bb4[0]"]);
}

#[test]
fn a_release_behind_a_flag_never_cleared_where_the_local_was_moved_is_flagged() {
    assert_one_finding(&flag_guarded_release(true, false), &["released", "bb4[0]"]);
}
