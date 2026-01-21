// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::optimization::constant_propagation::ConstantPropagation;
use miri::mir::optimization::OptimizationPass;
use miri::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Constant, ExecutionModel, Local, LocalDecl, Operand,
    Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
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
fn test_const_fold_binary() {
    let mut body = create_test_body();
    // _1 = 10
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));
    // _2 = 20
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));
    // _3 = _1 + _2
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

    // _2 = 20
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(2)),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: Span::default(),
                ty: Type::new(TypeKind::I64, Span::default()),
                literal: Literal::Integer(IntegerLiteral::I64(20)),
            }))),
        ),
        span: Span::default(),
    });

    // _3 = _1 + _2
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(3)),
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(Operand::Copy(Place::new(Local(1)))),
                Box::new(Operand::Copy(Place::new(Local(2)))),
            ),
        ),
        span: Span::default(),
    });

    bb0.terminator = Some(Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    });

    body.basic_blocks.push(bb0);

    let mut pass = ConstantPropagation;
    let changed = pass.run(&mut body);

    assert!(changed);

    // Check if _3 = 30
    let bb0 = &body.basic_blocks[0];
    let stmt2 = &bb0.statements[2];
    if let StatementKind::Assign(_, Rvalue::Use(Operand::Constant(c))) = &stmt2.kind {
        if let Literal::Integer(IntegerLiteral::I64(val)) = c.literal {
            assert_eq!(val, 30);
        } else {
            panic!("Expected I64 literal");
        }
    } else {
        panic!("Expected constant assignment, got {:?}", stmt2.kind);
    }
}

#[test]
fn test_const_fold_branch() {
    let mut body = create_test_body();
    // _1 = 1
    body.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::I64, Span::default()),
        Span::default(),
    ));

    let mut bb0 = BasicBlockData::new(None);
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: Span::default(),
                ty: Type::new(TypeKind::I64, Span::default()),
                literal: Literal::Integer(IntegerLiteral::I64(1)),
            }))),
        ),
        span: Span::default(),
    });

    // SwitchInt(_1) -> 0: bb1, otherwise: bb2
    bb0.terminator = Some(Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(Local(1))),
            targets: vec![(miri::mir::Discriminant::bool_false(), BasicBlock(1))],
            otherwise: BasicBlock(2),
        },
        span: Span::default(),
    });

    body.basic_blocks.push(bb0);
    body.basic_blocks.push(BasicBlockData::new(None)); // bb1
    body.basic_blocks.push(BasicBlockData::new(None)); // bb2

    let mut pass = ConstantPropagation;
    let changed = pass.run(&mut body);

    assert!(changed);

    let bb0 = &body.basic_blocks[0];
    if let Some(term) = &bb0.terminator {
        if let TerminatorKind::Goto { target } = term.kind {
            assert_eq!(target, BasicBlock(2));
        } else {
            panic!("Expected Goto");
        }
    }
}
