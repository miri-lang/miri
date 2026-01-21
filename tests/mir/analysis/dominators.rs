// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::syntax::Span;
use miri::mir::analysis::dominators::DominatorTree;
use miri::mir::{BasicBlock, BasicBlockData, Body, ExecutionModel, Terminator, TerminatorKind};

fn create_empty_block() -> BasicBlockData {
    BasicBlockData::new(None)
}

fn create_goto(target: BasicBlock) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto { target },
        span: Span::default(),
    }
}

fn create_return() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    }
}

fn create_branch(target: BasicBlock, otherwise: BasicBlock) -> Terminator {
    Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: miri::mir::Operand::Constant(Box::new(miri::mir::Constant {
                span: Span::default(),
                ty: miri::ast::types::Type::new(
                    miri::ast::types::TypeKind::Boolean,
                    Span::default(),
                ),
                literal: miri::ast::literal::Literal::Boolean(true),
            })),
            targets: vec![(miri::mir::Discriminant::bool_true(), target)],
            otherwise,
        },
        span: Span::default(),
    }
}

#[test]
fn test_linear_flow_dominators() {
    // 0 -> 1 -> 2 -> 3
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);

    // Create 4 blocks
    for _ in 0..4 {
        body.basic_blocks.push(create_empty_block());
    }

    body.basic_blocks[0].terminator = Some(create_goto(BasicBlock(1)));
    body.basic_blocks[1].terminator = Some(create_goto(BasicBlock(2)));
    body.basic_blocks[2].terminator = Some(create_goto(BasicBlock(3)));
    body.basic_blocks[3].terminator = Some(create_return());

    let dom_tree = DominatorTree::compute(&body);

    // Check immediate dominators
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(1)),
        Some(&BasicBlock(0))
    );
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(2)),
        Some(&BasicBlock(1))
    );
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(3)),
        Some(&BasicBlock(2))
    );

    // Check dominance
    assert!(dom_tree.dominates(BasicBlock(0), BasicBlock(3)));
    assert!(dom_tree.dominates(BasicBlock(1), BasicBlock(3)));
    assert!(!dom_tree.dominates(BasicBlock(3), BasicBlock(1)));
}

#[test]
fn test_if_else_dominators() {
    //      0
    //     / \
    //    1   2
    //     \ /
    //      3
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);

    for _ in 0..4 {
        body.basic_blocks.push(create_empty_block());
    }

    body.basic_blocks[0].terminator = Some(create_branch(BasicBlock(1), BasicBlock(2)));
    body.basic_blocks[1].terminator = Some(create_goto(BasicBlock(3)));
    body.basic_blocks[2].terminator = Some(create_goto(BasicBlock(3)));
    body.basic_blocks[3].terminator = Some(create_return());

    let dom_tree = DominatorTree::compute(&body);

    // 0 dominates everything
    assert!(dom_tree.dominates(BasicBlock(0), BasicBlock(1)));
    assert!(dom_tree.dominates(BasicBlock(0), BasicBlock(2)));
    assert!(dom_tree.dominates(BasicBlock(0), BasicBlock(3)));

    // 1 does NOT dominate 3 (could go via 2)
    assert!(!dom_tree.dominates(BasicBlock(1), BasicBlock(3)));

    // 2 does NOT dominate 3 (could go via 1)
    assert!(!dom_tree.dominates(BasicBlock(2), BasicBlock(3)));

    // IDOMs
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(3)),
        Some(&BasicBlock(0))
    );
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(1)),
        Some(&BasicBlock(0))
    );
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(2)),
        Some(&BasicBlock(0))
    );
}

#[test]
fn test_loop_dominators() {
    //      0
    //      ↓
    //  --> 1 (header)<--
    //  |   ↓           |
    //  |   2 (body) ----
    //  |
    //  --> 3 (exit)

    // Note in my implementation while headers have 2 successors: body and exit.
    // Body goes back to header.

    // 0 -> 1
    // 1 -> 2, 3 (branch)
    // 2 -> 1
    // 3 -> return

    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);

    for _ in 0..4 {
        body.basic_blocks.push(create_empty_block());
    }

    body.basic_blocks[0].terminator = Some(create_goto(BasicBlock(1)));
    body.basic_blocks[1].terminator = Some(create_branch(BasicBlock(2), BasicBlock(3)));
    body.basic_blocks[2].terminator = Some(create_goto(BasicBlock(1)));
    body.basic_blocks[3].terminator = Some(create_return());

    let dom_tree = DominatorTree::compute(&body);

    // 0 dominates 1
    assert!(dom_tree.dominates(BasicBlock(0), BasicBlock(1)));

    // 1 dominates 2 and 3
    assert!(dom_tree.dominates(BasicBlock(1), BasicBlock(2)));
    assert!(dom_tree.dominates(BasicBlock(1), BasicBlock(3)));

    // 2 does not dominate 1 (backward edge)
    assert!(!dom_tree.dominates(BasicBlock(2), BasicBlock(1)));

    // IDOMs
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(1)),
        Some(&BasicBlock(0))
    );
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(2)),
        Some(&BasicBlock(1))
    );
    assert_eq!(
        dom_tree.immediate_dominators.get(&BasicBlock(3)),
        Some(&BasicBlock(1))
    );
}

#[test]
fn test_dominance_frontier() {
    // Same as if-else
    //      0
    //     / \
    //    1   2
    //     \ /
    //      3

    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);

    for _ in 0..4 {
        body.basic_blocks.push(create_empty_block());
    }

    body.basic_blocks[0].terminator = Some(create_branch(BasicBlock(1), BasicBlock(2)));
    body.basic_blocks[1].terminator = Some(create_goto(BasicBlock(3)));
    body.basic_blocks[2].terminator = Some(create_goto(BasicBlock(3)));
    body.basic_blocks[3].terminator = Some(create_return());

    let dom_tree = DominatorTree::compute(&body);

    // DF(1) = {3} because 1 dominates predecessor of 3 (which is 1 itself), but 1 does not strictly dominate 3
    let df1 = dom_tree.dominance_frontiers.get(&BasicBlock(1)).unwrap();
    assert!(df1.contains(&BasicBlock(3)));
    assert_eq!(df1.len(), 1);

    // DF(2) = {3}
    let df2 = dom_tree.dominance_frontiers.get(&BasicBlock(2)).unwrap();
    assert!(df2.contains(&BasicBlock(3)));
    assert_eq!(df2.len(), 1);

    // DF(3) = {}
    let df3 = dom_tree.dominance_frontiers.get(&BasicBlock(3)).unwrap();
    assert!(df3.is_empty());

    // DF(0) = {} (dominates everything strictly or is start)
    // Actually 0 dominates 3, so it doesn't meet "does not strictly dominate" condition for any reachable node?
    // 0 dominates 1 (pred of 3) and strictly dominates 3.
    // 0 -> 1 -> 3
    // 0 -> 2 -> 3
    let df0 = dom_tree.dominance_frontiers.get(&BasicBlock(0)).unwrap();
    assert!(df0.is_empty());
}
