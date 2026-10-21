// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::collections::BTreeSet;

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::analysis::dominators::DominatorTree;
use miri::mir::analysis::loops::{natural_loops, predecessors};
use miri::mir::{
    BasicBlock, BasicBlockData, Body, Constant, Discriminant, ExecutionModel, Operand, Terminator,
    TerminatorKind,
};

fn goto(target: usize) -> Terminator {
    Terminator::new(
        TerminatorKind::Goto {
            target: BasicBlock(target),
        },
        Span::default(),
    )
}

fn branch(taken: usize, otherwise: usize) -> Terminator {
    let discr = Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: Type::new(TypeKind::Boolean, Span::default()),
        literal: Literal::Boolean(true),
    }));
    Terminator::new(
        TerminatorKind::SwitchInt {
            discr,
            targets: vec![(Discriminant::bool_true(), BasicBlock(taken))],
            otherwise: BasicBlock(otherwise),
        },
        Span::default(),
    )
}

fn ret() -> Terminator {
    Terminator::new(TerminatorKind::Return, Span::default())
}

/// A body whose blocks end in `terminators`, in order.
fn body_of(terminators: Vec<Terminator>) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.basic_blocks = terminators
        .into_iter()
        .map(|terminator| BasicBlockData::new(Some(terminator)))
        .collect();
    body
}

#[test]
fn test_a_nested_loop_is_found_inside_the_loop_around_it() {
    // 0 -> 1 (outer header) -> 2 (inner header) <-> 3; 2 -> 4 -> 1; 1 -> 5
    let body = body_of(vec![
        goto(1),
        branch(2, 5),
        branch(3, 4),
        goto(2),
        goto(1),
        ret(),
    ]);
    let loops = natural_loops(&body, &DominatorTree::compute(&body));
    assert_eq!(loops.len(), 2);
    let outer = loops.iter().find(|found| found.header == 1).unwrap();
    let inner = loops.iter().find(|found| found.header == 2).unwrap();
    assert_eq!(outer.blocks, BTreeSet::from([1, 2, 3, 4]));
    assert_eq!(outer.latches, vec![4]);
    assert_eq!(inner.blocks, BTreeSet::from([2, 3]));
    assert_eq!(inner.latches, vec![3]);
    let predecessors = predecessors(&body);
    assert_eq!(outer.entries(&predecessors), vec![0]);
    assert_eq!(inner.entries(&predecessors), vec![1]);
}

#[test]
fn test_a_body_without_a_back_edge_has_no_loop() {
    let body = body_of(vec![branch(1, 2), goto(2), ret()]);
    assert!(natural_loops(&body, &DominatorTree::compute(&body)).is_empty());
}
