// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::optimization::simplify_cfg::SimplifyCfg;
use miri::mir::optimization::OptimizationPass;
use miri::mir::{
    BasicBlock, BasicBlockData, Body, ExecutionModel, LocalDecl, Terminator, TerminatorKind,
};

fn create_test_body() -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::Void, Span::default()),
        Span::default(),
    )); // _0
    body
}

#[test]
fn test_remove_unreachable() {
    let mut body = create_test_body();

    let mut bb0 = BasicBlockData::new(None);
    bb0.terminator = Some(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlock(1),
        },
        span: Span::default(),
    });
    body.basic_blocks.push(bb0); // bb0

    let mut bb1 = BasicBlockData::new(None);
    bb1.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });
    body.basic_blocks.push(bb1); // bb1

    let mut bb2 = BasicBlockData::new(None); // Unreachable
    bb2.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });
    body.basic_blocks.push(bb2); // bb2

    let mut pass = SimplifyCfg;
    let changed = pass.run(&mut body);

    assert!(changed);
    assert_eq!(body.basic_blocks.len(), 2);
}

#[test]
fn test_thread_jumps() {
    let mut body = create_test_body();

    // bb0 -> bb1
    let mut bb0 = BasicBlockData::new(None);
    bb0.terminator = Some(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlock(1),
        },
        span: Span::default(),
    });
    body.basic_blocks.push(bb0); // bb0

    // bb1 -> bb2 (Empty block, pure forwarder)
    let mut bb1 = BasicBlockData::new(None);
    bb1.terminator = Some(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlock(2),
        },
        span: Span::default(),
    });
    body.basic_blocks.push(bb1); // bb1

    // bb2 -> Return
    let mut bb2 = BasicBlockData::new(None);
    bb2.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });
    body.basic_blocks.push(bb2); // bb2

    let mut pass = SimplifyCfg;
    let changed = pass.run(&mut body);

    assert!(changed);

    // bb1 is skipped: bb0 -> bb2.
    // BUT bb1 becomes unreachable.
    // So bb1 is removed.
    // bb2 (index 2) moves to index 1.
    // bb0 terminator points to new index 1.

    if let TerminatorKind::Goto { target } = &body.basic_blocks[0].terminator.as_ref().unwrap().kind
    {
        assert_eq!(*target, BasicBlock(1));
    } else {
        panic!("Expected Goto");
    }

    assert_eq!(body.basic_blocks.len(), 2);
}
