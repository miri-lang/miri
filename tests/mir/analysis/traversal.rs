// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::syntax::Span;
use miri::mir::analysis::traversal::reverse_postorder;
use miri::mir::{
    BasicBlock, BasicBlockData, Body, Discriminant, ExecutionModel, Local, Operand, Place,
    Terminator, TerminatorKind,
};

fn body_of(terminators: Vec<TerminatorKind>) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.basic_blocks = terminators
        .into_iter()
        .map(|kind| BasicBlockData::new(Some(Terminator::new(kind, Span::default()))))
        .collect();
    body
}

fn goto(target: usize) -> TerminatorKind {
    TerminatorKind::Goto {
        target: BasicBlock(target),
    }
}

fn branch(zero: usize, otherwise: usize) -> TerminatorKind {
    TerminatorKind::SwitchInt {
        discr: Operand::Copy(Place::new(Local(0))),
        targets: vec![(Discriminant::from(0), BasicBlock(zero))],
        otherwise: BasicBlock(otherwise),
    }
}

fn position(order: &[usize], block: usize) -> usize {
    order
        .iter()
        .position(|candidate| *candidate == block)
        .unwrap_or_else(|| panic!("bb{block} missing from {order:?}"))
}

#[test]
fn test_reverse_postorder_puts_each_block_before_its_successors() {
    // bb0 branches to bb1 and bb2, which meet in bb3.
    let body = body_of(vec![branch(1, 2), goto(3), goto(3), TerminatorKind::Return]);
    let order = reverse_postorder(&body);
    assert_eq!(order.first(), Some(&0));
    assert_eq!(order.len(), 4);
    for arm in [1, 2] {
        assert!(position(&order, 0) < position(&order, arm));
        assert!(position(&order, arm) < position(&order, 3));
    }
}

#[test]
fn test_reverse_postorder_leaves_out_unreachable_blocks_and_ends_at_back_edges() {
    // bb1 loops back to itself and to bb0; bb3 is never reached.
    let body = body_of(vec![goto(1), branch(0, 2), TerminatorKind::Return, goto(2)]);
    assert_eq!(reverse_postorder(&body), vec![0, 1, 2]);
}

#[test]
fn test_reverse_postorder_of_an_empty_body_is_empty() {
    assert!(reverse_postorder(&body_of(Vec::new())).is_empty());
}
