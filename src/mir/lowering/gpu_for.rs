// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `gpu for <ident> in <range>` loops.
//!
//! Extracts the loop body into a synthesized anonymous `gpu fn` kernel and
//! emits a `TerminatorKind::GpuLaunch` at the call site with a fixed
//! workgroup size of 256.
//!
//! F1 feature (variable-bound `gpu for`):
//! - Range start must be an Int literal (variable start is a follow-up).
//! - Range end may be a runtime Int expression (e.g., `let n = 4; gpu for i in 0..n`).
//!   When end is a literal, uses fast constant-grid path.
//!   When end is a runtime expression, computes grid at runtime and passes the
//!   bounds-check limit as a uniform buffer to the kernel.
//!
//! Other restrictions:
//! - Only one loop variable is accepted.
//! - The body may reference outer-scope variables whose types are GPU
//!   buffers (`Array<T, N>`); all such captures are exposed as read-write
//!   storage buffers.

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
use super::expression::lower_expression;
use super::statement::lower_statement;

const GPU_FOR_BLOCK_SIZE: u32 = 256;

/// Lowers a `gpu for` loop into a synthesized kernel + `GpuLaunch`.
///
/// Two paths (F1 feature):
/// - Literal end: uses existing constant-grid lowering (fast path, no grid arithmetic).
/// - Runtime end: computes grid at runtime, emits uniform buffer for bounds-check limit.
pub fn lower_gpu_for(
    ctx: &mut LoweringContext,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    match decls.len() {
        1 => lower_gpu_for_1d(ctx, span, stmt_id, decls, iterable, body),
        2 => lower_gpu_for_2d(ctx, span, stmt_id, decls, iterable, body),
        _ => Err(LoweringError::unsupported_expression(
            format!(
                "gpu for: expected 1 or 2 loop variables, got {}",
                decls.len()
            ),
            *span,
        )),
    }
}

fn lower_gpu_for_1d(
    ctx: &mut LoweringContext,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let loop_var_name = decls[0].name.clone();

    let ExpressionKind::Range(start, Some(end), range_type) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu for: iterable must be a bounded numeric range like '0..n'".to_string(),
            *span,
        ));
    };

    // Extract start as a literal (variable start is out of scope).
    let start_lit = read_int_literal(start, *span)?;

    // Check if end is a literal or a runtime expression.
    let is_literal_end = matches!(&end.node, ExpressionKind::Literal(Literal::Integer(_)));

    let captures = collect_capture_infos(ctx, body, &loop_var_name, *span)?;
    let kernel_name = format!("miri_gpu_for_{}", stmt_id);

    if is_literal_end {
        // Literal path: fast compile-time grid computation.
        let end_lit = read_int_literal(end, *span)?;
        let length = compute_range_length(start_lit, end_lit, range_type.clone(), *span)?;
        let kernel_body = build_kernel_body_literal(
            ctx,
            &captures,
            &loop_var_name,
            start_lit,
            length,
            body,
            *span,
        )?;
        ctx.lambda_bodies.push(LambdaInfo {
            name: kernel_name.clone(),
            body: kernel_body,
            captures: Vec::new(),
        });
        emit_gpu_launch_literal(ctx, &kernel_name, length, &captures, *span);
    } else {
        // Runtime path: grid computed at runtime, bounds-check via uniform buffer.
        let kernel_body =
            build_kernel_body_runtime(ctx, &captures, &loop_var_name, start_lit, body, *span)?;
        ctx.lambda_bodies.push(LambdaInfo {
            name: kernel_name.clone(),
            body: kernel_body,
            captures: Vec::new(),
        });
        emit_gpu_launch_runtime(
            ctx,
            &kernel_name,
            start_lit,
            end,
            range_type.clone(),
            &captures,
            *span,
        )?;
    }
    Ok(())
}

fn lower_gpu_for_2d(
    ctx: &mut LoweringContext,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    let ExpressionKind::Tuple(ranges) = &iterable.node else {
        return Err(LoweringError::unsupported_expression(
            "2D gpu for: expected a tuple of two ranges".to_string(),
            *span,
        ));
    };
    if ranges.len() != 2 {
        return Err(LoweringError::unsupported_expression(
            format!(
                "2D gpu for: expected exactly 2 ranges, got {}",
                ranges.len()
            ),
            *span,
        ));
    }

    let ExpressionKind::Range(start_x, Some(end_x), range_type_x) = &ranges[0].node else {
        return Err(LoweringError::unsupported_expression(
            "2D gpu for: first range must be a bounded numeric range".to_string(),
            *span,
        ));
    };
    let ExpressionKind::Range(start_y, Some(end_y), range_type_y) = &ranges[1].node else {
        return Err(LoweringError::unsupported_expression(
            "2D gpu for: second range must be a bounded numeric range".to_string(),
            *span,
        ));
    };

    let start_x_lit = read_int_literal(start_x, *span)?;
    let end_x_lit = read_int_literal(end_x, *span)?;
    let start_y_lit = read_int_literal(start_y, *span)?;
    let end_y_lit = read_int_literal(end_y, *span)?;

    let width = compute_range_length(start_x_lit, end_x_lit, range_type_x.clone(), *span)?;
    let height = compute_range_length(start_y_lit, end_y_lit, range_type_y.clone(), *span)?;

    let loop_var_x = decls[0].name.clone();
    let loop_var_y = decls[1].name.clone();

    let captures = collect_capture_infos(ctx, body, &loop_var_x, *span)?;
    let kernel_name = format!("miri_gpu_for_2d_{}", stmt_id);

    let kernel_body = build_kernel_body_2d_literal(
        ctx,
        Kernel2DContext {
            captures: &captures,
            loop_var_x: &loop_var_x,
            loop_var_y: &loop_var_y,
            start_x: start_x_lit,
            start_y: start_y_lit,
            width,
            height,
            body,
            span: *span,
        },
    )?;

    ctx.lambda_bodies.push(LambdaInfo {
        name: kernel_name.clone(),
        body: kernel_body,
        captures: Vec::new(),
    });

    emit_gpu_launch_2d_literal(ctx, &kernel_name, width, height, &captures, *span);
    Ok(())
}

struct CaptureInfo {
    name: String,
    ty: Type,
    outer_local: Local,
    is_written: bool,
}

/// Collects the names of variables that are written to in the given statement.
/// This includes assignments like `x = ...`, `x[i] = ...`, and `x.field = ...`.
fn collect_written_captures(body: &Statement) -> HashSet<String> {
    let mut written = HashSet::new();
    visit_written_stmt(body, &mut written);
    written
}

fn visit_written_stmt(stmt: &Statement, written: &mut HashSet<String>) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            for s in stmts {
                visit_written_stmt(s, written);
            }
        }
        StatementKind::Expression(expr) => visit_written_expr(expr, written),
        StatementKind::Variable(_, _) => {}
        StatementKind::Return(_) => {}
        StatementKind::If(_, then_branch, else_branch, _) => {
            visit_written_stmt(then_branch, written);
            if let Some(eb) = else_branch {
                visit_written_stmt(eb, written);
            }
        }
        StatementKind::While(_, body, _) => visit_written_stmt(body, written),
        StatementKind::For(_, _, body)
        | StatementKind::GpuFor(_, _, body)
        | StatementKind::GpuFrame(_, _, body) => {
            visit_written_stmt(body, written);
        }
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

fn visit_written_expr(expr: &Expression, written: &mut HashSet<String>) {
    if let ExpressionKind::Assignment(lhs, _, rhs) = &expr.node {
        extract_written_lhs(lhs, written);
        visit_written_expr(rhs, written);
    }
}

fn extract_written_lhs(
    lhs: &crate::ast::expression::LeftHandSideExpression,
    written: &mut HashSet<String>,
) {
    use crate::ast::expression::LeftHandSideExpression;
    match lhs {
        LeftHandSideExpression::Identifier(expr) => {
            if let ExpressionKind::Identifier(name, _) = &expr.node {
                written.insert(name.clone());
            }
        }
        LeftHandSideExpression::Index(expr) | LeftHandSideExpression::Member(expr) => {
            if let ExpressionKind::Index(base, _) | ExpressionKind::Member(base, _) = &expr.node {
                if let ExpressionKind::Identifier(name, _) = &base.node {
                    written.insert(name.clone());
                }
            }
        }
    }
}

/// Collect and validate the outer-variable captures of a `gpu for` body. Only
/// gpu-resident buffer (`Array`-shaped) captures are accepted; everything else
/// is rejected with a source-cited diagnostic.
fn collect_capture_infos(
    ctx: &LoweringContext,
    body: &Statement,
    loop_var_name: &str,
    span: Span,
) -> Result<Vec<CaptureInfo>, LoweringError> {
    let capture_names = collect_outer_captures(body, loop_var_name, ctx);
    let written = collect_written_captures(body);
    let mut captures: Vec<CaptureInfo> = Vec::with_capacity(capture_names.len());
    for name in capture_names {
        let Some(&outer_local) = ctx.variable_map.get(name.as_str()) else {
            return Err(LoweringError::unsupported_expression(
                format!(
                    "gpu for: captured variable '{}' is not visible at the loop site",
                    name
                ),
                span,
            ));
        };
        let ty = ctx.body.local_decls[outer_local.0].ty.clone();
        if !is_gpu_buffer_capture(&ty.kind) {
            return Err(LoweringError::unsupported_expression(
                format!("gpu for: capture '{}' has non-buffer type; baseline only accepts `Array<T, N>` or `[T; N]` captures (scalar/string/collection captures need uniform/push-constant lowering, follow-up)", name),
                span,
            ));
        }
        // Only a gpu-resident buffer may be marshaled as a kernel storage
        // binding; host buffers are rejected upstream (§6.4) so this guards
        // against any implicit upload (GPU_DRAFT §10.5 — no silent promotion).
        if ctx.body.local_decls[outer_local.0].residency != BindingResidency::Gpu {
            return Err(LoweringError::unsupported_expression(
                format!("gpu for: capture '{}' is not gpu-resident", name),
                span,
            ));
        }
        captures.push(CaptureInfo {
            name: name.clone(),
            ty,
            outer_local,
            is_written: written.contains(&name),
        });
    }
    Ok(captures)
}

/// Returns `true` for types whose runtime representation is a host-side
/// `MiriArray`-shaped buffer that the GPU dispatcher can marshal as a
/// storage binding. Scalars and non-buffer managed types pass the broader
/// `is_gpu_compatible` predicate (used for kernel-body type checking) but
/// would be misinterpreted as MiriArray pointers by `gpu_launch::translate`.
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
        | StatementKind::GpuFor(inner_decls, iter, body)
        | StatementKind::GpuFrame(inner_decls, iter, body) => {
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
        ExpressionKind::Cast(value_expr, target_type_expr) => {
            visit_expr(value_expr, bound, ctx, seen, ordered);
            visit_expr(target_type_expr, bound, ctx, seen, ordered);
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

/// Computes the global thread index for the given dimension:
/// `thread_id[dim] + block_id[dim] * block_dim[dim]`, cast to i64.
/// Returns the i64 thread index and emits the necessary statements into ctx.
fn compute_thread_index(ctx: &mut LoweringContext, dim: Dimension, span: Span) -> Local {
    let u32_ty = Type::new(TypeKind::U32, span);
    let i64_ty = Type::new(TypeKind::Int, span);

    let global_idx_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        global_idx_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(dim)),
        span,
    );
    let block_id_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        block_id_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(dim)),
        span,
    );
    let block_dim_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
        block_dim_u32,
        Rvalue::GpuIntrinsic(GpuIntrinsic::BlockDim(dim)),
        span,
    );
    let block_offset_u32 = ctx.push_temp(u32_ty.clone(), span);
    push_assign(
        ctx,
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
        ctx,
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
        ctx,
        thread_int,
        Rvalue::Cast(Box::new(Operand::Copy(Place::new(thread_u32))), i64_ty),
        span,
    );
    thread_int
}

fn build_kernel_body_literal(
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

    // Compute grid size: ceil(length / workgroup_size)
    let block_size_i64 = i64::from(GPU_FOR_BLOCK_SIZE);
    let grid_count = length / block_size_i64 + i64::from(length % block_size_i64 != 0);
    let grid_x = grid_count.min(u32::MAX as i64) as u32;

    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([GPU_FOR_BLOCK_SIZE, 1, 1]),
        grid_size: Some([grid_x, 1, 1]),
        required_capabilities: Vec::new(),
        is_frame_step: false,
    }));
    kernel.out_params = captures.iter().map(|c| c.is_written).collect();

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);
    for cap in captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    let i64_ty = Type::new(TypeKind::Int, span);
    let thread_int = compute_thread_index(&mut ctx, Dimension::X, span);

    let loop_local = ctx.push_local(loop_var_name.to_string(), i64_ty.clone(), span);
    push_assign(
        &mut ctx,
        loop_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_int))),
            Box::new(int_constant(start, span)),
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

fn emit_gpu_launch_literal(
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
    let arg_int_narrow: Vec<bool> = captures
        .iter()
        .map(|c| needs_int_narrowing(&c.ty))
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
            arg_read_only: captures.iter().map(|c| !c.is_written).collect(),
            arg_int_narrow,
            uniform_bound: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

/// Builds the kernel body for a runtime-bound `gpu for` loop.
/// The bounds-check limit is read from the uniform buffer at binding index
/// `captures.len()` instead of being a compile-time constant.
fn build_kernel_body_runtime(
    parent: &mut LoweringContext,
    captures: &[CaptureInfo],
    loop_var_name: &str,
    start: i64,
    body: &Statement,
    span: Span,
) -> Result<Body, LoweringError> {
    let arg_count = captures.len() + 1;
    let mut kernel = Body::new(arg_count, span, ExecutionModel::GpuKernel);
    kernel
        .local_decls
        .push(LocalDecl::new(Type::new(TypeKind::Void, span), span));
    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([GPU_FOR_BLOCK_SIZE, 1, 1]),
        grid_size: None, // Runtime-bound; grid computed at runtime
        required_capabilities: Vec::new(),
        is_frame_step: false,
    }));

    let mut out_params: Vec<bool> = captures.iter().map(|c| c.is_written).collect();
    out_params.push(false);
    kernel.out_params = out_params;

    let mut ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);

    for cap in captures {
        let local = ctx.push_param(cap.name.clone(), cap.ty.clone(), span);
        ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    let i64_ty = Type::new(TypeKind::Int, span);
    let uniform_param = ctx.push_param("_uniform_bound".to_string(), i64_ty.clone(), span);
    ctx.body.local_decls[uniform_param.0].storage_class = StorageClass::UniformBuffer;

    let thread_int = compute_thread_index(&mut ctx, Dimension::X, span);

    let loop_local = ctx.push_local(loop_var_name.to_string(), i64_ty.clone(), span);
    push_assign(
        &mut ctx,
        loop_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_int))),
            Box::new(int_constant(start, span)),
        ),
        span,
    );

    emit_bounds_check_loop(&mut ctx, loop_local, uniform_param, body, span)?;

    Ok(ctx.body)
}

/// Emits bounds check loop with uniform parameter.
/// Assumes loop_local and uniform_param are already initialized.
fn emit_bounds_check_loop(
    ctx: &mut LoweringContext,
    loop_local: Local,
    uniform_param: Local,
    body: &Statement,
    span: Span,
) -> Result<(), LoweringError> {
    let cond_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        cond_local,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(loop_local))),
            Box::new(Operand::Copy(Place::new(uniform_param))),
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
    lower_statement(ctx, body)?;
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

    Ok(())
}

/// Emits the GpuLaunch terminator for a runtime-bound `gpu for` loop.
/// Computes the grid size at runtime from `end - start`, clamped to 0 for
/// negative ranges, and passes the `end` operand (adjusted for inclusive ranges)
/// as the uniform bound.
/// Computes grid size from a clamped loop length using overflow-safe ceiling division.
/// Returns the grid-x value (Local). Emits statements into ctx.
fn compute_grid_size(ctx: &mut LoweringContext, clamped_length: Local, span: Span) -> Local {
    let i64_ty = Type::new(TypeKind::Int, span);
    let block_size_i64 = i64::from(GPU_FOR_BLOCK_SIZE);

    let grid_x_div_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        grid_x_div_local,
        Rvalue::BinaryOp(
            BinOp::Div,
            Box::new(Operand::Copy(Place::new(clamped_length))),
            Box::new(int_constant(block_size_i64, span)),
        ),
        span,
    );
    let grid_x_rem_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        grid_x_rem_local,
        Rvalue::BinaryOp(
            BinOp::Rem,
            Box::new(Operand::Copy(Place::new(clamped_length))),
            Box::new(int_constant(block_size_i64, span)),
        ),
        span,
    );
    let has_remainder_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        has_remainder_local,
        Rvalue::BinaryOp(
            BinOp::Ne,
            Box::new(Operand::Copy(Place::new(grid_x_rem_local))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );
    let has_remainder_i64_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        has_remainder_i64_local,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(has_remainder_local))),
            i64_ty.clone(),
        ),
        span,
    );
    let final_grid_x_local = ctx.push_temp(i64_ty, span);
    push_assign(
        ctx,
        final_grid_x_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(grid_x_div_local))),
            Box::new(Operand::Copy(Place::new(has_remainder_i64_local))),
        ),
        span,
    );
    final_grid_x_local
}

/// Computes clamped loop length from end - start.
/// Clamps to 0 for negative ranges. Returns the clamped length Local.
fn compute_clamped_length(
    ctx: &mut LoweringContext,
    end_op: Operand,
    start: i64,
    span: Span,
) -> Local {
    let i64_ty = Type::new(TypeKind::Int, span);
    let length_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        length_local,
        Rvalue::BinaryOp(
            BinOp::Sub,
            Box::new(end_op),
            Box::new(int_constant(start, span)),
        ),
        span,
    );

    let is_pos_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        is_pos_local,
        Rvalue::BinaryOp(
            BinOp::Gt,
            Box::new(Operand::Copy(Place::new(length_local))),
            Box::new(int_constant(0, span)),
        ),
        span,
    );

    let is_pos_i64_local = ctx.push_temp(i64_ty.clone(), span);
    push_assign(
        ctx,
        is_pos_i64_local,
        Rvalue::Cast(
            Box::new(Operand::Copy(Place::new(is_pos_local))),
            i64_ty.clone(),
        ),
        span,
    );

    let clamped_local = ctx.push_temp(i64_ty, span);
    push_assign(
        ctx,
        clamped_local,
        Rvalue::BinaryOp(
            BinOp::Mul,
            Box::new(Operand::Copy(Place::new(length_local))),
            Box::new(Operand::Copy(Place::new(is_pos_i64_local))),
        ),
        span,
    );
    clamped_local
}

/// Builds Dim3 grid and block dimensions at runtime from grid_x value.
/// Returns (grid_local, block_local).
fn build_dim3_descriptors(ctx: &mut LoweringContext, grid_x: Local, span: Span) -> (Local, Local) {
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);
    let one_op = int_constant(1, span);
    let block_size_i64 = i64::from(GPU_FOR_BLOCK_SIZE);
    let block_x_op = int_constant(block_size_i64, span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty.clone()),
            vec![
                Operand::Copy(Place::new(grid_x)),
                one_op.clone(),
                one_op.clone(),
            ],
        ),
        span,
    );

    let block_local = ctx.push_temp(dim3_ty, span);
    push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(Type::new(
                TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None),
                span,
            )),
            vec![block_x_op, one_op.clone(), one_op],
        ),
        span,
    );

    (grid_local, block_local)
}

fn emit_gpu_launch_runtime(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    start: i64,
    end: &Expression,
    range_type: RangeExpressionType,
    captures: &[CaptureInfo],
    span: Span,
) -> Result<(), LoweringError> {
    let mut end_op = lower_expression(ctx, end, None)?;

    if range_type == RangeExpressionType::Inclusive {
        let i64_ty = Type::new(TypeKind::Int, span);
        let end_plus_one_local = ctx.push_temp(i64_ty, span);
        push_assign(
            ctx,
            end_plus_one_local,
            Rvalue::BinaryOp(
                BinOp::Add,
                Box::new(end_op),
                Box::new(int_constant(1, span)),
            ),
            span,
        );
        end_op = Operand::Copy(Place::new(end_plus_one_local));
    }

    let clamped_length = compute_clamped_length(ctx, end_op.clone(), start, span);
    let grid_x = compute_grid_size(ctx, clamped_length, span);
    let (grid_local, block_local) = build_dim3_descriptors(ctx, grid_x, span);

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
    let arg_int_narrow: Vec<bool> = captures
        .iter()
        .map(|c| needs_int_narrowing(&c.ty))
        .collect();

    let void_ty = Type::new(TypeKind::Void, span);
    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();

    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            args: arg_ops,
            arg_handles,
            arg_read_only: {
                let mut read_only: Vec<bool> = captures.iter().map(|c| !c.is_written).collect();
                read_only.push(false);
                read_only
            },
            arg_int_narrow,
            uniform_bound: Some(Box::new(end_op)),
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
    Ok(())
}

fn push_assign(ctx: &mut LoweringContext, local: Local, rvalue: Rvalue, span: Span) {
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(Place::new(local), rvalue),
        span,
    });
}

struct Kernel2DContext<'a> {
    captures: &'a [CaptureInfo],
    loop_var_x: &'a str,
    loop_var_y: &'a str,
    start_x: i64,
    start_y: i64,
    width: i64,
    height: i64,
    body: &'a Statement,
    span: Span,
}

/// Emits the 2D bounds-check conditional, kernel body, and cleanup.
fn emit_2d_bounds_check_loop(
    ctx: &mut LoweringContext,
    x_local: Local,
    y_local: Local,
    end_x: i64,
    end_y: i64,
    body: &Statement,
    span: Span,
) -> Result<(), LoweringError> {
    let x_in_bounds = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        x_in_bounds,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(x_local))),
            Box::new(int_constant(end_x, span)),
        ),
        span,
    );

    let y_in_bounds = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        y_in_bounds,
        Rvalue::BinaryOp(
            BinOp::Lt,
            Box::new(Operand::Copy(Place::new(y_local))),
            Box::new(int_constant(end_y, span)),
        ),
        span,
    );

    let cond_local = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    push_assign(
        ctx,
        cond_local,
        Rvalue::BinaryOp(
            BinOp::BitAnd,
            Box::new(Operand::Copy(Place::new(x_in_bounds))),
            Box::new(Operand::Copy(Place::new(y_in_bounds))),
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
    lower_statement(ctx, body)?;
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

    Ok(())
}

fn build_kernel_body_2d_literal(
    parent: &mut LoweringContext,
    ctx: Kernel2DContext,
) -> Result<Body, LoweringError> {
    const BLOCK_SIZE_2D: u32 = 16;

    let arg_count = ctx.captures.len();
    let mut kernel = Body::new(arg_count, ctx.span, ExecutionModel::GpuKernel);
    kernel.local_decls.push(LocalDecl::new(
        Type::new(TypeKind::Void, ctx.span),
        ctx.span,
    ));

    // Compute grid size for 2D: ceil(width / BLOCK_SIZE_2D) x ceil(height / BLOCK_SIZE_2D)
    let block_size_i64 = i64::from(BLOCK_SIZE_2D);
    let grid_x = ctx.width / block_size_i64 + i64::from(ctx.width % block_size_i64 != 0);
    let grid_y = ctx.height / block_size_i64 + i64::from(ctx.height % block_size_i64 != 0);
    let grid_x_u32 = grid_x.min(u32::MAX as i64) as u32;
    let grid_y_u32 = grid_y.min(u32::MAX as i64) as u32;

    kernel.backend_metadata = Some(BackendMetadata::Gpu(GpuBodyMetadata {
        workgroup_size: Some([BLOCK_SIZE_2D, BLOCK_SIZE_2D, 1]),
        grid_size: Some([grid_x_u32, grid_y_u32, 1]),
        required_capabilities: Vec::new(),
        is_frame_step: false,
    }));
    kernel.out_params = ctx.captures.iter().map(|c| c.is_written).collect();

    let mut lower_ctx = LoweringContext::new(kernel, parent.type_checker, parent.is_release);
    for cap in ctx.captures {
        let local = lower_ctx.push_param(cap.name.clone(), cap.ty.clone(), ctx.span);
        lower_ctx.body.local_decls[local.0].storage_class = StorageClass::GpuGlobal;
    }

    let i64_ty = Type::new(TypeKind::Int, ctx.span);
    let thread_x = compute_thread_index(&mut lower_ctx, Dimension::X, ctx.span);
    let thread_y = compute_thread_index(&mut lower_ctx, Dimension::Y, ctx.span);

    let x_local = lower_ctx.push_local(ctx.loop_var_x.to_string(), i64_ty.clone(), ctx.span);
    push_assign(
        &mut lower_ctx,
        x_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_x))),
            Box::new(int_constant(ctx.start_x, ctx.span)),
        ),
        ctx.span,
    );

    let y_local = lower_ctx.push_local(ctx.loop_var_y.to_string(), i64_ty.clone(), ctx.span);
    push_assign(
        &mut lower_ctx,
        y_local,
        Rvalue::BinaryOp(
            BinOp::Add,
            Box::new(Operand::Copy(Place::new(thread_y))),
            Box::new(int_constant(ctx.start_y, ctx.span)),
        ),
        ctx.span,
    );

    let end_x = ctx.start_x + ctx.width;
    let end_y = ctx.start_y + ctx.height;

    emit_2d_bounds_check_loop(
        &mut lower_ctx,
        x_local,
        y_local,
        end_x,
        end_y,
        ctx.body,
        ctx.span,
    )?;

    Ok(lower_ctx.body)
}

fn emit_gpu_launch_2d_literal(
    ctx: &mut LoweringContext,
    kernel_name: &str,
    width: i64,
    height: i64,
    captures: &[CaptureInfo],
    span: Span,
) {
    const BLOCK_SIZE: u32 = 16;
    let dim3_ty = Type::new(TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None), span);

    let block_size_i64 = i64::from(BLOCK_SIZE);
    let grid_x = width / block_size_i64 + i64::from(width % block_size_i64 != 0);
    let grid_y = height / block_size_i64 + i64::from(height % block_size_i64 != 0);

    let grid_x_op = int_constant(grid_x, span);
    let grid_y_op = int_constant(grid_y, span);
    let one_op = int_constant(1, span);

    let grid_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        grid_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty.clone()),
            vec![grid_x_op, grid_y_op, one_op.clone()],
        ),
        span,
    );

    let block_size_op = int_constant(block_size_i64, span);
    let block_local = ctx.push_temp(dim3_ty.clone(), span);
    push_assign(
        ctx,
        block_local,
        Rvalue::Aggregate(
            AggregateKind::Struct(dim3_ty.clone()),
            vec![block_size_op.clone(), block_size_op, one_op],
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
    let arg_int_narrow: Vec<bool> = captures
        .iter()
        .map(|c| needs_int_narrowing(&c.ty))
        .collect();

    let void_ty = Type::new(TypeKind::Void, span);
    let dest_local = ctx.push_temp(void_ty, span);
    let after_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::GpuLaunch {
            kernel: kernel_op,
            grid: Operand::Copy(Place::new(grid_local)),
            block: Operand::Copy(Place::new(block_local)),
            args: arg_ops,
            arg_handles,
            arg_read_only: captures.iter().map(|c| !c.is_written).collect(),
            arg_int_narrow,
            uniform_bound: None,
            destination: Place::new(dest_local),
            target: Some(after_bb),
        },
        span,
    ));
    ctx.set_current_block(after_bb);
}

fn int_constant(value: i64, span: Span) -> Operand {
    Operand::Constant(Box::new(Constant {
        span,
        ty: Type::new(TypeKind::Int, span),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

/// CHANGE 2: Determines if a GPU buffer needs i64→i32 narrowing at the host
/// boundary. True iff the buffer's element type is `int` / `i64` — those are
/// 8-byte on the host but emitted as `array<i32>` (4-byte) in WGSL (WebGPU has
/// no 64-bit integer), so the runtime narrows on upload and widens on readback.
/// `i32`/`u32`/`f32`/`f64`/etc. buffers match the device width and are not narrowed.
fn needs_int_narrowing(ty: &Type) -> bool {
    let elem_expr = match &ty.kind {
        // AST-phase variants (before normalization).
        TypeKind::Array(elem_expr, _) | TypeKind::List(elem_expr) => Some(elem_expr.as_ref()),
        // Post-normalization: Array/List become Custom("Array"/"List", [elem_ty_expr, ...]).
        TypeKind::Custom(name, Some(args))
            if (name == "Array" || name == "List") && !args.is_empty() =>
        {
            Some(&args[0])
        }
        _ => None,
    };

    elem_expr
        .is_some_and(|expr| matches!(element_type_kind(expr), Some(TypeKind::Int | TypeKind::I64)))
}

/// Resolves a collection element-type expression to its `TypeKind`, covering the
/// forms produced across the pipeline: a `Type(...)` node (post-normalization),
/// a bare identifier, or a literal identifier.
fn element_type_kind(expr: &crate::ast::expression::Expression) -> Option<TypeKind> {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::literal::Literal;

    let name = match &expr.node {
        ExpressionKind::Type(inner, _) => return Some(inner.kind.clone()),
        ExpressionKind::Identifier(name, _) => name.as_str(),
        ExpressionKind::Literal(Literal::Identifier(name)) => name.as_str(),
        _ => return None,
    };
    match name {
        "int" => Some(TypeKind::Int),
        "i64" => Some(TypeKind::I64),
        "i32" => Some(TypeKind::I32),
        "i16" => Some(TypeKind::I16),
        "i8" => Some(TypeKind::I8),
        "u64" => Some(TypeKind::U64),
        "u32" => Some(TypeKind::U32),
        "f32" => Some(TypeKind::F32),
        "f64" => Some(TypeKind::F64),
        _ => None,
    }
}
