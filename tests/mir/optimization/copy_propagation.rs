// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::optimization::copy_propagation::CopyPropagation;
use miri::mir::optimization::OptimizationPass;
use miri::mir::{
    BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
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
fn test_copy_propagation_basic() {
    let mut body = create_test_body();
    // _1
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));
    // _2
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));
    // _3
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));

    let mut bb0 = BasicBlockData::new(None);

    // _1 = Copy(_100)
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Use(Operand::Copy(Place::new(Local(100)))),
        ),
        span: Span::default(),
    });

    // _2 = _1 (Copy) -> Should become _2 = Copy(_100)
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(2)),
            Rvalue::Use(Operand::Copy(Place::new(Local(1)))),
        ),
        span: Span::default(),
    });

    // _3 = _2 (Copy) -> Should become _3 = Copy(_100)
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(3)),
            Rvalue::Use(Operand::Copy(Place::new(Local(2)))),
        ),
        span: Span::default(),
    });

    bb0.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });

    body.basic_blocks.push(bb0);

    let mut pass = CopyPropagation;
    let changed = pass.run(&mut body);

    assert!(changed);

    // _3 = _2 should become _3 = _100 (transitive)
    let bb0 = &body.basic_blocks[0];
    let stmt2 = &bb0.statements[2]; // _3 = ...
    if let StatementKind::Assign(_, Rvalue::Use(Operand::Copy(place))) = &stmt2.kind {
        assert_eq!(place.local, Local(100));
    } else {
        panic!("Expected Assign Use Copy");
    }
}
