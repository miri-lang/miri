// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A named function read as a value.
//!
//! Every function-typed value in Miri is a closure struct — the layout
//! `[malloc_ptr][RC][fn_ptr][cap0]...` that [`super::lambda_expr`] builds — and a
//! call through such a value loads `fn_ptr` from the payload, then passes the
//! payload back as an implicit first argument. A named function is a bare symbol
//! with neither a payload nor a leading environment parameter, so it cannot be
//! handed to a callee that expects that shape.
//!
//! Reading a function name in value position therefore builds a closure over a
//! synthesized forwarding thunk. The thunk has the closure calling convention —
//! `(env_ptr, params...)` — and calls the named function with the arguments it
//! was given, so the value is indistinguishable from a lambda's to everything
//! downstream. Its one capture is the enclosing allocator, which a Miri-defined
//! callee takes as an implicit trailing parameter.
//!
//! Callee position is deliberately different: `foo(1)` wants the symbol itself
//! so the call stays direct, which is what
//! [`super::identifier_expr::lower_identifier_symbol`] produces.

use crate::ast::common::Parameter;
use crate::ast::expression::Expression;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::mir::lambda::{CapturedVar, LambdaInfo};
use crate::mir::rvalue::AggregateKind;
use crate::mir::{
    Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::resolve_type;

/// The local holding a body's return value, by MIR convention.
const RETURN_LOCAL: Local = Local(0);

/// Lower `name` as a function value when it names a global function, returning
/// `None` when it names anything else so the caller falls back to its ordinary
/// identifier handling.
pub(crate) fn try_lower_function_reference(
    ctx: &mut LoweringContext,
    expr: &Expression,
    name: &str,
    dest: Option<Place>,
) -> Option<Operand> {
    let info = ctx.type_checker.global_scope().get(name)?;
    let TypeKind::Function(func_data) = &info.ty.kind else {
        return None;
    };
    let func_data = func_data.clone();
    let fn_ty = info.ty.clone();
    let symbol = info
        .original_name
        .clone()
        .unwrap_or_else(|| name.to_string());
    Some(lower_function_reference(
        ctx, expr, &symbol, &fn_ty, &func_data, dest,
    ))
}

/// Build the closure value and register the thunk body it points at.
fn lower_function_reference(
    ctx: &mut LoweringContext,
    expr: &Expression,
    symbol: &str,
    fn_ty: &Type,
    func_data: &FunctionTypeData,
    dest: Option<Place>,
) -> Operand {
    // The reference site's expression id keeps two references to the same
    // function from claiming one symbol, the way a lambda's id does.
    let thunk_name = format!("__fnref_{}_{}", symbol, expr.id);
    let forwarded_allocator = forwarded_allocator(ctx, symbol);
    let thunk = build_forwarding_thunk(
        ctx,
        expr,
        symbol,
        &thunk_name,
        func_data,
        forwarded_allocator.as_ref(),
    );
    ctx.lambda_bodies.push(thunk);

    let target = dest.unwrap_or_else(|| Place::new(ctx.push_temp(fn_ty.clone(), expr.span)));
    let capture_operands: Vec<Operand> = forwarded_allocator
        .iter()
        .map(|alloc| Operand::Copy(Place::new(alloc.local)))
        .collect();
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(
                AggregateKind::Closure(thunk_name.into(), fn_ty.clone()),
                capture_operands,
            ),
        ),
        span: expr.span,
    });
    // The allocator is an unmanaged scalar, so this closure owns no captured
    // values. A stale capture-type entry left by an earlier assignment to the
    // same local would make Perceus drop fields the new payload does not have.
    ctx.body.closure_capture_types.remove(&target.local);

    Operand::Copy(target)
}

/// The enclosing function's allocator, when the referenced function takes one
/// and the enclosing body has one to give.
///
/// A Miri-defined function is lowered with an implicit trailing allocator
/// parameter, so the thunk has to supply it — and the thunk's own signature is
/// fixed by the closure calling convention. The allocator therefore travels the
/// only way anything else reaches a closure body: as a capture.
fn forwarded_allocator(ctx: &LoweringContext, symbol: &str) -> Option<AllocatorCapture> {
    if !crate::mir::lowering::dispatch::callee_takes_allocator(ctx, symbol) {
        return None;
    }
    let local = *ctx.variable_map.get("allocator")?;
    Some(AllocatorCapture {
        local,
        ty: ctx.body.local_decls[local.0].ty.clone(),
    })
}

/// The enclosing allocator local a thunk captures, with the type to declare it
/// under inside the thunk body.
struct AllocatorCapture {
    local: Local,
    ty: Type,
}

/// Build the thunk body: `(env_ptr, p0..pN) -> symbol(p0..pN)`.
fn build_forwarding_thunk(
    ctx: &LoweringContext,
    expr: &Expression,
    symbol: &str,
    thunk_name: &str,
    func_data: &FunctionTypeData,
    allocator: Option<&AllocatorCapture>,
) -> LambdaInfo {
    let span = expr.span;
    let params = &func_data.params;
    let ret_ty = match &func_data.return_type {
        Some(ret_expr) => resolve_type(ctx.type_checker, ret_expr),
        None => Type::new(TypeKind::Void, span),
    };

    let mut body = Body::new(1 + params.len(), span, ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ret_ty, span));

    let mut thunk_ctx = LoweringContext::new(body, ctx.type_checker, ctx.is_release);
    thunk_ctx.use_compilation_ids(ctx.compilation_ids.clone());
    thunk_ctx.push_param(
        "__env_ptr".to_string(),
        Type::new(TypeKind::RawPtr, span),
        span,
    );

    let mut args = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let param_ty = resolve_type(ctx.type_checker, &param.typ);
        let local = thunk_ctx.push_param(thunk_param_name(param, index), param_ty, span);
        args.push(Operand::Copy(Place::new(local)));
    }

    let captures = capture_allocator(&mut thunk_ctx, allocator, span, &mut args);

    let after_call = thunk_ctx.new_basic_block();
    thunk_ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: crate::mir::lowering::dispatch::runtime_fn_operand(symbol, span),
            args,
            out_args: params.iter().map(|param| param.is_out).collect(),
            arg_handles: Vec::new(),
            destination: Place::new(RETURN_LOCAL),
            target: Some(after_call),
        },
        span,
    ));
    thunk_ctx.set_current_block(after_call);
    thunk_ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));

    LambdaInfo {
        name: thunk_name.to_string(),
        body: thunk_ctx.body,
        captures,
    }
}

/// Declare the captured allocator inside the thunk and append it to the
/// forwarded argument list, matching the trailing position a direct call gives
/// it. Returns the capture record the closure payload is built from.
fn capture_allocator(
    thunk_ctx: &mut LoweringContext,
    allocator: Option<&AllocatorCapture>,
    span: crate::error::syntax::Span,
    args: &mut Vec<Operand>,
) -> Vec<CapturedVar> {
    let Some(allocator) = allocator else {
        return Vec::new();
    };
    let local = thunk_ctx.push_param("allocator".to_string(), allocator.ty.clone(), span);
    thunk_ctx.body.env_capture_locals.push(local);
    args.push(Operand::Copy(Place::new(local)));
    vec![CapturedVar {
        name: "allocator".into(),
        lambda_local: local,
        outer_local: allocator.local,
    }]
}

/// A parameter of a function *type* may be unnamed. A positional fallback keeps
/// every thunk local distinct so one does not shadow another in the name map.
fn thunk_param_name(param: &Parameter, index: usize) -> String {
    if param.name.is_empty() {
        format!("__arg{index}")
    } else {
        param.name.clone()
    }
}
