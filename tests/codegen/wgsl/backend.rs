// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::codegen::backend::Backend;
use miri::codegen::wgsl::{WgslBackend, WgslOptions};
use miri::error::syntax::Span;
use miri::mir::backend::{BackendMetadata, GpuBodyMetadata};
use miri::mir::{
    AggregateKind, BasicBlock, BasicBlockData, BinOp, Body, Constant, Dimension, ExecutionModel,
    GpuIntrinsic, Local, LocalDecl, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind,
    StorageClass, Terminator, TerminatorKind, UnOp,
};

fn dummy_span() -> Span {
    Span::new(0, 0)
}

fn f32_array_type() -> Type {
    let span = dummy_span();
    let f32_ty = Type::new(TypeKind::F32, span);
    let f32_expr = Expression::new(0, ExpressionKind::Type(Box::new(f32_ty), false), span);
    Type::new(TypeKind::List(Box::new(f32_expr)), span)
}

/// Construct a minimal MIR body equivalent to:
///
/// ```text
/// gpu fn copy_kernel(input [f32], output out [f32]):
///     let i = gpu_thread_idx.x
///     output[i] = input[i]
/// ```
fn build_copy_kernel() -> Body {
    let span = dummy_span();
    let mut body = Body::new(2, span, ExecutionModel::GpuKernel);

    let void_ty = Type::new(TypeKind::Void, span);
    body.local_decls.push(LocalDecl::new(void_ty, span));

    let array_ty = f32_array_type();
    let mut input = LocalDecl::new(array_ty.clone(), span);
    input.storage_class = StorageClass::GpuGlobal;
    input.name = Some("input".into());
    input.is_user_variable = true;
    body.local_decls.push(input);

    let mut output = LocalDecl::new(array_ty, span);
    output.storage_class = StorageClass::GpuGlobal;
    output.name = Some("output".into());
    output.is_user_variable = true;
    body.local_decls.push(output);

    body.out_params = vec![false, true];
    body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([64, 1, 1]),
        required_capabilities: Vec::new(),
    }));

    let u32_ty = Type::new(TypeKind::U32, span);
    let idx_local = body.new_local(LocalDecl::new(u32_ty, span));
    let f32_ty = Type::new(TypeKind::F32, span);
    let val_local = body.new_local(LocalDecl::new(f32_ty, span));

    let idx_place = Place::new(idx_local);
    let val_place = Place::new(val_local);

    let mut bb0 = BasicBlockData::new(None);
    bb0.statements.push(Statement {
        kind: StatementKind::StorageLive(idx_place.clone()),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            idx_place.clone(),
            Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X)),
        ),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::StorageLive(val_place.clone()),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            val_place.clone(),
            Rvalue::Use(Operand::Copy(Place {
                local: Local(1),
                projection: vec![PlaceElem::Index(idx_local)],
            })),
        ),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place {
                local: Local(2),
                projection: vec![PlaceElem::Index(idx_local)],
            },
            Rvalue::Use(Operand::Copy(val_place)),
        ),
        span,
    });
    bb0.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb0);
    body
}

fn assert_wgsl_valid(source: &str) {
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|err| {
        panic!(
            "naga failed to parse generated WGSL:\n{}\n--- source ---\n{}",
            err.emit_to_string(source),
            source
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator.validate(&module).unwrap_or_else(|err| {
        panic!(
            "naga failed to validate generated WGSL: {:?}\n--- source ---\n{}",
            err, source
        )
    });
}

#[test]
fn copy_kernel_emits_wgsl_that_passes_naga_validation() {
    let body = build_copy_kernel();
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("copy_kernel", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");

    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");
    assert_wgsl_valid(source);
}

#[test]
fn copy_kernel_emits_compute_entry_point_with_workgroup_size() {
    let body = build_copy_kernel();
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("copy_kernel", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");

    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");
    assert!(
        source.contains("@compute"),
        "expected @compute attribute, got:\n{}",
        source
    );
    assert!(
        source.contains("@workgroup_size(64, 1, 1)"),
        "expected @workgroup_size(64, 1, 1), got:\n{}",
        source
    );
    assert!(
        source.contains("fn copy_kernel"),
        "expected entry-point named copy_kernel, got:\n{}",
        source
    );
}

#[test]
fn input_buffer_is_read_only_storage() {
    let body = build_copy_kernel();
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("copy_kernel", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");
    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");

    assert!(
        source.contains("var<storage, read>"),
        "input buffer should be read-only storage, got:\n{}",
        source
    );
}

#[test]
fn output_buffer_is_read_write_storage() {
    let body = build_copy_kernel();
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("copy_kernel", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");
    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");

    assert!(
        source.contains("var<storage, read_write>"),
        "output buffer should be read-write storage, got:\n{}",
        source
    );
}

#[test]
fn backend_name_is_wgsl() {
    assert_eq!(WgslBackend.name(), "wgsl");
}

#[test]
fn cpu_function_is_skipped() {
    let span = dummy_span();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    body.local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    let mut bb = BasicBlockData::new(None);
    bb.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb);

    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("cpu_fn", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");
    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");
    assert!(
        !source.contains("fn cpu_fn"),
        "CPU functions must not be emitted as WGSL, got:\n{}",
        source
    );
}

#[test]
fn math_intrinsic_maps_to_wgsl_sqrt() {
    use miri::mir::MathIntrinsic;
    let span = dummy_span();
    let mut body = Body::new(1, span, ExecutionModel::GpuKernel);

    let void_ty = Type::new(TypeKind::Void, span);
    body.local_decls.push(LocalDecl::new(void_ty, span));

    let array_ty = f32_array_type();
    let mut buf = LocalDecl::new(array_ty, span);
    buf.storage_class = StorageClass::GpuGlobal;
    buf.name = Some("buf".into());
    buf.is_user_variable = true;
    body.local_decls.push(buf);

    body.out_params = vec![true];
    body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([1, 1, 1]),
        required_capabilities: Vec::new(),
    }));

    let u32_ty = Type::new(TypeKind::U32, span);
    let idx_local = body.new_local(LocalDecl::new(u32_ty, span));
    let f32_ty = Type::new(TypeKind::F32, span);
    let in_local = body.new_local(LocalDecl::new(f32_ty.clone(), span));
    let out_local = body.new_local(LocalDecl::new(f32_ty, span));

    let mut bb0 = BasicBlockData::new(None);
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(idx_local),
            Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X)),
        ),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(in_local),
            Rvalue::Use(Operand::Copy(Place {
                local: Local(1),
                projection: vec![PlaceElem::Index(idx_local)],
            })),
        ),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(out_local),
            Rvalue::MathIntrinsic(
                MathIntrinsic::Sqrt,
                vec![Operand::Copy(Place::new(in_local))],
            ),
        ),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place {
                local: Local(1),
                projection: vec![PlaceElem::Index(idx_local)],
            },
            Rvalue::Use(Operand::Copy(Place::new(out_local))),
        ),
        span,
    });
    bb0.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb0);

    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("sqrt_kernel", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");
    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");
    assert!(
        source.contains("sqrt("),
        "expected sqrt(...) call, got:\n{}",
        source
    );
    assert_wgsl_valid(source);
}

#[test]
fn block_dim_intrinsic_substitutes_workgroup_size_literal() {
    let span = dummy_span();
    let mut body = Body::new(1, span, ExecutionModel::GpuKernel);

    let void_ty = Type::new(TypeKind::Void, span);
    body.local_decls.push(LocalDecl::new(void_ty, span));

    let array_ty = f32_array_type();
    let mut buf = LocalDecl::new(array_ty, span);
    buf.storage_class = StorageClass::GpuGlobal;
    buf.name = Some("buf".into());
    buf.is_user_variable = true;
    body.local_decls.push(buf);

    body.out_params = vec![true];
    body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([128, 1, 1]),
        required_capabilities: Vec::new(),
    }));

    let u32_ty = Type::new(TypeKind::U32, span);
    let dim_local = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    let idx_local = body.new_local(LocalDecl::new(u32_ty, span));

    let mut bb0 = BasicBlockData::new(None);
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(dim_local),
            Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(Dimension::X)),
        ),
        span,
    });
    bb0.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(idx_local),
            Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X)),
        ),
        span,
    });
    bb0.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb0);

    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("dim_kernel", &body)], &WgslOptions::default())
        .expect("WGSL backend should succeed");
    let source = std::str::from_utf8(&artifact.bytes).expect("WGSL output is UTF-8");
    assert!(
        source.contains("128u"),
        "expected workgroup_size.x literal 128u, got:\n{}",
        source
    );
    assert!(
        !source.contains("workgroup_size_x"),
        "must not emit invalid identifier workgroup_size_x, got:\n{}",
        source
    );
    assert_wgsl_valid(source);
}

fn single_buffer_kernel() -> Body {
    let span = dummy_span();
    let mut body = Body::new(1, span, ExecutionModel::GpuKernel);
    body.local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    let mut buf = LocalDecl::new(f32_array_type(), span);
    buf.storage_class = StorageClass::GpuGlobal;
    buf.name = Some("buf".into());
    buf.is_user_variable = true;
    body.local_decls.push(buf);
    body.out_params = vec![true];
    body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([1, 1, 1]),
        required_capabilities: Vec::new(),
    }));
    body
}

fn finish_body(body: &mut Body, stmts: Vec<Statement>) {
    let span = dummy_span();
    let mut bb = BasicBlockData::new(None);
    bb.statements = stmts;
    bb.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb);
}

fn compile_err(body: &Body) -> String {
    let backend = WgslBackend;
    let err = backend
        .compile(&[("k", body)], &WgslOptions::default())
        .expect_err("compile must fail");
    format!("{:?}", err)
}

fn assign(place: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(place, rvalue),
        span: dummy_span(),
    }
}

#[test]
fn multi_block_goto_is_rejected() {
    let span = dummy_span();
    let mut body = single_buffer_kernel();
    let mut bb0 = BasicBlockData::new(None);
    bb0.terminator = Some(Terminator::new(
        TerminatorKind::Goto {
            target: BasicBlock(1),
        },
        span,
    ));
    let mut bb1 = BasicBlockData::new(None);
    bb1.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb0);
    body.basic_blocks.push(bb1);

    let msg = compile_err(&body);
    assert!(
        msg.contains("Goto") && msg.contains("not yet supported"),
        "expected Goto-not-supported error, got: {}",
        msg
    );
}

#[test]
fn out_params_length_mismatch_is_rejected() {
    let mut body = single_buffer_kernel();
    body.out_params.clear();
    finish_body(&mut body, vec![]);

    let msg = compile_err(&body);
    assert!(
        msg.contains("out_params length"),
        "expected out_params length error, got: {}",
        msg
    );
}

#[test]
fn deref_projection_is_rejected() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let f32_ty = Type::new(TypeKind::F32, span);
    let val = body.new_local(LocalDecl::new(f32_ty, span));
    let stmt = assign(
        Place::new(val),
        Rvalue::Use(Operand::Copy(Place {
            local: Local(1),
            projection: vec![PlaceElem::Deref],
        })),
    );
    finish_body(&mut body, vec![stmt]);

    let msg = compile_err(&body);
    assert!(msg.contains("Deref"), "expected Deref error, got: {}", msg);
}

#[test]
fn move_operand_renders_like_copy() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let idx = body.new_local(LocalDecl::new(u32_ty, span));
    let f32_ty = Type::new(TypeKind::F32, span);
    let val = body.new_local(LocalDecl::new(f32_ty, span));
    finish_body(
        &mut body,
        vec![
            assign(
                Place::new(idx),
                Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X)),
            ),
            assign(
                Place::new(val),
                Rvalue::Use(Operand::Move(Place {
                    local: Local(1),
                    projection: vec![PlaceElem::Index(idx)],
                })),
            ),
        ],
    );
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("move_kernel", &body)], &WgslOptions::default())
        .expect("compile ok");
    let source = std::str::from_utf8(&artifact.bytes).unwrap();
    assert!(
        source.contains("buf[_"),
        "Move operand should still render as place access, got:\n{}",
        source
    );
}

#[test]
fn grid_block_sync_intrinsics_emit_expected_text() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let a = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    let b = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    let c = body.new_local(LocalDecl::new(u32_ty, span));
    finish_body(
        &mut body,
        vec![
            assign(
                Place::new(a),
                Rvalue::GpuIntrinsic(GpuIntrinsic::GridDim(Dimension::Y)),
            ),
            assign(
                Place::new(b),
                Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Z)),
            ),
            assign(
                Place::new(c),
                Rvalue::GpuIntrinsic(GpuIntrinsic::SyncThreads),
            ),
        ],
    );
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("g", &body)], &WgslOptions::default())
        .expect("compile ok");
    let source = std::str::from_utf8(&artifact.bytes).unwrap();
    assert!(
        source.contains("_num_workgroups.y"),
        "GridDim missing: {}",
        source
    );
    assert!(
        source.contains("_workgroup_id.z"),
        "BlockIdx missing: {}",
        source
    );
    assert!(
        source.contains("workgroupBarrier()"),
        "SyncThreads missing: {}",
        source
    );
}

#[test]
fn binop_offset_is_rejected() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let a = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    let b = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    let c = body.new_local(LocalDecl::new(u32_ty, span));
    finish_body(
        &mut body,
        vec![assign(
            Place::new(c),
            Rvalue::BinaryOp(
                BinOp::Offset,
                Box::new(Operand::Copy(Place::new(a))),
                Box::new(Operand::Copy(Place::new(b))),
            ),
        )],
    );

    let msg = compile_err(&body);
    assert!(
        msg.contains("pointer offset"),
        "expected pointer offset error, got: {}",
        msg
    );
}

#[test]
fn unop_await_is_rejected() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let a = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    let b = body.new_local(LocalDecl::new(u32_ty, span));
    finish_body(
        &mut body,
        vec![assign(
            Place::new(b),
            Rvalue::UnaryOp(UnOp::Await, Box::new(Operand::Copy(Place::new(a)))),
        )],
    );

    let msg = compile_err(&body);
    assert!(
        msg.contains("await") || msg.contains("Await"),
        "expected await error, got: {}",
        msg
    );
}

#[test]
fn cast_to_non_scalar_is_rejected() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let a = body.new_local(LocalDecl::new(u32_ty, span));
    let dst = body.new_local(LocalDecl::new(f32_array_type(), span));
    finish_body(
        &mut body,
        vec![assign(
            Place::new(dst),
            Rvalue::Cast(Box::new(Operand::Copy(Place::new(a))), f32_array_type()),
        )],
    );

    let msg = compile_err(&body);
    assert!(
        msg.contains("cannot represent type") || msg.contains("scalar"),
        "expected non-scalar cast error, got: {}",
        msg
    );
}

#[test]
fn unsupported_rvalue_aggregate_is_rejected() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let dst = body.new_local(LocalDecl::new(u32_ty, span));
    finish_body(
        &mut body,
        vec![assign(
            Place::new(dst),
            Rvalue::Aggregate(AggregateKind::Tuple, vec![]),
        )],
    );

    let msg = compile_err(&body);
    assert!(
        msg.contains("rvalue") && msg.contains("not yet supported"),
        "expected rvalue-not-supported error, got: {}",
        msg
    );
}

#[test]
fn string_literal_constant_is_rejected() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let u32_ty = Type::new(TypeKind::U32, span);
    let dst = body.new_local(LocalDecl::new(u32_ty.clone(), span));
    finish_body(
        &mut body,
        vec![assign(
            Place::new(dst),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: Type::new(TypeKind::String, span),
                literal: Literal::String("hi".into()),
            }))),
        )],
    );

    let msg = compile_err(&body);
    assert!(
        msg.contains("cannot embed literal"),
        "expected literal-embed error, got: {}",
        msg
    );
}

#[test]
fn integer_constant_renders_signed_for_signed_type() {
    let mut body = single_buffer_kernel();
    let span = dummy_span();
    let i32_ty = Type::new(TypeKind::I32, span);
    let dst = body.new_local(LocalDecl::new(i32_ty.clone(), span));
    finish_body(
        &mut body,
        vec![assign(
            Place::new(dst),
            Rvalue::Use(Operand::Constant(Box::new(Constant {
                span,
                ty: i32_ty,
                literal: Literal::Integer(IntegerLiteral::I32(-7)),
            }))),
        )],
    );
    let backend = WgslBackend;
    let artifact = backend
        .compile(&[("k", &body)], &WgslOptions::default())
        .expect("compile ok");
    let source = std::str::from_utf8(&artifact.bytes).unwrap();
    assert!(
        source.contains("= -7;"),
        "expected signed -7 literal, got:\n{}",
        source
    );
    assert!(
        !source.contains("-7u"),
        "signed literal must not get u suffix, got:\n{}",
        source
    );
}

#[test]
fn non_buffer_kernel_param_is_rejected() {
    let span = dummy_span();
    let mut body = Body::new(1, span, ExecutionModel::GpuKernel);

    let void_ty = Type::new(TypeKind::Void, span);
    body.local_decls.push(LocalDecl::new(void_ty, span));

    let i32_ty = Type::new(TypeKind::I32, span);
    let mut scalar_param = LocalDecl::new(i32_ty, span);
    scalar_param.name = Some("n".into());
    scalar_param.is_user_variable = true;
    body.local_decls.push(scalar_param);

    body.out_params = vec![false];
    body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([1, 1, 1]),
        required_capabilities: Vec::new(),
    }));

    let mut bb = BasicBlockData::new(None);
    bb.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb);

    let backend = WgslBackend;
    let err = backend
        .compile(&[("scalar_kernel", &body)], &WgslOptions::default())
        .expect_err("non-buffer kernel param must be rejected");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("storage class"),
        "expected error to mention storage class, got: {}",
        msg
    );
}

#[test]
fn gpu_device_function_is_rejected() {
    let span = dummy_span();
    let mut body = Body::new(0, span, ExecutionModel::GpuDevice);
    body.local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    let mut bb = BasicBlockData::new(None);
    bb.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb);

    let backend = WgslBackend;
    let err = backend
        .compile(&[("dev", &body)], &WgslOptions::default())
        .expect_err("GpuDevice must not be silently emitted as a kernel");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("GpuDevice"),
        "expected GpuDevice rejection, got: {}",
        msg
    );
}

#[test]
fn uniform_buffer_storage_class_is_rejected() {
    let span = dummy_span();
    let mut body = Body::new(1, span, ExecutionModel::GpuKernel);
    body.local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    let mut buf = LocalDecl::new(f32_array_type(), span);
    buf.storage_class = StorageClass::UniformBuffer;
    buf.name = Some("buf".into());
    buf.is_user_variable = true;
    body.local_decls.push(buf);

    body.out_params = vec![false];
    body.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([1, 1, 1]),
        required_capabilities: Vec::new(),
    }));
    let mut bb = BasicBlockData::new(None);
    bb.terminator = Some(Terminator::new(TerminatorKind::Return, span));
    body.basic_blocks.push(bb);

    let backend = WgslBackend;
    let err = backend
        .compile(&[("u", &body)], &WgslOptions::default())
        .expect_err("UniformBuffer storage class must be rejected");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("UniformBuffer"),
        "expected UniformBuffer rejection, got: {}",
        msg
    );
}
