// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::ssa::construction::construct_ssa;
use miri::mir::{
    BasicBlock, BasicBlockData, Body, ExecutionModel, LocalDecl, Terminator, TerminatorKind,
};
use miri::mir::{Constant, Operand, Place, Rvalue, Statement, StatementKind};

fn create_body() -> Body {
    Body::new(0, Span::default(), ExecutionModel::Cpu)
}

fn create_local(body: &mut Body, ty: Type) -> miri::mir::Local {
    let decl = LocalDecl::new(ty, Span::default());
    let idx = body.local_decls.len();
    body.local_decls.push(decl);
    miri::mir::Local(idx)
}

fn create_assign_int(block: &mut BasicBlockData, local: miri::mir::Local, val: i64) {
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span: Span::default(),
                ty: Type::new(TypeKind::I64, Span::default()),
                literal: Literal::Integer(IntegerLiteral::I64(val)),
            }))),
        ),
        span: Span::default(),
    };
    block.statements.push(stmt);
}

fn create_assign_copy(block: &mut BasicBlockData, dest: miri::mir::Local, src: miri::mir::Local) {
    let stmt = Statement {
        kind: StatementKind::Assign(
            Place::new(dest),
            Rvalue::Use(Operand::Copy(Place::new(src))),
        ),
        span: Span::default(),
    };
    block.statements.push(stmt);
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
            discr: Operand::Constant(Box::new(Constant {
                span: Span::default(),
                ty: Type::new(TypeKind::Boolean, Span::default()),
                literal: Literal::Boolean(true),
            })),
            targets: vec![(miri::mir::Discriminant::bool_true(), target)],
            otherwise,
        },
        span: Span::default(),
    }
}

#[test]
fn test_ssa_linear() {
    let mut body = create_body();
    // locals: 0=return, 1=x, 2=y
    let _ret = create_local(&mut body, Type::new(TypeKind::I64, Span::default())); // Local(0) is return place usually? No, Local(0) is return. Body::new creates?
                                                                                   // Body::new(arg_count) creates args. Locals are separate.
                                                                                   // Actually Body::new creates empty local_decls? No, it initializes args.
                                                                                   // Let's assume typical: Local(0) is return pointer/value. Args are 1..N.
                                                                                   // Our create_local appends.

    // Check Body::new.
    // Let's assume we just create locals and use them.
    let x = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let y = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));

    body.basic_blocks.push(BasicBlockData::new(None)); // bb0

    // x = 1
    create_assign_int(&mut body.basic_blocks[0], x, 1);
    // x = 2
    create_assign_int(&mut body.basic_blocks[0], x, 2);
    // y = x
    create_assign_copy(&mut body.basic_blocks[0], y, x);

    body.basic_blocks[0].terminator = Some(create_return());

    construct_ssa(&mut body);

    // Verify:
    // x should have different versions.
    // x = 1 -> x_a = 1
    // x = 2 -> x_b = 2
    // y = x -> y_c = x_b

    // Inspect statements
    let stmts = &body.basic_blocks[0].statements;
    assert_eq!(stmts.len(), 3);

    // 1st: x_new1 = 1
    let stmt1 = &stmts[0];
    let x_new1 = if let StatementKind::Assign(place, _) = &stmt1.kind {
        place.local
    } else {
        panic!()
    };
    assert_ne!(x_new1, x);

    // 2nd: x_new2 = 2
    let stmt2 = &stmts[1];
    let x_new2 = if let StatementKind::Assign(place, _) = &stmt2.kind {
        place.local
    } else {
        panic!()
    };
    assert_ne!(x_new2, x);
    assert_ne!(x_new2, x_new1);

    // 3rd: y_new = x_new2
    let stmt3 = &stmts[2];
    if let StatementKind::Assign(place, rvalue) = &stmt3.kind {
        assert_ne!(place.local, y); // y renamed
        if let Rvalue::Use(Operand::Copy(src)) = rvalue {
            assert_eq!(src.local, x_new2); // Should use latest version of x
        } else {
            panic!("Expected Use(Copy)");
        }
    } else {
        panic!("Expected Assign");
    }
}

#[test]
fn test_ssa_if_else_phi() {
    //      0 (x=1)
    //     / \
    // 1(x=2) 2(x=3)
    //     \ /
    //      3 (y=x) -> Phi needed for x

    let mut body = create_body();
    let x = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let y = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));

    for _ in 0..4 {
        body.basic_blocks.push(BasicBlockData::new(None));
    }

    // bb0: x = 1; goto 1 else 2
    create_assign_int(&mut body.basic_blocks[0], x, 1);
    body.basic_blocks[0].terminator = Some(create_branch(BasicBlock(1), BasicBlock(2)));

    // bb1: x = 2; goto 3
    create_assign_int(&mut body.basic_blocks[1], x, 2);
    body.basic_blocks[1].terminator = Some(create_goto(BasicBlock(3)));

    // bb2: x = 3; goto 3
    create_assign_int(&mut body.basic_blocks[2], x, 3);
    body.basic_blocks[2].terminator = Some(create_goto(BasicBlock(3)));

    // bb3: y = x; return
    create_assign_copy(&mut body.basic_blocks[3], y, x);
    body.basic_blocks[3].terminator = Some(create_return());

    construct_ssa(&mut body);

    // Check bb3. Should start with Phi for x.
    let bb3_stmts = &body.basic_blocks[3].statements;
    // We expect at least 2 statements: Phi and y=x.
    // Phi might be first.

    let phi_stmt = &bb3_stmts[0];
    if let StatementKind::Assign(place, Rvalue::Phi(args)) = &phi_stmt.kind {
        // This should be x's new version
        // verify args
        assert_eq!(args.len(), 2);
        // One from bb1, one from bb2
        // We can't easily check exact values without tracing, but structure is confirmed.

        let x_phi = place.local;

        // Next stmt should use x_phi
        let next_stmt = &bb3_stmts[1];
        if let StatementKind::Assign(_, Rvalue::Use(Operand::Copy(src))) = &next_stmt.kind {
            assert_eq!(src.local, x_phi);
        } else {
            panic!("Expected y=x assignment using phi result");
        }
    } else {
        panic!(
            "Expected Phi at start of block 3, found {:?}",
            phi_stmt.kind
        );
    }
}
