// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `.reduce()` on gpu-resident arrays to GPU tree-reduction kernels.
//!
//! Extracts the fold function and generates a synthesized kernel that:
//! - Uses workgroup-shared memory (StorageClass::GpuShared) for reduction state.
//! - Performs a grid-stride loop to accumulate over the input.
//! - Executes parallel tree reduction with workgroup barriers.

use crate::ast::expression::ExpressionKind;
use crate::ast::literal::Literal;
use crate::ast::operator::BinaryOp;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, DIM3_TYPE_NAME};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::backend::{BackendMetadata, GpuBodyMetadata};
use crate::mir::body::BindingResidency;
use crate::mir::lambda::LambdaInfo;
use crate::mir::{
    AggregateKind, BinOp, Body, Constant, Dimension, Discriminant, ExecutionModel, GpuIntrinsic,
    GpuLaunchArgs, Local, LocalDecl, Operand, Place, Rvalue, Statement as MirStatement,
    StatementKind as MirStatementKind, StorageClass, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::expression::lower_expression;
use super::forall_gpu::{compute_thread_index, int_constant, needs_int_narrowing, push_assign};

/// Runtime entry that fences outstanding device writes and copies a
/// `gpu`-resident buffer back to its host array.
const READBACK_FN: &str = "miri_gpu_readback";

/// Block size for GPU reduction kernels (1D workgroups, 256 threads).
/// This value is coordinated with `GPU_REDUCE_BLOCK_SIZE`
/// and must match the `@workgroup_size` directive in the generated WGSL.
const GPU_REDUCE_BLOCK_SIZE: u32 = 256;

/// Try to lower a `.reduce()` call on a gpu-resident array to a GPU tree-reduction kernel.
///
/// Returns `Ok(Some(operand))` if successfully lowered (the operand is the result).
/// Returns `Ok(None)` if the receiver is not gpu-resident (falls through to CPU path).
/// Returns `Err(...)` for unsupported fold expressions or lowering errors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_lower_gpu_reduce(
    ctx: &mut LoweringContext,
    obj: &crate::ast::expression::Expression,
    obj_ty: &Type,
    init_expr: &crate::ast::expression::Expression,
    fold_expr: &crate::ast::expression::Expression,
    call_expr_id: usize,
    dest: Option<Place>,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    // Check if the receiver is gpu-resident.
    let obj_op = lower_expression(ctx, obj, None)?;
    let receiver_local = match &obj_op {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => place.local,
        _ => return Ok(None), // Not a simple local; fall through to CPU path
    };

    if ctx.body.local_decls[receiver_local.0].residency != BindingResidency::Gpu {
        return Ok(None); // Not gpu-resident; use CPU path
    }

    // Extract the fold binary operator from the fold function literal.
    let fold_op = extract_reduce_fold_op(fold_expr, *span)?;

    // Lower the init expression to get the scalar initial value.
    let init_op = lower_expression(ctx, init_expr, None)?;

    // Extract the real array length N from Array<T, N> type.
    let array_length = extract_array_length_from_type(obj_ty, *span)?;

    // Build the reduction kernel.
    let kernel_name = format!("miri_gpu_reduce_{}", ctx.kernel_index(call_expr_id));
    let kernel_body = build_gpu_reduce_kernel(ctx, obj_ty, array_length, fold_op, *span)?;

    ctx.lambda_bodies.push(LambdaInfo {
        name: kernel_name.clone(),
        body: kernel_body,
        captures: Vec::new(),
    });

    // Emit the GpuLaunch terminator. Returns an operand reading the result
    // (the reduced scalar) out of the 1-element output buffer, plus that
    // buffer's device handle.
    // If dest is gpu-resident, skip the readback (buffer stays on GPU).
    let dest_is_gpu_resident = match &dest {
        Some(d) => ctx.body.local_decls[d.local.0].residency == BindingResidency::Gpu,
        None => false,
    };
    let (output_op, handle_id) = emit_gpu_reduce_launch(
        ctx,
        &kernel_name,
        receiver_local,
        init_op,
        *span,
        dest_is_gpu_resident,
    )?;

    // Honor the caller's destination: the call lowering passes `dest` for
    // `let sum = a.reduce(...)` and expects the intrinsic to write it. Mirror
    // the `element_at` intrinsic — write the result into `dest` (or a temp) and
    // return a Copy of it. Without this the binding never receives the value.
    let elem_ty = extract_element_type(obj_ty)?;

    // Save the destination local before moving dest (for handle transfer below)
    let dest_local_opt = dest.as_ref().map(|d| d.local);

    let (destination, result_op) = match dest {
        Some(d) => (d.clone(), Operand::Copy(d)),
        None => {
            let temp = ctx.push_temp(elem_ty, *span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        }
    };
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(destination, Rvalue::Use(output_op)),
        span: *span,
    });

    // For gpu-resident results, transfer the persistent device buffer's
    // handle from output_local to the destination binding. The destination local
    // already has a device_handle allocated (in apply_variable_residency), but
    // for reduce results, we want it to reference the 1-element output buffer
    // instead. This ensures cross-residency assignment (`let h = gpu_sum`) uses
    // the correct device buffer for readback.
    // `_reduce_out` is a local of the enclosing scope, which releases it on every
    // exit path; releasing it here as well would free the buffer twice.
    if dest_is_gpu_resident {
        if let Some(dest_local) = dest_local_opt {
            ctx.body.local_decls[dest_local.0].device_handle = Some(handle_id);
        }
    }

    Ok(Some(result_op))
}

/// Extract a binary operator from a fold function literal.
/// Accepts only `fn(a T, b T) T: a OP b` where OP is + or * and both operands are the parameters.
fn extract_reduce_fold_op(
    fold_expr: &crate::ast::expression::Expression,
    span: Span,
) -> Result<BinOp, LoweringError> {
    if let ExpressionKind::Lambda(lambda_data) = &fold_expr.node {
        if lambda_data.params.len() != 2 {
            return Err(LoweringError::unsupported_expression(
                format!(
                    "reduce fold function must take exactly 2 parameters, got {}",
                    lambda_data.params.len()
                ),
                span,
            ));
        }

        let param1_name = &lambda_data.params[0].name;
        let param2_name = &lambda_data.params[1].name;

        // Body is a Statement; check if it's an expression statement with a binary operation.
        if let crate::ast::statement::StatementKind::Expression(expr) = &lambda_data.body.node {
            if let ExpressionKind::Binary(lhs, op, rhs) = &expr.node {
                // Verify both operands are identifiers naming the two parameters (either order).
                let lhs_is_param =
                    is_identifier_param(lhs, param1_name) || is_identifier_param(lhs, param2_name);
                let rhs_is_param =
                    is_identifier_param(rhs, param1_name) || is_identifier_param(rhs, param2_name);

                if !lhs_is_param || !rhs_is_param {
                    return Err(LoweringError::unsupported_expression(
                        "reduce fold operands must be the two fold parameters".to_string(),
                        span,
                    ));
                }

                // Check the operator is associative and commutative.
                match op {
                    BinaryOp::Add | BinaryOp::Mul => Ok(mir_binop_from_ast(*op)),
                    _ => Err(LoweringError::unsupported_expression(
                        "reduce fold must use an associative binary operator (+ or *) over its two parameters".to_string(),
                        span,
                    )),
                }
            } else {
                Err(LoweringError::unsupported_expression(
                    "reduce fold body must be a single binary operation".to_string(),
                    span,
                ))
            }
        } else {
            Err(LoweringError::unsupported_expression(
                "reduce fold body must be an expression".to_string(),
                span,
            ))
        }
    } else {
        Err(LoweringError::unsupported_expression(
            "reduce fold must be a function literal".to_string(),
            span,
        ))
    }
}

/// Check if an expression is an identifier with the given name.
fn is_identifier_param(expr: &crate::ast::expression::Expression, name: &str) -> bool {
    matches!(
        &expr.node,
        ExpressionKind::Identifier(id, None) if id == name
    )
}

/// Convert AST BinaryOp to MIR BinOp.
/// Only Add and Mul are valid; this function is called after validation so any
/// other op indicates a bug in the validation logic.
fn mir_binop_from_ast(op: BinaryOp) -> BinOp {
    match op {
        BinaryOp::Add => BinOp::Add,
        BinaryOp::Mul => BinOp::Mul,
        _ => unreachable!(
            "mir_binop_from_ast called with non-associative operator; \
             this should have been caught by extract_reduce_fold_op validation"
        ),
    }
}

/// Extract the real array length N from Array<T, N> type.
fn extract_array_length_from_type(arr_ty: &Type, span: Span) -> Result<i64, LoweringError> {
    if let TypeKind::Custom(name, Some(type_args)) = &arr_ty.kind {
        if name == BuiltinCollectionKind::Array.name() && type_args.len() >= 2 {
            // The second type_arg is the size expression; try to const-eval it.
            if let Some(val) = crate::type_checker::TypeChecker::try_eval_const_int(&type_args[1]) {
                return Ok(val as i64);
            }
        }
    }
    Err(LoweringError::unsupported_expression(
        "reduce requires an Array<T, N> with a const-evaluable size N".to_string(),
        span,
    ))
}

/// Extract the element type T from Array<T, N>.
fn extract_element_type(arr_ty: &Type) -> Result<Type, LoweringError> {
    if let TypeKind::Custom(name, Some(type_args)) = &arr_ty.kind {
        if name == BuiltinCollectionKind::Array.name() && !type_args.is_empty() {
            // The type_args are expressions, so we need to check if they're type expressions.
            if let ExpressionKind::Type(elem_type, _) = &type_args[0].node {
                return Ok(elem_type.as_ref().clone());
            }
        }
    }
    Err(LoweringError::unsupported_expression(
        "expected Array<T, N>".to_string(),
        Span::default(),
    ))
}

/// Get the identity element for a binary operator.
fn identity_for_op(op: BinOp, elem_ty: &Type) -> Operand {
    let span = Span::default();
    match op {
        BinOp::Add => {
            if matches!(elem_ty.kind, TypeKind::F32) {
                Operand::Constant(Box::new(Constant {
                    span,
                    ty: elem_ty.clone(),
                    literal: Literal::Float(crate::ast::literal::FloatLiteral::F32(0u32)),
                }))
            } else if matches!(elem_ty.kind, TypeKind::F64 | TypeKind::Float) {
                Operand::Constant(Box::new(Constant {
                    span,
                    ty: elem_ty.clone(),
                    literal: Literal::Float(crate::ast::literal::FloatLiteral::F64(0u64)),
                }))
            } else {
                Operand::Constant(Box::new(Constant {
                    span,
                    ty: elem_ty.clone(),
                    literal: Literal::Integer(crate::ast::literal::IntegerLiteral::I64(0)),
                }))
            }
        }
        BinOp::Mul => {
            if matches!(elem_ty.kind, TypeKind::F32) {
                Operand::Constant(Box::new(Constant {
                    span,
                    ty: elem_ty.clone(),
                    literal: Literal::Float(crate::ast::literal::FloatLiteral::F32(1065353216u32)), // 1.0 as f32 bits
                }))
            } else if matches!(elem_ty.kind, TypeKind::F64 | TypeKind::Float) {
                Operand::Constant(Box::new(Constant {
                    span,
                    ty: elem_ty.clone(),
                    literal: Literal::Float(crate::ast::literal::FloatLiteral::F64(
                        4607182119529216000u64,
                    )), // 1.0 as f64 bits
                }))
            } else {
                Operand::Constant(Box::new(Constant {
                    span,
                    ty: elem_ty.clone(),
                    literal: Literal::Integer(crate::ast::literal::IntegerLiteral::I64(1)),
                }))
            }
        }
        _ => int_constant(0, span),
    }
}

/// Helper to assign a value to a place.
fn push_assign_place(ctx: &mut LoweringContext, place: Place, rvalue: Rvalue, span: Span) {
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(place, rvalue),
        span,
    });
}

/// Emits a borrowing call to a runtime entry, splitting the current block.
/// Used for GPU readback to fence device work and copy results back to host.
fn emit_void_runtime_call(
    ctx: &mut LoweringContext,
    fn_name: &str,
    args: Vec<Operand>,
    span: Span,
) {
    let func = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier(fn_name.to_string()),
    }));
    let dest_local = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

/// Constructs a device handle operand (the handle ID as an i64 constant).
fn handle_operand(handle: crate::mir::body::DeviceHandleId, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(crate::ast::literal::IntegerLiteral::I64(handle.0 as i64)),
    }))
}

/// Setup kernel parameters and shared memory for reduction.
fn setup_reduce_kernel_params(
    ctx: &mut LoweringContext,
    obj_ty: &Type,
    span: Span,
) -> Result<(Local, Local, Local, Local, Local, u32), LoweringError> {
    let workgroup_size = GPU_REDUCE_BLOCK_SIZE;
    let input_local = ctx.push_param("input".to_string(), obj_ty.clone(), span);
    ctx.body.local_decls[input_local.0].storage_class = StorageClass::GpuGlobal;

    let elem_ty = extract_element_type(obj_ty)?;
    let init_local = ctx.push_param("init".to_string(), elem_ty.clone(), span);
    ctx.body.local_decls[init_local.0].storage_class = StorageClass::UniformBuffer;

    let output_local = ctx.push_param("output".to_string(), obj_ty.clone(), span);
    ctx.body.local_decls[output_local.0].storage_class = StorageClass::GpuGlobal;

    let sdata_array_ty = Type::new(
        TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![
                crate::ast::expression::Expression {
                    id: 0,
                    node: ExpressionKind::Type(Box::new(elem_ty), false),
                    span,
                },
                crate::ast::expression::Expression {
                    id: 0,
                    node: ExpressionKind::Literal(Literal::Integer(
                        crate::ast::literal::IntegerLiteral::I64(i64::from(workgroup_size)),
                    )),
                    span,
                },
            ]),
        ),
        span,
    );

    let sdata_local = ctx.push_local("_sdata".to_string(), sdata_array_ty, span);
    ctx.body.local_decls[sdata_local.0].storage_class = StorageClass::GpuShared;

    let thread_idx = compute_thread_index(ctx, Dimension::X, span);

    Ok((
        input_local,
        init_local,
        output_local,
        sdata_local,
        thread_idx,
        workgroup_size,
    ))
}

/// Initialize accumulator with lane-0 getting init value, others getting identity.
fn emit_acc_init(
    ctx: &mut LoweringContext,
    init_local: Local,
    thread_idx: Local,
    fold_op: BinOp,
    elem_ty: &Type,
    span: Span,
) -> Result<Local, LoweringError> {
    let identity_literal = identity_for_op(fold_op, elem_ty);
    let acc_local = ctx.push_local("acc".to_string(), elem_ty.clone(), span);
    let is_lane_zero = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);

    push_assign(
        ctx,
        is_lane_zero,
        Rvalue::BinaryOp(
            BinOp::Eq,
            Box::new(Operand::Copy(Place::new(thread_idx))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );

    let then_bb = ctx.new_basic_block();
    let else_bb = ctx.new_basic_block();
    let merge_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(is_lane_zero)),
            targets: vec![(Discriminant::bool_true(), then_bb)],
            otherwise: else_bb,
        },
        span,
    ));

    ctx.set_current_block(then_bb);
    push_assign(
        ctx,
        acc_local,
        Rvalue::Use(Operand::Copy(Place::new(init_local))),
        span,
    );
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: merge_bb },
        span,
    ));

    ctx.set_current_block(else_bb);
    push_assign(ctx, acc_local, Rvalue::Use(identity_literal), span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: merge_bb },
        span,
    ));

    ctx.set_current_block(merge_bb);
    Ok(acc_local)
}

/// Setup loop variable and branch to grid-stride loop.
fn setup_grid_stride_loop(
    ctx: &mut LoweringContext,
    thread_idx: Local,
    span: Span,
) -> (
    Local,
    crate::mir::BasicBlock,
    crate::mir::BasicBlock,
    crate::mir::BasicBlock,
) {
    let loop_idx = ctx.push_local("i".to_string(), Type::new(TypeKind::Int, span), span);
    push_assign(
        ctx,
        loop_idx,
        Rvalue::Use(Operand::Copy(Place::new(thread_idx))),
        span,
    );

    let loop_start_bb = ctx.new_basic_block();
    let loop_body_bb = ctx.new_basic_block();
    let loop_exit_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop_start_bb,
        },
        span,
    ));

    (loop_idx, loop_start_bb, loop_body_bb, loop_exit_bb)
}

/// Emit condition check and loop body for grid-stride accumulation.
#[allow(clippy::too_many_arguments)]
fn emit_grid_stride_body(
    ctx: &mut LoweringContext,
    loop_idx: Local,
    loop_start_bb: crate::mir::BasicBlock,
    loop_body_bb: crate::mir::BasicBlock,
    loop_exit_bb: crate::mir::BasicBlock,
    input_local: Local,
    acc_local: Local,
    array_length: i64,
    fold_op: BinOp,
    workgroup_size: u32,
    span: Span,
) {
    ctx.set_current_block(loop_start_bb);
    let loop_cond = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        loop_cond,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(loop_idx))),
            Box::new(int_constant(array_length, span)),
        ),
        span,
    );

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(loop_cond)),
            targets: vec![(Discriminant::bool_true(), loop_body_bb)],
            otherwise: loop_exit_bb,
        },
        span,
    ));

    ctx.set_current_block(loop_body_bb);
    let mut elem_place = Place::new(input_local);
    elem_place
        .projection
        .push(crate::mir::PlaceElem::Index(loop_idx));
    let elem_op = Operand::Copy(elem_place);

    push_assign(
        ctx,
        acc_local,
        Rvalue::BinaryOp(
            fold_op,
            Box::new(Operand::Copy(Place::new(acc_local))),
            Box::new(elem_op),
        ),
        span,
    );

    push_assign(
        ctx,
        loop_idx,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(loop_idx))),
            Box::new(int_constant(i64::from(workgroup_size), span)),
        ),
        span,
    );

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: loop_start_bb,
        },
        span,
    ));

    ctx.set_current_block(loop_exit_bb);
}

/// Grid-stride accumulation loop over input array.
#[allow(clippy::too_many_arguments)]
fn emit_grid_stride_loop(
    ctx: &mut LoweringContext,
    input_local: Local,
    acc_local: Local,
    thread_idx: Local,
    array_length: i64,
    fold_op: BinOp,
    workgroup_size: u32,
    span: Span,
) -> Result<Local, LoweringError> {
    let (loop_idx, loop_start_bb, loop_body_bb, loop_exit_bb) =
        setup_grid_stride_loop(ctx, thread_idx, span);

    emit_grid_stride_body(
        ctx,
        loop_idx,
        loop_start_bb,
        loop_body_bb,
        loop_exit_bb,
        input_local,
        acc_local,
        array_length,
        fold_op,
        workgroup_size,
        span,
    );

    Ok(loop_idx)
}

/// Store accumulated value to shared memory and barrier.
fn emit_sdata_store_and_barrier(
    ctx: &mut LoweringContext,
    sdata_local: Local,
    acc_local: Local,
    thread_idx: Local,
    span: Span,
) {
    let mut sdata_store_place = Place::new(sdata_local);
    sdata_store_place
        .projection
        .push(crate::mir::PlaceElem::Index(thread_idx));
    push_assign_place(
        ctx,
        sdata_store_place,
        Rvalue::Use(Operand::Copy(Place::new(acc_local))),
        span,
    );

    emit_workgroup_barrier(ctx, span);
}

/// Setup stride and branch to tree-reduction loop.
fn setup_tree_loop(
    ctx: &mut LoweringContext,
    span: Span,
) -> (
    Local,
    crate::mir::BasicBlock,
    crate::mir::BasicBlock,
    crate::mir::BasicBlock,
) {
    let stride = ctx.push_local("s".to_string(), Type::new(TypeKind::Int, span), span);
    push_assign(ctx, stride, Rvalue::Use(int_constant(128, span)), span);

    let tree_loop_start = ctx.new_basic_block();
    let tree_loop_body = ctx.new_basic_block();
    let tree_loop_exit = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: tree_loop_start,
        },
        span,
    ));

    (stride, tree_loop_start, tree_loop_body, tree_loop_exit)
}

/// Compute sdata[thread_idx] = sdata[thread_idx] OP sdata[thread_idx + stride].
fn compute_tree_reduction_result(
    ctx: &mut LoweringContext,
    sdata_local: Local,
    thread_idx: Local,
    stride: Local,
    elem_ty: &Type,
    fold_op: BinOp,
    span: Span,
) {
    let other_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    push_assign(
        ctx,
        other_idx,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_idx))),
            Box::new(Operand::Copy(Place::new(stride))),
        ),
        span,
    );

    let other_val = ctx.push_temp(elem_ty.clone(), span);
    let mut other_sdata_place = Place::new(sdata_local);
    other_sdata_place
        .projection
        .push(crate::mir::PlaceElem::Index(other_idx));
    push_assign(
        ctx,
        other_val,
        Rvalue::Use(Operand::Copy(other_sdata_place)),
        span,
    );

    let mut my_indexed_place = Place::new(sdata_local);
    my_indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(thread_idx));

    let result_val = ctx.push_temp(elem_ty.clone(), span);
    push_assign(
        ctx,
        result_val,
        Rvalue::BinaryOp(
            fold_op,
            Box::new(Operand::Copy(my_indexed_place)),
            Box::new(Operand::Copy(Place::new(other_val))),
        ),
        span,
    );

    let mut result_place = Place::new(sdata_local);
    result_place
        .projection
        .push(crate::mir::PlaceElem::Index(thread_idx));
    push_assign_place(
        ctx,
        result_place,
        Rvalue::Use(Operand::Copy(Place::new(result_val))),
        span,
    );
}

/// Emit tree-reduction iteration body (one halving step).
#[allow(clippy::too_many_arguments)]
fn emit_tree_step(
    ctx: &mut LoweringContext,
    sdata_local: Local,
    thread_idx: Local,
    stride: Local,
    elem_ty: &Type,
    fold_op: BinOp,
    span: Span,
) -> crate::mir::BasicBlock {
    let in_range = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        in_range,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(thread_idx))),
            Box::new(Operand::Copy(Place::new(stride))),
        ),
        span,
    );

    let then_bb = ctx.new_basic_block();
    let then_exit = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(in_range)),
            targets: vec![(Discriminant::bool_true(), then_bb)],
            otherwise: then_exit,
        },
        span,
    ));

    ctx.set_current_block(then_bb);
    compute_tree_reduction_result(ctx, sdata_local, thread_idx, stride, elem_ty, fold_op, span);

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: then_exit },
        span,
    ));

    then_exit
}

/// Tree-reduction loop reducing sdata across workgroup.
fn emit_tree_reduction_loop(
    ctx: &mut LoweringContext,
    sdata_local: Local,
    thread_idx: Local,
    elem_ty: &Type,
    fold_op: BinOp,
    span: Span,
) -> Result<Local, LoweringError> {
    let (stride, tree_loop_start, tree_loop_body, tree_loop_exit) = setup_tree_loop(ctx, span);

    ctx.set_current_block(tree_loop_start);
    let stride_cond = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        stride_cond,
        Rvalue::BinaryOp(
            BinOp::Gt,
            Box::new(Operand::Copy(Place::new(stride))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(stride_cond)),
            targets: vec![(Discriminant::bool_true(), tree_loop_body)],
            otherwise: tree_loop_exit,
        },
        span,
    ));

    ctx.set_current_block(tree_loop_body);
    let then_exit = emit_tree_step(ctx, sdata_local, thread_idx, stride, elem_ty, fold_op, span);

    ctx.set_current_block(then_exit);
    emit_workgroup_barrier(ctx, span);

    push_assign(
        ctx,
        stride,
        Rvalue::BinaryOp(
            BinOp::Shr,
            Box::new(Operand::Copy(Place::new(stride))),
            Box::new(int_constant(1, span)),
        ),
        span,
    );

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: tree_loop_start,
        },
        span,
    ));

    ctx.set_current_block(tree_loop_exit);
    Ok(stride)
}

/// Lane-0 writes reduced result to output buffer.
fn emit_lane_zero_output_write(
    ctx: &mut LoweringContext,
    output_local: Local,
    sdata_local: Local,
    thread_idx: Local,
    span: Span,
) {
    let is_lane_zero_out = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        is_lane_zero_out,
        Rvalue::BinaryOp(
            BinOp::Eq,
            Box::new(Operand::Copy(Place::new(thread_idx))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );

    let output_write_bb = ctx.new_basic_block();
    let output_done_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(is_lane_zero_out)),
            targets: vec![(Discriminant::bool_true(), output_write_bb)],
            otherwise: output_done_bb,
        },
        span,
    ));

    ctx.set_current_block(output_write_bb);

    let zero_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    push_assign(ctx, zero_idx, Rvalue::Use(int_constant(0, span)), span);

    let mut sdata_result_place = Place::new(sdata_local);
    sdata_result_place
        .projection
        .push(crate::mir::PlaceElem::Index(zero_idx));

    let mut output_place = Place::new(output_local);
    output_place
        .projection
        .push(crate::mir::PlaceElem::Index(zero_idx));

    push_assign_place(
        ctx,
        output_place,
        Rvalue::Use(Operand::Copy(sdata_result_place)),
        span,
    );

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: output_done_bb,
        },
        span,
    ));

    ctx.set_current_block(output_done_bb);
}

/// Build a GPU tree-reduction kernel body.
fn build_gpu_reduce_kernel(
    parent: &mut LoweringContext,
    obj_ty: &Type,
    array_length: i64,
    fold_op: BinOp,
    span: Span,
) -> Result<Body, LoweringError> {
    let workgroup_size = GPU_REDUCE_BLOCK_SIZE;
    let arg_count = 3;

    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));

    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([workgroup_size, 1, 1]),
        grid_size: Some([1, 1, 1]),
        logical_extent: None,
        required_capabilities: Vec::new(),
        is_frame_step: false,
    }));

    kernel.out_params = vec![false, false, true];

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    let (input_local, init_local, output_local, sdata_local, thread_idx, ws) =
        setup_reduce_kernel_params(&mut ctx, obj_ty, span)?;

    let elem_ty = extract_element_type(obj_ty)?;
    let acc_local = emit_acc_init(&mut ctx, init_local, thread_idx, fold_op, &elem_ty, span)?;

    let loop_idx = emit_grid_stride_loop(
        &mut ctx,
        input_local,
        acc_local,
        thread_idx,
        array_length,
        fold_op,
        ws,
        span,
    )?;

    emit_sdata_store_and_barrier(&mut ctx, sdata_local, acc_local, thread_idx, span);

    let stride =
        emit_tree_reduction_loop(&mut ctx, sdata_local, thread_idx, &elem_ty, fold_op, span)?;

    emit_lane_zero_output_write(&mut ctx, output_local, sdata_local, thread_idx, span);

    ctx.push_statement(MirStatement {
        kind: MirStatementKind::StorageDead(Place::new(stride)),
        span,
    });
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::StorageDead(Place::new(loop_idx)),
        span,
    });
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::StorageDead(Place::new(acc_local)),
        span,
    });
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::StorageDead(Place::new(sdata_local)),
        span,
    });

    ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));

    Ok(ctx.body)
}

/// Emit a workgroup barrier.
fn emit_workgroup_barrier(ctx: &mut LoweringContext, span: Span) {
    let void_temp = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(void_temp),
            Rvalue::GpuIntrinsic(GpuIntrinsic::SyncThreads),
        ),
        span,
    });
}

/// Setup 1-element output buffer and return the handle.
fn setup_reduce_output_buffer(
    ctx: &mut LoweringContext,
    elem_ty: &Type,
    span: Span,
) -> Result<(Local, crate::mir::body::DeviceHandleId), LoweringError> {
    let output_array_ty = Type::new(
        TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![
                crate::ast::expression::Expression {
                    id: 0,
                    node: ExpressionKind::Type(Box::new(elem_ty.clone()), false),
                    span,
                },
                crate::ast::expression::Expression {
                    id: 0,
                    node: ExpressionKind::Literal(Literal::Integer(
                        crate::ast::literal::IntegerLiteral::I64(1),
                    )),
                    span,
                },
            ]),
        ),
        span,
    );

    let output_local = ctx.push_local("_reduce_out".to_string(), output_array_ty, span);
    ctx.body.local_decls[output_local.0].residency = BindingResidency::Gpu;
    let handle_id = crate::mir::body::DeviceHandleId::fresh();
    ctx.body.local_decls[output_local.0].device_handle = Some(handle_id);

    let zero_elem = identity_for_op(BinOp::Add, elem_ty);
    push_assign(
        ctx,
        output_local,
        Rvalue::Aggregate(AggregateKind::Array, vec![zero_elem]),
        span,
    );

    Ok((output_local, handle_id))
}

/// Assemble GpuLaunchArgs from input/output buffers and init scalar.
#[allow(clippy::too_many_arguments)]
fn assemble_reduce_launch_args(
    ctx: &mut LoweringContext,
    receiver_local: Local,
    output_local: Local,
    init_op: Operand,
    handle_id: crate::mir::body::DeviceHandleId,
    receiver_ty: &Type,
    elem_ty: &Type,
    span: Span,
) -> Result<(Vec<Operand>, GpuLaunchArgs), LoweringError> {
    let buffer_ops = vec![
        Operand::Copy(Place::new(receiver_local)),
        Operand::Copy(Place::new(output_local)),
    ];

    let init_local = ctx.push_temp(elem_ty.clone(), span);
    push_assign(ctx, init_local, Rvalue::Use(init_op), span);
    let scalar_ops = vec![Operand::Copy(Place::new(init_local))];

    let arg_handles = vec![
        ctx.body.local_decls[receiver_local.0].device_handle,
        Some(handle_id),
    ];
    let output_ty = ctx.body.local_decls[output_local.0].ty.clone();
    let arg_int_narrow = vec![
        needs_int_narrowing(receiver_ty),
        needs_int_narrowing(&output_ty),
    ];

    let arg_read_only = vec![true, false];
    let launch_args = GpuLaunchArgs::new(buffer_ops, arg_handles, arg_read_only, arg_int_narrow)
        .map_err(|e| LoweringError::custom(e.to_string(), span, None))?;

    Ok((scalar_ops, launch_args))
}

/// Emit GpuLaunch terminator and conditional readback.
#[allow(clippy::too_many_arguments)]
fn emit_reduce_gpu_launch(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    scalar_ops: Vec<Operand>,
    launch_args: GpuLaunchArgs,
    output_local: Local,
    handle_id: crate::mir::body::DeviceHandleId,
    dest_is_gpu_resident: bool,
    span: Span,
) {
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);
    let void_ty = Type::new(TypeKind::Void, span);
    let one_op = int_constant(1, span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty.clone()),
            vec![one_op.clone(), one_op.clone(), one_op.clone()],
        ),
        span,
    );

    let block_size = GPU_REDUCE_BLOCK_SIZE;
    let block_size_i64 = i64::from(block_size);
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty),
            vec![int_constant(block_size_i64, span), one_op.clone(), one_op],
        ),
        span,
    );

    let kernel_op = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier(kernel_name.to_string()),
    }));

    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            launch_args,
            scalar_args: scalar_ops,
            uniform_bound_x: None,
            uniform_bound_y: None,
            uniform_bound_z: None,
            uniform_start_x: None,
            uniform_start_y: None,
            uniform_start_z: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
    // The grid and block dimensions are allocations of their own, read by the
    // launch and dead once it returns.
    ctx.emit_temp_drop(grid_local, 0, span);
    ctx.emit_temp_drop(block_local, 0, span);

    if !dest_is_gpu_resident {
        emit_void_runtime_call(
            ctx,
            READBACK_FN,
            vec![
                handle_operand(handle_id, span),
                Operand::Copy(Place::new(output_local)),
            ],
            span,
        );
    }
}

/// Extract the result operand from the output buffer.
fn extract_reduce_result(ctx: &mut LoweringContext, output_local: Local, span: Span) -> Operand {
    let zero_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    push_assign(ctx, zero_idx, Rvalue::Use(int_constant(0, span)), span);
    let mut output_elem_place = Place::new(output_local);
    output_elem_place
        .projection
        .push(crate::mir::PlaceElem::Index(zero_idx));
    Operand::Copy(output_elem_place)
}

/// Emit the GpuLaunch terminator for the reduction kernel.
/// Returns (reduced_result_operand, output_local_backing_1element_buffer, device_handle_id).
/// The caller must emit `StorageDead` for the output_local (for host-resident
/// destinations) or transfer its device handle to the destination binding
/// (for gpu-resident destinations) to avoid leaks or handle mismatches.
///
/// Gpu-resident reduce: if `dest_is_gpu_resident` is true, the 1-element
/// output buffer remains gpu-resident and is NOT eagerly read back to the host.
/// The buffer persists with its device handle, allowing the result to remain
/// on-GPU for subsequent operations. Cross-residency assignment (`let h = gpu_s`)
/// will trigger the readback then.
fn emit_gpu_reduce_launch(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    receiver_local: Local,
    init_op: Operand,
    span: Span,
    dest_is_gpu_resident: bool,
) -> Result<(Operand, crate::mir::body::DeviceHandleId), LoweringError> {
    let receiver_ty = ctx.body.local_decls[receiver_local.0].ty.clone();
    let elem_ty = extract_element_type(&receiver_ty)?;

    let (output_local, handle_id) = setup_reduce_output_buffer(ctx, &elem_ty, span)?;

    let (scalar_ops, launch_args) = assemble_reduce_launch_args(
        ctx,
        receiver_local,
        output_local,
        init_op,
        handle_id,
        &receiver_ty,
        &elem_ty,
        span,
    )?;

    emit_reduce_gpu_launch(
        ctx,
        kernel_name,
        scalar_ops,
        launch_args,
        output_local,
        handle_id,
        dest_is_gpu_resident,
        span,
    );

    let result_op = extract_reduce_result(ctx, output_local, span);
    Ok((result_op, handle_id))
}
