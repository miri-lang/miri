// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::optimization::dead_code::DeadCodeElimination;
use miri::mir::optimization::OptimizationPass;
use miri::mir::{
    BasicBlockData, Body, Constant, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
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
fn test_dead_code_elimination() {
    let mut body = create_test_body();
    // _1 = 10 (Unused)
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));

    let mut bb0 = BasicBlockData::new(None);

    // _1 = 10
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: Span::default(),
                ty: Type::new(TypeKind::I64, Span::default()),
                literal: Literal::Integer(IntegerLiteral::I64(10)),
            }))),
        ),
        span: Span::default(),
    });

    bb0.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });

    body.basic_blocks.push(bb0);

    let mut pass = DeadCodeElimination;
    let changed = pass.run(&mut body);

    assert!(changed);

    // Statement should be Nop
    let bb0 = &body.basic_blocks[0];
    assert_eq!(bb0.statements.len(), 1);
    if let StatementKind::Nop = bb0.statements[0].kind {
        // Correct
    } else {
        panic!("Expected Nop");
    }
}
