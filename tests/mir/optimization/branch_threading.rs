// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that lets a block branch on the condition it computed
//! rather than jump to a join block that branches on a copy of it. Each body
//! is the shape `a and b` lowers to:
//!
//! ```text
//! bb0: a = x < y; switchInt(a) -> [1: bb1, otherwise: bb2]
//! bb1: t = x < z; goto bb3
//! bb2: t = false; goto bb3
//! bb3: s = t; switchInt(s) -> [1: bb4, otherwise: bb5]
//! ```

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::optimization::branch_threading::thread_branch_conditions;
use miri::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Constant, Discriminant, ExecutionModel, Local,
    LocalDecl, Operand, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};

const X: Local = Local(1);
const Y: Local = Local(2);
const Z: Local = Local(3);
const A: Local = Local(4);
const T: Local = Local(5);
const S: Local = Local(6);
const JOIN: usize = 3;
const WHEN_TRUE: usize = 4;
const WHEN_FALSE: usize = 5;

fn assign(local: Local, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(Place::new(local), rvalue),
        span: Span::default(),
    }
}

fn copy(local: Local) -> Rvalue {
    Rvalue::Use(Operand::Copy(Place::new(local)))
}

fn less(left: Local, right: Local) -> Rvalue {
    Rvalue::BinaryOp(
        BinOp::Lt,
        Box::new(Operand::Copy(Place::new(left))),
        Box::new(Operand::Copy(Place::new(right))),
    )
}

fn boolean(value: bool) -> Rvalue {
    Rvalue::Use(Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::Boolean, Span::default()),
        literal: Literal::Boolean(value),
    })))
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    let mut data = BasicBlockData::new(Some(Terminator::new(kind, Span::default())));
    data.statements = statements;
    data
}

fn goto(target: usize) -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(target),
    }
}

fn branch_on(local: Local, when_true: usize, otherwise: usize) -> TerminatorKind {
    TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(local)),
        targets: vec![(Discriminant::bool_true(), BasicBlock(when_true))],
        otherwise: BasicBlock(otherwise),
    }
}

/// The body `a and b` lowers to, with `s` declared as `join_copy_type`.
fn and_chain(join_copy_type: TypeKind) -> Body {
    let mut body = Body::new(3, Span::default(), ExecutionModel::Cpu);
    body.local_decls = [
        TypeKind::Int,
        TypeKind::Int,
        TypeKind::Int,
        TypeKind::Int,
        TypeKind::Boolean,
        TypeKind::Boolean,
        join_copy_type,
    ]
    .into_iter()
    .map(|kind| LocalDecl::new(Type::new(kind, Span::default()), Span::default()))
    .collect();
    body.basic_blocks = vec![
        block(vec![assign(A, less(X, Y))], branch_on(A, 1, 2)),
        block(vec![assign(T, less(X, Z))], goto(JOIN)),
        block(vec![assign(T, boolean(false))], goto(JOIN)),
        block(
            vec![assign(S, copy(T))],
            branch_on(S, WHEN_TRUE, WHEN_FALSE),
        ),
        block(vec![], TerminatorKind::Return),
        block(vec![], TerminatorKind::Return),
    ];
    body
}

fn terminator_of(body: &Body, index: usize) -> &TerminatorKind {
    &body.basic_blocks[index]
        .terminator
        .as_ref()
        .expect("every block is terminated")
        .kind
}

#[test]
fn test_a_constant_condition_jumps_straight_to_its_target() {
    let mut body = and_chain(TypeKind::Boolean);
    assert!(thread_branch_conditions(&mut body));
    assert_eq!(terminator_of(&body, 2), &goto(WHEN_FALSE));
}

#[test]
fn test_a_computed_condition_branches_where_it_is_computed() {
    let mut body = and_chain(TypeKind::Boolean);
    thread_branch_conditions(&mut body);
    assert_eq!(
        terminator_of(&body, 1),
        &branch_on(S, WHEN_TRUE, WHEN_FALSE)
    );
    assert_eq!(
        body.basic_blocks[1].statements,
        vec![assign(T, less(X, Z)), assign(S, copy(T))],
        "the join block's copy runs in the predecessor, so later reads of s see it"
    );
}

#[test]
fn test_a_join_block_that_copies_anything_but_booleans_is_left_alone() {
    let mut body = and_chain(TypeKind::Int);
    let before = body.basic_blocks.clone();
    assert!(!thread_branch_conditions(&mut body));
    assert_eq!(body.basic_blocks, before);
}

#[test]
fn test_a_block_that_jumps_to_itself_is_left_alone() {
    let mut body = and_chain(TypeKind::Boolean);
    body.basic_blocks[JOIN] = block(vec![assign(S, copy(T))], goto(JOIN));
    let before = body.basic_blocks.clone();
    assert!(!thread_branch_conditions(&mut body));
    assert_eq!(body.basic_blocks, before);
}

#[test]
fn test_a_constant_later_overwritten_by_a_comparison_is_not_folded() {
    let mut body = and_chain(TypeKind::Boolean);
    body.basic_blocks[2].statements.push(assign(T, less(X, Z)));
    thread_branch_conditions(&mut body);
    assert_eq!(
        terminator_of(&body, 2),
        &branch_on(S, WHEN_TRUE, WHEN_FALSE)
    );
}

#[test]
fn test_a_true_constant_jumps_to_the_true_target() {
    let mut body = and_chain(TypeKind::Boolean);
    body.basic_blocks[2].statements = vec![assign(T, boolean(true))];
    thread_branch_conditions(&mut body);
    assert_eq!(terminator_of(&body, 2), &goto(WHEN_TRUE));
}
