// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `gpu for <ident> in <range>` loops.
//!
//! Extracts the loop body into a synthesized anonymous `gpu fn` kernel and
//! emits a `TerminatorKind::GpuLaunch` at the call site with a fixed
//! workgroup size of 256 and a grid sized from the range length.
//!
//! Baseline restrictions (M6.5 Task 4):
//! - Range start and end must be Int literals (e.g. `0..256`). Variable
//!   bounds require scalar uniform/push-constant support in the WGSL
//!   backend, which is a follow-up.
//! - Only one loop variable is accepted.
//! - The body may reference outer-scope variables whose types are GPU
//!   buffers (`Array<T, N>`, `GpuArray<T, N>`); all such captures are
//!   exposed as read-write storage buffers.

use std::collections::HashSet;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, DIM3_TYPE_NAME};
use crate::ast::RangeExpressionType;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::backend::{BackendMetadata, GpuBodyMetadata};
use crate::mir::body::{BindingResidency, DeviceHandleId};
use crate::mir::lambda::LambdaInfo;
use crate::mir::{
    AggregateKind, BinOp, Body, Constant, Dimension, Discriminant, ExecutionModel, GpuIntrinsic,
    Local, LocalDecl, Operand, Place, Rvalue, Statement as MirStatement,
    StatementKind as MirStatementKind, StorageClass, Terminator, TerminatorKind,
};

use super::context::LoweringContext;
use super::statement::lower_statement;

const GPU_FOR_BLOCK_SIZE: u32 = 256;

/// Lowers a `gpu for` loop into a synthesized kernel + `GpuLaunch`.
pub fn lower_gpu_for(
    ctx: &mut LoweringContext,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    if decls.len() != 1 {
        return Err(LoweringError::unsupported_expression(
            "gpu for: exactly one loop variable is supported".to_string(),
            *span,
        ));
    }
    let loop_var_name = decls[0].name.clone();
    let (start, end, range_type) = extract_literal_range(iterable, *span)?;
    let length = compute_range_length(start, end, range_type, *span)?;

    let capture_names = collect_outer_captures(body, &loop_var_name, ctx);
    let mut captures: Vec<CaptureInfo> = Vec::with_capacity(capture_names.len());
    for name in capture_names {
        let Some(&outer_local) = ctx.variable_map.get(name.as_str()) else {
            return Err(LoweringError::unsupported_expression(
                format!(
                    "gpu for: captured variable '{}' is not visible at the loop site",
                    name
                ),
                *span,
            ));
        };
        let ty = ctx.body.local_decls[outer_local.0].ty.clone();
        if !is_gpu_buffer_capture(&ty.kind) {
            return Err(LoweringError::unsupported_expression(
                format!(
                    "gpu for: capture '{}' has non-buffer type; baseline only accepts `Array<T, N>`, `[T; N]`, or `GpuArray<T>` captures (scalar/string/collection captures need uniform/push-constant lowering, follow-up)",
                    name
                ),
                *span,
            ));
        }
        // Only a gpu-resident buffer may be marshaled as a kernel storage
        // binding. Host-resident buffer captures are rejected upstream with a
        // source-cited §6.4 diagnostic, so this guard is unreachable in
        // well-typed programs; it keeps MIR lowering from ever uploading a
        // host buffer implicitly (GPU_DRAFT §10.5 — no silent promotion).
        if ctx.body.local_decls[outer_local.0].residency != BindingResidency::Gpu {
            return Err(LoweringError::unsupported_expression(
                format!("gpu for: capture '{}' is not gpu-resident", name),
                *span,
            ));
        }
        captures.push(CaptureInfo {
            name,
            ty,
            outer_local,
        });
    }

    // Use the AST statement's globally-unique id so kernel names cannot
    // collide between different `gpu for` sites (across functions and
    // across files). Earlier mangling used `(basic_blocks.len(),
    // lambda_bodies.len())` which is local to the enclosing function and
    // collides at link time between functions whose loops happen to lower
    // at the same local indices.
    let kernel_name = format!("miri_gpu_for_{}", stmt_id);
    let kernel_body =
        build_kernel_body(ctx, &captures, &loop_var_name, start, length, body, *span)?;
    ctx.lambda_bodies.push(LambdaInfo {
        name: kernel_name.clone(),
        body: kernel_body,
        captures: Vec::new(),
    });

    emit_gpu_launch(ctx, &kernel_name, length, &captures, *span);
    Ok(())
}

struct CaptureInfo {
    name: String,
    ty: Type,
    outer_local: Local,
}

/// Returns `true` for types whose runtime representation is a host-side
/// `MiriArray`-shaped buffer that the GPU dispatcher can marshal as a
/// storage binding. Scalars and non-buffer managed types pass the broader
/// `is_gpu_compatible` predicate (used for kernel-body type checking) but
/// would be misinterpreted as MiriArray pointers by `gpu_launch::translate`.
///
/// `GpuArray<T, N>` is intentionally **excluded** here even though it is
/// `is_gpu_compatible`: it is a stdlib class wrapping an `Array` field, so
/// the local stores a class payload pointer whose offset 0 is either a
/// vtable pointer (if the class has one) or the inner `data` field — not
/// a `MiriArray` header. Routing it through the dispatcher would silently
/// read garbage as the device data pointer / length. Re-enable once the
/// dispatcher unwraps the class indirection (M8a follow-up).
fn is_gpu_buffer_capture(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) => true,
        TypeKind::Custom(name, _) => {
            BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
        }
        // Listed explicitly so a new `TypeKind` variant must be classified
        // here on purpose (PRINCIPLES §3.5). Every kind below currently
        // ships through the kernel body but cannot be marshaled as a
        // storage buffer by the dispatcher.
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Generic(_, _, _)
        | TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Meta(_)
        | TypeKind::RawPtr
        | TypeKind::Identifier
        | TypeKind::Function(_) => false,
    }
}

fn extract_literal_range(
    iterable: &Expression,
    span: Span,
) -> Result<(i64, i64, RangeExpressionType), LoweringError> {
    let ExpressionKind::Range(start_box, Some(end_box), range_type) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu for: iterable must be a bounded numeric range like '0..n'".to_string(),
            span,
        ));
    };
    let start = read_int_literal(start_box, span)?;
    let end = read_int_literal(end_box, span)?;
    Ok((start, end, range_type.clone()))
}

fn read_int_literal(expr: &Expression, span: Span) -> Result<i64, LoweringError> {
    if let ExpressionKind::Literal(Literal::Integer(int_lit)) = &expr.node {
        Ok(int_literal_to_i64(int_lit))
    } else {
        Err(LoweringError::unsupported_expression(
            "gpu for: range bounds must be Int literals in the baseline (variable bounds are a follow-up)"
                .to_string(),
            span,
        ))
    }
}

fn int_literal_to_i64(lit: &IntegerLiteral) -> i64 {
    match *lit {
        IntegerLiteral::I8(v) => v as i64,
        IntegerLiteral::I16(v) => v as i64,
        IntegerLiteral::I32(v) => v as i64,
        IntegerLiteral::I64(v) => v,
        IntegerLiteral::I128(v) => v as i64,
        IntegerLiteral::U8(v) => v as i64,
        IntegerLiteral::U16(v) => v as i64,
        IntegerLiteral::U32(v) => v as i64,
        IntegerLiteral::U64(v) => v as i64,
        IntegerLiteral::U128(v) => v as i64,
    }
}

fn compute_range_length(
    start: i64,
    end: i64,
    range_type: RangeExpressionType,
    span: Span,
) -> Result<i64, LoweringError> {
    let raw = match range_type {
        RangeExpressionType::Exclusive => end.checked_sub(start),
        RangeExpressionType::Inclusive => end.checked_sub(start).and_then(|d| d.checked_add(1)),
        RangeExpressionType::IterableObject => {
            return Err(LoweringError::unsupported_expression(
                "gpu for: iterable-object ranges are not supported (use 'a..b')".to_string(),
                span,
            ));
        }
    };
    let raw = raw.ok_or_else(|| {
        LoweringError::unsupported_expression(
            "gpu for: range bounds overflow i64".to_string(),
            span,
        )
    })?;
    if raw <= 0 {
        return Err(LoweringError::unsupported_expression(
            "gpu for: range length must be positive".to_string(),
            span,
        ));
    }
    Ok(raw)
}

fn collect_outer_captures(body: &Statement, loop_var: &str, ctx: &LoweringContext) -> Vec<String> {
    let mut bound: HashSet<String> = HashSet::new();
    bound.insert(loop_var.to_string());

    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();

    visit_stmt(body, &mut bound, ctx, &mut seen, &mut ordered);
    ordered
}

fn visit_stmt(
    stmt: &Statement,
    bound: &mut HashSet<String>,
    ctx: &LoweringContext,
    seen: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            let scope_snapshot = bound.clone();
            for s in stmts {
                visit_stmt(s, bound, ctx, seen, ordered);
            }
            *bound = scope_snapshot;
        }
        StatementKind::Expression(expr) => visit_expr(expr, bound, ctx, seen, ordered),
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    visit_expr(init, bound, ctx, seen, ordered);
                }
                bound.insert(d.name.clone());
            }
        }
        StatementKind::Return(Some(e)) => visit_expr(e, bound, ctx, seen, ordered),
        StatementKind::Return(None) => {}
        StatementKind::If(cond, then_branch, else_branch, _) => {
            visit_expr(cond, bound, ctx, seen, ordered);
            visit_stmt(then_branch, bound, ctx, seen, ordered);
            if let Some(eb) = else_branch {
                visit_stmt(eb, bound, ctx, seen, ordered);
            }
        }
        StatementKind::While(cond, body, _) => {
            visit_expr(cond, bound, ctx, seen, ordered);
            visit_stmt(body, bound, ctx, seen, ordered);
        }
        StatementKind::For(inner_decls, iter, body)
        | StatementKind::GpuFor(inner_decls, iter, body) => {
            visit_expr(iter, bound, ctx, seen, ordered);
            let scope_snapshot = bound.clone();
            for d in inner_decls {
                bound.insert(d.name.clone());
            }
            visit_stmt(body, bound, ctx, seen, ordered);
            *bound = scope_snapshot;
        }
        // Listed explicitly so a new `StatementKind` variant cannot be
        // silently dropped from capture collection (PRINCIPLES §3.5, §5.4).
        // None of these shapes can introduce a captured outer-scope
        // identifier into a `gpu for` body: control-flow markers carry no
        // expression, and nested declarations open a fresh scope that the
        // GPU type check rejects anyway.
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
    }
}

fn visit_expr(
    expr: &Expression,
    bound: &mut HashSet<String>,
    ctx: &LoweringContext,
    seen: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => {
            if !bound.contains(name)
                && ctx.variable_map.contains_key(name.as_str())
                && !seen.contains(name)
            {
                seen.insert(name.clone());
                ordered.push(name.clone());
            }
        }
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            visit_expr(lhs, bound, ctx, seen, ordered);
            visit_expr(rhs, bound, ctx, seen, ordered);
        }
        ExpressionKind::Unary(_, inner) | ExpressionKind::Guard(_, inner) => {
            visit_expr(inner, bound, ctx, seen, ordered)
        }
        ExpressionKind::Call(callee, args) => {
            visit_expr(callee, bound, ctx, seen, ordered);
            for a in args {
                visit_expr(a, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Index(obj, idx) => {
            visit_expr(obj, bound, ctx, seen, ordered);
            visit_expr(idx, bound, ctx, seen, ordered);
        }
        ExpressionKind::Member(obj, prop) => {
            visit_expr(obj, bound, ctx, seen, ordered);
            visit_expr(prop, bound, ctx, seen, ordered);
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            visit_lhs(lhs, bound, ctx, seen, ordered);
            visit_expr(rhs, bound, ctx, seen, ordered);
        }
        ExpressionKind::Conditional(cond, then_e, else_opt, _) => {
            visit_expr(cond, bound, ctx, seen, ordered);
            visit_expr(then_e, bound, ctx, seen, ordered);
            if let Some(else_e) = else_opt {
                visit_expr(else_e, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Range(start, end, _) => {
            visit_expr(start, bound, ctx, seen, ordered);
            if let Some(end) = end {
                visit_expr(end, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Array(elems, _)
        | ExpressionKind::List(elems)
        | ExpressionKind::Tuple(elems)
        | ExpressionKind::Set(elems)
        | ExpressionKind::FormattedString(elems) => {
            for e in elems {
                visit_expr(e, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Map(entries) => {
            for (k, v) in entries {
                visit_expr(k, bound, ctx, seen, ordered);
                visit_expr(v, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::Match(scrutinee, branches) => {
            visit_expr(scrutinee, bound, ctx, seen, ordered);
            for b in branches {
                if let Some(guard) = &b.guard {
                    visit_expr(guard, bound, ctx, seen, ordered);
                }
                visit_stmt(&b.body, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::EnumValue(name_expr, args) => {
            visit_expr(name_expr, bound, ctx, seen, ordered);
            for a in args {
                visit_expr(a, bound, ctx, seen, ordered);
            }
        }
        ExpressionKind::NamedArgument(_, inner) => visit_expr(inner, bound, ctx, seen, ordered),
        ExpressionKind::Block(stmts, final_expr) => {
            let snap = bound.clone();
            for s in stmts {
                visit_stmt(s, bound, ctx, seen, ordered);
            }
            visit_expr(final_expr, bound, ctx, seen, ordered);
            *bound = snap;
        }
        // No captureable identifiers in these shapes; rejected by the GPU
        // type check anyway. Listed explicitly so a future `ExpressionKind`
        // variant cannot be silently dropped from capture collection.
        ExpressionKind::Literal(_)
        | ExpressionKind::Super
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::Lambda(_) => {}
    }
}

fn visit_lhs(
    lhs: &crate::ast::expression::LeftHandSideExpression,
    bound: &mut HashSet<String>,
    ctx: &LoweringContext,
    seen: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) {
    use crate::ast::expression::LeftHandSideExpression;
    match lhs {
        LeftHandSideExpression::Identifier(expr)
        | LeftHandSideExpression::Member(expr)
        | LeftHandSideExpression::Index(expr) => visit_expr(expr, bound, ctx, seen, ordered),
    }
}

fn build_kernel_body(
    parent: &mut LoweringContext,
    captures: &[CaptureInfo],
    loop_var_name: &str,
    start: i64,
    length: i64,
    body: &Statement,
    span: Span,
) -> Result<Body, LoweringError> {
    let arg_count = captures.len();
    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([GPU_FOR_BLOCK_SIZE, 1, 1]),
        required_capabilities: Vec::new(),
    }));
    // In a GPU kernel body the `out_params` slot is reused by the WGSL
    // backend to decide whether a storage-buffer binding is read-only or
    // read-write (see `wgsl::emitter`). Captures of `gpu for` are exposed
    // as read-write storage buffers regardless of whether the body writes
    // them — a wider read-write permission is harmless on the GPU and lets
    // common `dst[i] = ...` patterns work without per-capture write
    // detection, which the baseline does not yet do. Length must match
    // `arg_count`; the WGSL emitter errors out otherwise.
    kernel.out_params = vec![true; captures.len()];

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);
    for cap in captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    let i64_ty = Type::new(TypeKind::Int, span);
    let u32_ty = Type::new(TypeKind::U32, span);

    let global_idx_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        &mut ctx,
        global_idx_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X)),
        span,
    );
    let block_id_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        &mut ctx,
        block_id_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::X)),
        span,
    );
    let block_dim_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        &mut ctx,
        block_dim_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(Dimension::X)),
        span,
    );
    let block_offset_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        &mut ctx,
        block_offset_u32,
        Rvalue::BinaryOp(
            BinOp::Mul,
            Box::new(Operand::Copy(Place::new(block_id_u32))),
            Box::new(Operand::Copy(Place::new(block_dim_u32))),
        ),
        span,
    );
    let thread_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        &mut ctx,
        thread_u32,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(global_idx_u32))),
            Box::new(Operand::Copy(Place::new(block_offset_u32))),
        ),
        span,
    );

    let thread_int = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        &mut ctx,
        thread_int,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(thread_u32))),
            i64_ty.clone(),
        ),
        span,
    );
    let loop_local = ctx.push_local(loop_var_name.to_string(), i64_ty.clone(), span);
    let start_const = int_constant(start, span);
    push_assign(
        &mut ctx,
        loop_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_int))),
            Box::new(start_const),
        ),
        span,
    );

    let cond_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let limit = start + length;
    push_assign(
        &mut ctx,
        cond_local,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(loop_local))),
            Box::new(int_constant(limit, span)),
        ),
        span,
    );

    let body_bb = ctx.new_basic_block();
    let exit_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(cond_local)),
            targets: vec![(Discriminant::bool_true(), body_bb)],
            otherwise: exit_bb,
        },
        span,
    ));

    ctx.set_current_block(body_bb);
    lower_statement(&mut ctx, body)?;
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.set_terminator(Terminator::new(
            TerminatorKind::Goto { target: exit_bb },
            span,
        ));
    }

    ctx.set_current_block(exit_bb);
    ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));

    Ok(ctx.body)
}

fn emit_gpu_launch(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    length: i64,
    captures: &[CaptureInfo],
    span: Span,
) {
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);
    let void_ty = Type::new(TypeKind::Void, span);

    let block_size_i64 = i64::from(GPU_FOR_BLOCK_SIZE);
    // length and block_size are both positive (validated by
    // `compute_range_length`), so `length / block_size` cannot overflow.
    // Round up without ever forming `length + block_size - 1`, which would
    // overflow when length is near i64::MAX.
    let grid_count = length / block_size_i64 + i64::from(length % block_size_i64 != 0);
    let one_op = int_constant(1, span);
    let grid_x_op = int_constant(grid_count, span);
    let block_x_op = int_constant(block_size_i64, span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty.clone()),
            vec![grid_x_op, one_op.clone(), one_op.clone()],
        ),
        span,
    );
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty.clone()),
            vec![block_x_op, one_op.clone(), one_op],
        ),
        span,
    );

    let kernel_op = Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: Literal::Identifier(kernel_name.to_string()),
    }));

    let arg_ops: Vec<Operand> = captures
        .iter()
        .map(|c| Operand::Copy(Place::new(c.outer_local)))
        .collect();
    let arg_handles: Vec<Option<DeviceHandleId>> = captures
        .iter()
        .map(|c| ctx.body.local_decls[c.outer_local.0].device_handle)
        .collect();

    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            args: arg_ops,
            arg_handles,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

fn push_assign(ctx: &mut LoweringContext, local: Local, rvalue: Rvalue, span: Span) {
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(Place::new(local), rvalue),
        span,
    });
}

fn int_constant(value: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}
