// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lambda/closure lowering to MIR.
//!
//! All lambdas — capturing or not — are lowered to **closure structs**.
//! A closure struct is a heap allocation: [malloc_ptr][RC][fn_ptr][cap0][cap1]...
//! The lambda variable holds `payload_ptr` (past the 2-word header).
//!
//! Lambda body calling convention:
//!   Local 0      = return value
//!   Local 1      = env_ptr (TypeKind::RawPtr) — implicit first parameter
//!   Local 2..N+1 = user parameters
//!   Local N+2..  = captured values (loaded from env_ptr in codegen)
//!
//! Capture detection: outer-scope variables are added to the lambda context so the
//! body can reference them. After lowering, only those that are actually READ in the
//! body MIR are kept as real captures. Unused ones are pruned.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::lambda::{CapturedVar, LambdaInfo};
use crate::mir::rvalue::AggregateKind;
use crate::mir::{
    Body, ExecutionModel, LocalDecl, Operand, Place, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::{lower_as_return, resolve_type};
use std::collections::HashSet;

/// Collect all `Local` indices that appear as operand sources in the body.
/// This is used to detect which "potential captures" are actually referenced.
fn collect_read_locals(body: &Body) -> HashSet<crate::mir::Local> {
    let mut used = HashSet::new();
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            match &stmt.kind {
                MirStatementKind::Assign(_, rvalue) | MirStatementKind::Reassign(_, rvalue) => {
                    collect_rvalue_locals(rvalue, &mut used);
                }
                MirStatementKind::IncRef(place)
                | MirStatementKind::DecRef(place)
                | MirStatementKind::Dealloc(place) => {
                    used.insert(place.local);
                }
                _ => {}
            }
        }
        if let Some(term) = &block.terminator {
            use crate::mir::TerminatorKind;
            match &term.kind {
                TerminatorKind::Call { func, args, .. } => {
                    collect_operand_locals(func, &mut used);
                    for arg in args {
                        collect_operand_locals(arg, &mut used);
                    }
                }
                TerminatorKind::VirtualCall { args, .. } => {
                    for arg in args {
                        collect_operand_locals(arg, &mut used);
                    }
                }
                TerminatorKind::SwitchInt { discr, .. } => {
                    collect_operand_locals(discr, &mut used);
                }
                _ => {}
            }
        }
    }
    used
}

fn collect_operand_locals(op: &Operand, out: &mut HashSet<crate::mir::Local>) {
    match op {
        Operand::Copy(place) | Operand::Move(place) => {
            out.insert(place.local);
        }
        Operand::Constant(_) => {}
    }
}

fn collect_rvalue_locals(rv: &Rvalue, out: &mut HashSet<crate::mir::Local>) {
    match rv {
        Rvalue::Use(op) => collect_operand_locals(op, out),
        Rvalue::Ref(place) => {
            out.insert(place.local);
        }
        Rvalue::BinaryOp(_, lhs, rhs) => {
            collect_operand_locals(lhs, out);
            collect_operand_locals(rhs, out);
        }
        Rvalue::UnaryOp(_, op) => collect_operand_locals(op, out),
        Rvalue::Cast(op, _) => collect_operand_locals(op, out),
        Rvalue::Len(place) => {
            out.insert(place.local);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                collect_operand_locals(op, out);
            }
        }
        Rvalue::Phi(pairs) => {
            for (op, _) in pairs {
                collect_operand_locals(op, out);
            }
        }
        Rvalue::Allocate(a, b, c) => {
            collect_operand_locals(a, out);
            collect_operand_locals(b, out);
            collect_operand_locals(c, out);
        }
        Rvalue::GpuIntrinsic(_) => {}
    }
}

pub(crate) fn lower_lambda_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Lambda(lambda) = &expr.node else {
        unreachable!()
    };
    let params = &lambda.params;
    let ret_type_expr = &lambda.return_type;
    let body_stmt = &lambda.body;
    let props = &lambda.properties;

    // Generate a unique name for this lambda.
    let lambda_id = expr.id;

    // Optimization: avoid format! overhead in hot paths
    let mut num_len = 1;
    let mut n = lambda_id;
    while n >= 10 {
        n /= 10;
        num_len += 1;
    }

    let mut name_buf = String::with_capacity(9 + num_len);
    name_buf.push_str("__lambda_");

    use std::fmt::Write;
    let _ = write!(name_buf, "{}", lambda_id);

    let lambda_name: std::rc::Rc<str> = name_buf.into();

    // Resolve the lambda's full function type (used as type of the closure variable).
    let lambda_ty = resolve_type(ctx.type_checker, expr);

    // Resolve return type.
    let ret_ty = if let Some(ret_expr) = ret_type_expr {
        resolve_type(ctx.type_checker, ret_expr)
    } else {
        Type::new(TypeKind::Void, expr.span)
    };

    // Execution model.
    let execution_model = if props.is_gpu {
        ExecutionModel::GpuKernel
    } else if props.is_async {
        ExecutionModel::Async
    } else {
        ExecutionModel::Cpu
    };

    // ── Build the lambda Body ────────────────────────────────────────────
    // arg_count = 1 (env_ptr) + user params.
    // Note: the actual arg_count will be adjusted below after capture detection,
    // but we start with the maximum possible value.
    let arg_count = 1 + params.len();
    let mut lambda_body = Body::new(arg_count, expr.span, execution_model);

    // Local 0: return value — allocated directly in body before context creation.
    lambda_body.new_local(LocalDecl::new(ret_ty.clone(), expr.span));

    // ── Create the inner lowering context ───────────────────────────────
    // NOTE: all param locals (1, 2, ...) are allocated via push_param, NOT new_local,
    // so the LoweringContext sees them as proper parameters.
    let outer_variable_map = ctx.variable_map.clone();
    let mut lambda_ctx = LoweringContext::new(lambda_body, ctx.type_checker, ctx.is_release);

    // Local 1: env_ptr (implicit first parameter — pointer to the closure struct payload).
    // We use push_param so it does NOT emit StorageLive.
    lambda_ctx.push_param(
        "__env_ptr".to_string(),
        Type::new(TypeKind::RawPtr, expr.span),
        expr.span,
    );

    // Locals 2..N+1: user parameters.
    for param in params {
        let param_ty = resolve_type(ctx.type_checker, &param.typ);
        lambda_ctx.push_param(param.name.clone(), param_ty, param.typ.span);
    }

    // ── Register potential captures ──────────────────────────────────────
    // All outer variables that are NOT lambda params are potential captures.
    // We add them to the lambda context so the body can reference them.
    // After lowering, we prune any that are not actually read in the body.
    let param_names: HashSet<&str> = params.iter().map(|p| p.name.as_str()).collect();

    // Stable ordering: sort by outer_local index so env slot is deterministic.
    let mut outer_vars: Vec<(std::rc::Rc<str>, crate::mir::Local)> = outer_variable_map
        .iter()
        .filter(|(name, _)| !param_names.contains(name.as_ref()))
        .map(|(name, &local)| (name.clone(), local))
        .collect();
    outer_vars.sort_by_key(|(_, local)| local.0);

    // Tentative captures: add all outer vars to lambda_ctx so the body can reference them.
    let mut tentative_captures: Vec<(std::rc::Rc<str>, crate::mir::Local, crate::mir::Local)> =
        Vec::new();
    for (name, outer_local) in &outer_vars {
        let cap_ty = ctx.body.local_decls[outer_local.0].ty.clone();
        let lambda_local = lambda_ctx.push_param(name.to_string(), cap_ty, expr.span);
        tentative_captures.push((name.clone(), *outer_local, lambda_local));
    }

    // ── Lower the lambda body ────────────────────────────────────────────
    lower_as_return(&mut lambda_ctx, body_stmt, &ret_ty)?;

    // Ensure the last block has a terminator.
    let last_block_idx = lambda_ctx.current_block.0;
    if lambda_ctx.body.basic_blocks[last_block_idx]
        .terminator
        .is_none()
    {
        lambda_ctx.set_terminator(Terminator::new(TerminatorKind::Return, expr.span));
    }

    // ── Prune unused captures ────────────────────────────────────────────
    // Only keep captures whose lambda_local is actually READ in the body.
    let read_locals = collect_read_locals(&lambda_ctx.body);

    let mut captures: Vec<CapturedVar> = Vec::new();
    for (name, outer_local, lambda_local) in &tentative_captures {
        if read_locals.contains(lambda_local) {
            // This capture is actually used — record it.
            lambda_ctx.body.env_capture_locals.push(*lambda_local);
            captures.push(CapturedVar {
                name: name.clone(),
                lambda_local: *lambda_local,
                outer_local: *outer_local,
            });
        }
        // If not used, we leave the local allocated (it's harmless) but don't
        // include it in env_capture_locals or in the closure aggregate operands.
    }

    // ── Register the lambda ─────────────────────────────────────────────
    let lambda_info = LambdaInfo {
        name: lambda_name.to_string(),
        body: lambda_ctx.body,
        captures: captures.clone(),
    };
    ctx.lambda_bodies.push(lambda_info);

    // ── Emit closure struct allocation at the creation site ─────────────
    // Build capture operands from the outer scope's locals.
    let capture_operands: Vec<Operand> = captures
        .iter()
        .map(|cap| Operand::Copy(Place::new(cap.outer_local)))
        .collect();

    // Destination place.
    let target = dest.unwrap_or_else(|| Place::new(ctx.push_temp(lambda_ty.clone(), expr.span)));

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(
                AggregateKind::Closure(lambda_name, lambda_ty),
                capture_operands,
            ),
        ),
        span: expr.span,
    });

    Ok(Operand::Copy(target))
}
