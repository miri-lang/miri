// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::ssa::destruction::deconstruct_ssa;
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

fn create_assign_phi(
    block: &mut BasicBlockData,
    dest: miri::mir::Local,
    args: Vec<(miri::mir::Local, BasicBlock)>,
) {
    let args_ops = args
        .into_iter()
        .map(|(l, b)| (Operand::Copy(Place::new(l)), b))
        .collect();
    let stmt = Statement {
        kind: StatementKind::Assign(Place::new(dest), Rvalue::Phi(args_ops)),
        span: Span::default(),
    };
    block.statements.insert(0, stmt); // Phis at start
}

fn create_goto(target: BasicBlock) -> Terminator {
    Terminator {
        kind: TerminatorKind::Goto { target },
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

fn create_return() -> Terminator {
    Terminator {
        kind: TerminatorKind::Return,
        span: Span::default(),
    }
}

#[test]
fn test_destruction_simple() {
    // bb0 -> bb1, bb2 -> bb3 (phi)
    // critical edges: bb0->bb1 (OK), bb0->bb2 (OK).
    // bb1->bb3 (OK if goto), bb2->bb3 (OK if goto).

    let mut body = create_body();
    let v1 = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let v2 = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let v3 = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));

    for _ in 0..4 {
        body.basic_blocks.push(BasicBlockData::new(None));
    }

    // bb0
    body.basic_blocks[0].terminator = Some(create_branch(BasicBlock(1), BasicBlock(2)));

    // bb1
    body.basic_blocks[1].terminator = Some(create_goto(BasicBlock(3)));

    // bb2
    body.basic_blocks[2].terminator = Some(create_goto(BasicBlock(3)));

    // bb3: v3 = phi(v1, bb1), (v2, bb2)
    create_assign_phi(
        &mut body.basic_blocks[3],
        v3,
        vec![(v1, BasicBlock(1)), (v2, BasicBlock(2))],
    );
    body.basic_blocks[3].terminator = Some(create_return());

    deconstruct_ssa(&mut body);

    // Verify:
    // bb3 has NO phi.
    let bb3_stmts = &body.basic_blocks[3].statements;
    assert!(bb3_stmts
        .iter()
        .all(|s| !matches!(s.kind, StatementKind::Assign(_, Rvalue::Phi(_)))));

    // bb1 should have v3 = v1
    let bb1 = &body.basic_blocks[1];
    let copy1 = bb1.statements.last().unwrap(); // Should be inserted at end
    if let StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(src))) = &copy1.kind {
        assert_eq!(dest.local, v3);
        assert_eq!(src.local, v1);
    } else {
        panic!("Expected copy in bb1");
    }

    // bb2 should have v3 = v2
    let bb2 = &body.basic_blocks[2];
    let copy2 = bb2.statements.last().unwrap();
    if let StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(src))) = &copy2.kind {
        assert_eq!(dest.local, v3);
        assert_eq!(src.local, v2);
    } else {
        panic!("Expected copy in bb2");
    }
}

#[test]
fn test_destruction_critical_edge() {
    // bb0 -> bb1 (phi) and bb2 (phi) via switch
    // bb0: switch { 0 => bb1, else => bb2 }
    // bb1: x = phi(v1, bb0)
    // bb2: y = phi(v2, bb0)

    // Since bb0 has multiple successors, and bb1/bb2 have Phis using bb0,
    // we MUST split edges bb0->bb1 and bb0->bb2.

    let mut body = create_body();
    let v1 = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let v2 = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let x = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));
    let y = create_local(&mut body, Type::new(TypeKind::I64, Span::default()));

    for _ in 0..3 {
        body.basic_blocks.push(BasicBlockData::new(None));
    }

    // bb0
    body.basic_blocks[0].terminator = Some(create_branch(BasicBlock(1), BasicBlock(2)));
    // bb1: x = phi(v1, bb0)
    create_assign_phi(&mut body.basic_blocks[1], x, vec![(v1, BasicBlock(0))]);
    body.basic_blocks[1].terminator = Some(create_return());
    // bb2: y = phi(v2, bb0)
    create_assign_phi(&mut body.basic_blocks[2], y, vec![(v2, BasicBlock(0))]);
    body.basic_blocks[2].terminator = Some(create_return());

    deconstruct_ssa(&mut body);

    // Verify edge splitting.
    // bb0 terminator should now point to NEW blocks, say bb3 and bb4.
    let term0 = body.basic_blocks[0].terminator.as_ref().unwrap();
    let mut targets = term0.successors().into_iter();
    let t1 = targets.next().unwrap();
    let t2 = targets.next().unwrap();

    assert!(t1.0 != 1 && t1.0 != 2); // Should be new blocks
    assert!(t2.0 != 1 && t2.0 != 2);

    // Check new blocks have copies
    // One block should have x = v1. Other y = v2.
    // Identifying which is which:
    // t1 corresponds to branch target 1?
    // t1 is 'succ' in split_edge logic.
    // If t1 eventually goes to bb1, it should have x=v1.

    let check_split =
        |curr: BasicBlock, target_local: miri::mir::Local, val_local: miri::mir::Local| {
            let bb = &body.basic_blocks[curr.0];
            // Should contain assignment
            let has_copy = bb.statements.iter().any(|s| {
                if let StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(src))) = &s.kind {
                    dest.local == target_local && src.local == val_local
                } else {
                    false
                }
            });
            assert!(
                has_copy,
                "Block {:?} missing copy {:?} = {:?}",
                curr, target_local, val_local
            );

            // Should goto original target
            // We don't know original target from here easily without checking graph,
            // but we can assume correctness if copy is present.
        };

    // targets: (1, t1), otherwise t2.
    // t1 replaces bb1 (target1). So t1 needs x=v1.
    // t2 replaces bb2 (otherwise). So t2 needs y=v2.

    // Warning: SwitchInt targets vec order might match successors() iterator order?
    // successors() usually yields targets then otherwise.

    check_split(t1, x, v1);
    check_split(t2, y, v2);
}
