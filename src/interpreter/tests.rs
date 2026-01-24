// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::*;
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::block::BasicBlockData;
use crate::mir::place::Local;
use crate::mir::{
    BasicBlock, ExecutionModel, LocalDecl, Operand, Place, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind,
};

#[test]
fn test_gpu_launch_terminator() {
    let mut interpreter = Interpreter::new();
    let span = Span::default();

    // specific to GpuLaunch test
    // We construct a Function Body that has a GpuLaunch terminator.
    let mut body = Body::new(0, span.clone(), ExecutionModel::Cpu);

    // We need some locals for operands.
    // _0: return
    body.new_local(LocalDecl::new(
        Type::new(TypeKind::Void, span.clone()),
        span.clone(),
    ));

    // Create dummy operands.
    // We need a Kernel operand.
    // Let's make a dummy local for it.
    let kernel_ty = Type::new(TypeKind::Custom("Kernel".to_string(), None), span.clone());
    let kernel_local = body.new_local(LocalDecl::new(kernel_ty, span.clone()));
    // We must initialize it or Interpreter will complain Uninitialized.
    // Wait, we can't easily initialize it without running statements.
    // Or we can pre-populate locals in Interpreter?
    // Interpreter::call uses execute_function which creates new EvalContext and empty locals (except args).
    // So we must rely on args or init code.
    // Let's make kernel an ARGUMENT (Local 1).
    body.arg_count = 1;

    let target_bb = BasicBlock(1);

    let mut block_data = BasicBlockData::new(None);
    block_data.terminator = Some(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: Operand::Copy(Place::new(kernel_local)), // _1
            grid: Operand::Constant(Box::new(crate::mir::Constant {
                span: span.clone(),
                ty: Type::new(TypeKind::Int, span.clone()),
                literal: crate::ast::literal::Literal::Integer(
                    crate::ast::literal::IntegerLiteral::I32(1),
                ),
            })),
            block: Operand::Constant(Box::new(crate::mir::Constant {
                span: span.clone(),
                ty: Type::new(TypeKind::Int, span.clone()),
                literal: crate::ast::literal::Literal::Integer(
                    crate::ast::literal::IntegerLiteral::I32(1),
                ),
            })),
            destination: Place::new(Local(0)), // result to return value
            target: Some(target_bb),
        },
        span.clone(),
    ));

    body.basic_blocks.push(block_data); // bb0

    // Target block (bb1) simply returns
    let mut target_data = BasicBlockData::new(None);
    target_data.terminator = Some(Terminator::new(TerminatorKind::Return, span.clone()));
    body.basic_blocks.push(target_data); // bb1

    interpreter.functions.insert("gpu_test".to_string(), body);

    // Call it. Arg 1 is kernel.
    let kernel_val = Value::Struct("Kernel".to_string(), HashMap::new());

    let result = interpreter.call("gpu_test", vec![kernel_val]);

    assert!(result.is_ok(), "GpuLaunch should succeed (simulated)");
}

#[test]
fn test_phi_node() {
    let mut interpreter = Interpreter::new();
    let span = Span::default();

    // Construct a diamond CFG:
    // bb0: Switch(arg1) -> [0: bb1, 1: bb2]
    // bb1: goto bb3
    // bb2: goto bb3
    // bb3: _0 = Phi([(10, bb1), (20, bb2)]); Return

    let mut body = Body::new(1, span.clone(), ExecutionModel::Cpu); // 1 arg
                                                                    // _0: return
    body.new_local(LocalDecl::new(
        Type::new(TypeKind::Int, span.clone()),
        span.clone(),
    ));
    // _1: arg (int)
    body.new_local(LocalDecl::new(
        Type::new(TypeKind::Int, span.clone()),
        span.clone(),
    ));

    // bb0
    let mut bb0 = BasicBlockData::new(None);
    let bb1 = BasicBlock(1);
    let bb2 = BasicBlock(2);
    let bb3 = BasicBlock(3); // exit

    use crate::mir::Discriminant;
    bb0.terminator = Some(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(Local(1))),
            targets: vec![(Discriminant::from(0), bb1), (Discriminant::from(1), bb2)],
            otherwise: bb1, // default to path 1
        },
        span.clone(),
    ));
    body.basic_blocks.push(bb0);

    // bb1
    let mut bb1_data = BasicBlockData::new(None);
    bb1_data.terminator = Some(Terminator::new(
        TerminatorKind::Goto { target: bb3 },
        span.clone(),
    ));
    body.basic_blocks.push(bb1_data);

    // bb2
    let mut bb2_data = BasicBlockData::new(None);
    bb2_data.terminator = Some(Terminator::new(
        TerminatorKind::Goto { target: bb3 },
        span.clone(),
    ));
    body.basic_blocks.push(bb2_data);

    // bb3
    let mut bb3_data = BasicBlockData::new(None);
    // Add Phi statement: _0 = Phi(...)
    // Phi is an Rvalue. We need an Assign statement.

    // Need to use correct Constant struct fields. `ty` not `ts`?
    // Let's check Constant definition. `pub struct Constant { pub span: Span, pub ty: Type, pub literal: Literal }`

    // Correcting constant construction:
    let const_10 = Operand::Constant(Box::new(crate::mir::Constant {
        span: span.clone(),
        ty: Type::new(TypeKind::Int, span.clone()),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(
            10,
        )),
    }));
    let const_20 = Operand::Constant(Box::new(crate::mir::Constant {
        span: span.clone(),
        ty: Type::new(TypeKind::Int, span.clone()),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(
            20,
        )),
    }));

    let phi_args_corrected = vec![(const_10, bb1), (const_20, bb2)];

    bb3_data.statements.push(Statement {
        kind: StatementKind::Assign(Place::new(Local(0)), Rvalue::Phi(phi_args_corrected)),
        span: span.clone(),
    });

    bb3_data.terminator = Some(Terminator::new(TerminatorKind::Return, span.clone()));
    body.basic_blocks.push(bb3_data);

    interpreter.functions.insert("phi_test".to_string(), body);

    // Test Path 1 (arg=0) -> expect 10
    let res1 = interpreter.call("phi_test", vec![Value::Int(0)]).unwrap();
    assert_eq!(res1, Value::Int(10), "Path 1 should yield 10");

    // Test Path 2 (arg=1) -> expect 20
    let res2 = interpreter.call("phi_test", vec![Value::Int(1)]).unwrap();
    assert_eq!(res2, Value::Int(20), "Path 2 should yield 20");
}
#[test]
fn test_ref_counting() {
    let mut interpreter = Interpreter::new();
    let span = Span::default();

    // Body:
    // _0: return (void)
    // _1: ptr

    let mut body = Body::new(0, span.clone(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(
        Type::new(TypeKind::Void, span.clone()),
        span.clone(),
    )); // _0
    body.new_local(LocalDecl::new(
        Type::new(TypeKind::Int, span.clone()),
        span.clone(),
    )); // _1 (type doesn't matter much here)

    let mut bb0 = BasicBlockData::new(None);

    // 1. _1 = Allocate(8, 8, 0)
    let size_op = Operand::Constant(Box::new(crate::mir::Constant {
        span: span.clone(),
        ty: Type::new(TypeKind::Int, span.clone()),
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I64(8)),
    }));

    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(1)),
            Rvalue::Allocate(size_op.clone(), size_op.clone(), size_op.clone()),
        ),
        span: span.clone(),
    });

    // 2. IncRef(_1)
    bb0.statements.push(Statement {
        kind: StatementKind::IncRef(Place::new(Local(1))),
        span: span.clone(),
    });

    // 3. DecRef(_1)
    bb0.statements.push(Statement {
        kind: StatementKind::DecRef(Place::new(Local(1))),
        span: span.clone(),
    });

    bb0.terminator = Some(Terminator::new(TerminatorKind::Return, span.clone()));
    body.basic_blocks.push(bb0);

    interpreter.functions.insert("rc_test".to_string(), body);

    // Run basic alloc/inc/dec
    let _ = interpreter.call("rc_test", vec![]).unwrap();

    // Verify heap state.
    // We allocated one object. ID should be 1.
    // Initial RC=1. IncRef -> 2. DecRef -> 1.
    // Should still exist.
    let val = interpreter.heap_get(1);
    assert!(val.is_some(), "Heap object 1 should exist with RC=1");

    // Now verify DecRef to 0 triggers dealloc.
    // We can't run more MIR easily without modifying body, so we use internal API to verify.
    // The previous call finished.
    let dropped = interpreter.heap_dec_ref(1);
    assert!(dropped, "DecRef to 0 should return true (dropped)");

    let val_after = interpreter.heap_get(1);
    assert!(val_after.is_none(), "Heap object 1 should be gone");
}
