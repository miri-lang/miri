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
    Local, LocalDecl, Operand, Place, Rvalue, Statement as MirStatement,
    StatementKind as MirStatementKind, StorageClass, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::expression::lower_expression;
use super::forall_gpu::{compute_thread_index, needs_int_narrowing, FORALL_GPU_BLOCK_SIZE};

/// Runtime entry that fences outstanding device writes and copies a
/// `gpu`-resident buffer back to its host array.
const READBACK_FN: &str = "miri_gpu_readback";

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
    let kernel_name = format!("miri_gpu_reduce_{}", call_expr_id);
    let kernel_body = build_gpu_reduce_kernel(ctx, obj_ty, array_length, fold_op, *span)?;

    ctx.lambda_bodies.push(LambdaInfo {
        name: kernel_name.clone(),
        body: kernel_body,
        captures: Vec::new(),
    });

    // Emit the GpuLaunch terminator. Returns an operand reading the readback
    // result (the reduced scalar) out of the 1-element output buffer, plus the
    // local backing that buffer (so it can be freed below).
    let (output_op, output_local) =
        emit_gpu_reduce_launch(ctx, &kernel_name, receiver_local, init_op, *span)?;

    // Honor the caller's destination: the call lowering passes `dest` for
    // `let sum = a.reduce(...)` and expects the intrinsic to write it. Mirror
    // the `element_at` intrinsic — write the result into `dest` (or a temp) and
    // return a Copy of it. Without this the binding never receives the value.
    let elem_ty = extract_element_type(obj_ty)?;
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

    // Free the 1-element output array now that its element has been copied into
    // the destination. `_reduce_out` is a managed heap array created without a
    // `StorageLive`, so Perceus only `DecRef`s it at this explicit `StorageDead`;
    // omitting it leaks the buffer on every reduce call.
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::StorageDead(Place::new(output_local)),
        span: *span,
    });

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

/// Helper to create an integer constant operand.
fn int_constant(value: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(crate::ast::literal::IntegerLiteral::I64(value)),
    }))
}

/// Helper to assign a value to a local.
fn push_assign(ctx: &mut LoweringContext, local: Local, rvalue: Rvalue, span: Span) {
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(Place::new(local), rvalue),
        span,
    });
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

/// Build a GPU tree-reduction kernel body.
#[allow(clippy::too_many_arguments)]
fn build_gpu_reduce_kernel(
    parent: &mut LoweringContext,
    obj_ty: &Type,
    array_length: i64,
    fold_op: BinOp,
    span: Span,
) -> Result<Body, LoweringError> {
    let workgroup_size = FORALL_GPU_BLOCK_SIZE;
    // 3 params: input array, init scalar, output array (1-element, read_write)
    let arg_count = 3;

    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));

    // Grid is 1x1x1 for reduction (single workgroup).
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([workgroup_size, 1, 1]),
        grid_size: Some([1, 1, 1]),
        required_capabilities: Vec::new(),
        is_frame_step: false,
    }));

    // out_params: input and init are read-only; output is read_write
    kernel.out_params = vec![false, false, true];

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    // Add parameters: input array (read-only), init scalar (uniform), output array (read_write).
    let input_local = ctx.push_param("input".to_string(), obj_ty.clone(), span);
    ctx.body.local_decls[input_local.0].storage_class = StorageClass::GpuGlobal;

    let elem_ty = extract_element_type(obj_ty)?;
    let init_local = ctx.push_param("init".to_string(), elem_ty.clone(), span);
    ctx.body.local_decls[init_local.0].storage_class = StorageClass::UniformBuffer;

    // Output: 1-element array to hold the reduced result (read_write).
    let output_local = ctx.push_param("output".to_string(), obj_ty.clone(), span);
    ctx.body.local_decls[output_local.0].storage_class = StorageClass::GpuGlobal;

    // Create workgroup-shared array sized to BLOCK_SIZE (not input length).
    // The shared array holds one slot per thread in the workgroup (256 threads).
    // Build Array<T, 256> type manually using Custom with type_args.
    let sdata_array_ty = Type::new(
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

    // Get thread index.
    let thread_idx = compute_thread_index(&mut ctx, Dimension::X, span);

    // Accumulator: lane 0 gets init, others get identity.
    let identity_literal = identity_for_op(fold_op, &elem_ty);
    let acc_local = ctx.push_local("acc".to_string(), elem_ty.clone(), span);
    let is_lane_zero = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);

    // is_lane_zero = (thread_idx == 0)
    push_assign(
        &mut ctx,
        is_lane_zero,
        Rvalue::BinaryOp(
            BinOp::Eq,
            Box::new(Operand::Copy(Place::new(thread_idx))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );

    // acc = is_lane_zero ? init : IDENTITY
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

    // Then branch: acc = init
    ctx.set_current_block(then_bb);
    push_assign(
        &mut ctx,
        acc_local,
        Rvalue::Use(Operand::Copy(Place::new(init_local))),
        span,
    );
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: merge_bb },
        span,
    ));

    // Else branch: acc = identity
    ctx.set_current_block(else_bb);
    push_assign(&mut ctx, acc_local, Rvalue::Use(identity_literal), span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: merge_bb },
        span,
    ));

    // Merge: Continue with grid-stride loop.
    ctx.set_current_block(merge_bb);

    // Grid-stride loop: accumulate over input.
    let loop_idx = ctx.push_local("i".to_string(), Type::new(TypeKind::Int, span), span);
    push_assign(
        &mut ctx,
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

    // Loop condition: i < array_length
    ctx.set_current_block(loop_start_bb);
    let loop_cond = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        &mut ctx,
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

    // Loop body: acc = acc OP input[i]
    ctx.set_current_block(loop_body_bb);
    let mut elem_place = Place::new(input_local);
    elem_place
        .projection
        .push(crate::mir::PlaceElem::Index(loop_idx));
    let elem_op = Operand::Copy(elem_place);

    push_assign(
        &mut ctx,
        acc_local,
        Rvalue::BinaryOp(
            fold_op,
            Box::new(Operand::Copy(Place::new(acc_local))),
            Box::new(elem_op),
        ),
        span,
    );

    // Increment loop index: i += workgroup_size
    push_assign(
        &mut ctx,
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

    // Loop exit: write accumulated value to sdata[local_id.x]
    ctx.set_current_block(loop_exit_bb);

    // BUG FIX 2: Store acc to sdata[thread_idx], not a no-op self-assign.
    let mut sdata_store_place = Place::new(sdata_local);
    sdata_store_place
        .projection
        .push(crate::mir::PlaceElem::Index(thread_idx));
    push_assign_place(
        &mut ctx,
        sdata_store_place,
        Rvalue::Use(Operand::Copy(Place::new(acc_local))),
        span,
    );

    // Write barrier.
    emit_workgroup_barrier(&mut ctx, span);

    // Tree reduction loop.
    let stride = ctx.push_local("s".to_string(), Type::new(TypeKind::Int, span), span);
    push_assign(&mut ctx, stride, Rvalue::Use(int_constant(128, span)), span); // workgroup_size / 2

    let tree_loop_start = ctx.new_basic_block();
    let tree_loop_body = ctx.new_basic_block();
    let tree_loop_exit = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto {
            target: tree_loop_start,
        },
        span,
    ));

    // Tree loop condition: s > 0
    ctx.set_current_block(tree_loop_start);
    let stride_cond = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        &mut ctx,
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

    // Tree loop body: if (thread_idx < s) sdata[lid] = sdata[lid] OP sdata[lid + s]
    ctx.set_current_block(tree_loop_body);
    let in_range = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        &mut ctx,
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
    let other_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    push_assign(
        &mut ctx,
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
        &mut ctx,
        other_val,
        Rvalue::Use(Operand::Copy(other_sdata_place)),
        span,
    );

    let mut my_indexed_place = Place::new(sdata_local);
    my_indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(thread_idx));

    let result_val = ctx.push_temp(elem_ty, span);
    push_assign(
        &mut ctx,
        result_val,
        Rvalue::BinaryOp(
            fold_op,
            Box::new(Operand::Copy(my_indexed_place)),
            Box::new(Operand::Copy(Place::new(other_val))),
        ),
        span,
    );

    // Assign the result to sdata[thread_idx]
    let mut result_place = Place::new(sdata_local);
    result_place
        .projection
        .push(crate::mir::PlaceElem::Index(thread_idx));
    push_assign_place(
        &mut ctx,
        result_place,
        Rvalue::Use(Operand::Copy(Place::new(result_val))),
        span,
    );

    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: then_exit },
        span,
    ));

    ctx.set_current_block(then_exit);

    // Barrier.
    emit_workgroup_barrier(&mut ctx, span);

    // Stride >>= 1
    push_assign(
        &mut ctx,
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

    // Tree loop exit: lane 0 writes the result to output[0].
    ctx.set_current_block(tree_loop_exit);

    // BUG FIX 3+4: Emit output buffer write from lane 0.
    let is_lane_zero_out = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        &mut ctx,
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

    // Lane 0: output[0] = sdata[0]
    ctx.set_current_block(output_write_bb);

    // Create a temporary local with value 0 for indexing.
    let zero_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    push_assign(&mut ctx, zero_idx, Rvalue::Use(int_constant(0, span)), span);

    let mut sdata_result_place = Place::new(sdata_local);
    sdata_result_place
        .projection
        .push(crate::mir::PlaceElem::Index(zero_idx));

    let mut output_place = Place::new(output_local);
    output_place
        .projection
        .push(crate::mir::PlaceElem::Index(zero_idx));

    push_assign_place(
        &mut ctx,
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

    // Emit StorageDead for all locals that have StorageLive before return.
    // These are: sdata_local, acc_local, loop_idx, stride.
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

/// Emit the GpuLaunch terminator for the reduction kernel.
/// Returns the operand that reads the reduced result, and the local backing the
/// 1-element output array. The caller must emit `StorageDead` for that local
/// after consuming the operand so Perceus frees the host buffer (it is created
/// with `push_local`, which emits no `StorageLive`, so without an explicit
/// `StorageDead` Perceus would never `DecRef` it — a per-call leak).
fn emit_gpu_reduce_launch(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    receiver_local: Local,
    init_op: Operand,
    span: Span,
) -> Result<(Operand, Local), LoweringError> {
    let receiver_ty = ctx.body.local_decls[receiver_local.0].ty.clone();
    let elem_ty = extract_element_type(&receiver_ty)?;

    // Create a 1-element output buffer on GPU to hold the reduced result.
    // BUG FIX 3+4: Wire output buffer into kernel.
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
    // Allocate a fresh DeviceHandleId for the output buffer.
    let handle_id = crate::mir::body::DeviceHandleId::fresh();
    ctx.body.local_decls[output_local.0].device_handle = Some(handle_id);

    // Materialize a real 1-element array backing for the output. Without an
    // initializer the local's MiriArrayHeader is null, so building the launch
    // buffer descriptor (and the readback) dereferences a null pointer and
    // segfaults. A zero-seeded single element gives the buffer valid
    // data/len bytes; the kernel overwrites element 0 with the reduced result.
    let zero_elem = identity_for_op(BinOp::Add, &elem_ty);
    push_assign(
        ctx,
        output_local,
        Rvalue::Aggregate(AggregateKind::Array, vec![zero_elem]),
        span,
    );

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

    let block_size_i64 = i64::from(FORALL_GPU_BLOCK_SIZE);
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

    // Args: input (read-only), output (read_write)
    let buffer_ops = vec![
        Operand::Copy(Place::new(receiver_local)),
        Operand::Copy(Place::new(output_local)),
    ];

    // FIX 1: Materialize init_op into a fresh local to satisfy Cranelift's requirement
    // that all GpuLaunch operands must be Copy/Move of projection-free locals.
    let init_local = ctx.push_temp(elem_ty, span);
    push_assign(ctx, init_local, Rvalue::Use(init_op), span);
    let scalar_ops = vec![Operand::Copy(Place::new(init_local))];

    let arg_handles = vec![
        ctx.body.local_decls[receiver_local.0].device_handle,
        Some(handle_id),
    ];
    // Int (i64) arrays are narrowed to i32 on upload and widened back on
    // readback — the kernel operates on `array<i32>`. Mirror the forall path so
    // the host i64 element width matches the device i32 width; without this the
    // kernel reads the i64 host bytes as i32 and computes garbage.
    let output_ty = ctx.body.local_decls[output_local.0].ty.clone();
    let arg_int_narrow = vec![
        needs_int_narrowing(&receiver_ty),
        needs_int_narrowing(&output_ty),
    ];

    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            args: buffer_ops,
            arg_handles,
            arg_read_only: vec![true, false], // input is read-only, output is read_write
            arg_int_narrow,
            scalar_args: scalar_ops,
            uniform_bound_x: None,
            uniform_bound_y: None,
            uniform_bound_z: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);

    // MILESTONE 2 FIX: Emit readback to fence device work and copy result to host array.
    // After GpuLaunch, the device buffer contains the result, but the host array is
    // not yet updated. The readback call synchronizes the host array with the device buffer.
    emit_void_runtime_call(
        ctx,
        READBACK_FN,
        vec![
            handle_operand(handle_id, span),
            Operand::Copy(Place::new(output_local)),
        ],
        span,
    );

    // Return an operand that reads from the output buffer's first element.
    // After the readback fence, the host array `_reduce_out` is synchronized with
    // the device buffer, so `output[0]` holds the reduced result.
    // `PlaceElem::Index` indexes by a *local holding the index value*, so the
    // constant 0 must be materialized into a bare local (using `Local(0)` would
    // index by the return slot's contents, not by zero).
    let zero_idx = ctx.push_temp(Type::new(TypeKind::Int, span), span);
    push_assign(ctx, zero_idx, Rvalue::Use(int_constant(0, span)), span);
    let mut output_elem_place = Place::new(output_local);
    output_elem_place
        .projection
        .push(crate::mir::PlaceElem::Index(zero_idx));
    Ok((Operand::Copy(output_elem_place), output_local))
}
