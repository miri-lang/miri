// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A named function read as a value.
//!
//! Every function-typed value in Miri is a closure struct — the layout
//! `[size][RC][fn_ptr][cap0]...` that [`super::lambda_expr`] builds — and a
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
use crate::mir::symbol::{ClosureKind, Symbol};
use crate::mir::{
    ArgMode, Body, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};

use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::runtime_writes::{copy_forwarded_argument, RuntimeCallee};

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
) -> Result<Option<Operand>, LoweringError> {
    let Some(info) = ctx.type_checker.global_scope().get(name) else {
        return Ok(None);
    };
    let TypeKind::Function(func_data) = &info.ty.kind else {
        return Ok(None);
    };
    // The type checker refuses a generic function used as a value; one that
    // reaches here would be a thunk over the shared body, laid out for no
    // caller's types.
    if func_data
        .generics
        .as_ref()
        .is_some_and(|generics| !generics.is_empty())
    {
        return Err(LoweringError::internal(
            DiagnosticCode::MirInvalidInstantiationArgument,
            format!("the generic function `{name}` reached lowering as a value"),
            expr.span,
        ));
    }
    let func_data = func_data.clone();
    let fn_ty = info.ty.clone();
    let declared = super::identifier_expr::declared_name(info, name);
    let target = super::identifier_expr::global_function_symbol(ctx, expr, declared)?;
    lower_function_reference(ctx, expr, expr, &target, &fn_ty, &func_data, dest).map(Some)
}

/// Lower `M.name`, a function read as a value through the module alias `M`,
/// as the bare `name` a plain import reads. `member` is the whole expression
/// and `name` the member it names, which carries the declaration the checker
/// resolved it to. `None` when that is not a function the program declares.
pub(crate) fn try_lower_module_member_reference(
    ctx: &mut LoweringContext,
    member: &Expression,
    name: &Expression,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let Some(declared) = ctx.type_checker.declared_callee(name.id).cloned() else {
        return Ok(None);
    };
    let Some(fn_ty) = ctx.type_checker.get_type(member.id).cloned() else {
        return Ok(None);
    };
    let TypeKind::Function(func_data) = &fn_ty.kind else {
        return Ok(None);
    };
    if func_data
        .generics
        .as_ref()
        .is_some_and(|generics| !generics.is_empty())
    {
        return Err(LoweringError::internal(
            DiagnosticCode::MirInvalidInstantiationArgument,
            format!(
                "the generic function `{}` reached lowering as a value",
                declared.name
            ),
            member.span,
        ));
    }
    let func_data = func_data.clone();
    let target = Symbol::declared_function(&declared.module, &declared.name);
    lower_function_reference(ctx, member, name, &target, &fn_ty, &func_data, dest).map(Some)
}

/// Build the closure value and register the thunk body it points at.
/// `callee` is the expression naming the function, which says whether it
/// takes an allocator.
#[allow(clippy::too_many_arguments)]
fn lower_function_reference(
    ctx: &mut LoweringContext,
    expr: &Expression,
    callee: &Expression,
    symbol: &Symbol,
    fn_ty: &Type,
    func_data: &FunctionTypeData,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    // The reference site's expression id keeps two references to the same
    // function from claiming one symbol, the way a lambda's id does.
    let thunk_symbol =
        ctx.closure_symbol(ClosureKind::FunctionReference(symbol.link_name()), expr.id);
    let forwarded_allocator = forwarded_allocator(ctx, callee);
    let thunk = build_forwarding_thunk(
        ctx,
        expr,
        symbol,
        thunk_symbol.clone(),
        func_data,
        forwarded_allocator.as_ref(),
    )?;
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
                AggregateKind::Closure(thunk_symbol, fn_ty.clone()),
                capture_operands,
            ),
        ),
        span: expr.span,
    });
    // The allocator is an unmanaged scalar, so this closure owns no captured
    // values. A stale capture-type entry left by an earlier assignment to the
    // same local would make Perceus drop fields the new payload does not have.
    ctx.body.closure_capture_types.remove(&target.local);

    Ok(Operand::Copy(target))
}

/// The enclosing function's allocator, when the referenced function takes one
/// and the enclosing body has one to give.
///
/// A Miri-defined function is lowered with an implicit trailing allocator
/// parameter, so the thunk has to supply it — and the thunk's own signature is
/// fixed by the closure calling convention. The allocator therefore travels the
/// only way anything else reaches a closure body: as a capture.
fn forwarded_allocator(ctx: &LoweringContext, reference: &Expression) -> Option<AllocatorCapture> {
    if !crate::mir::lowering::dispatch::callee_takes_allocator(ctx, reference) {
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
///
/// A parameter a runtime entry point writes in place is forwarded as a copy
/// the thunk makes its own (`runtime_writes::copy_forwarded_argument`),
/// released once the call returns, so the write never reaches the caller's
/// value.
fn build_forwarding_thunk(
    ctx: &LoweringContext,
    expr: &Expression,
    symbol: &Symbol,
    thunk_symbol: Symbol,
    func_data: &FunctionTypeData,
    allocator: Option<&AllocatorCapture>,
) -> Result<LambdaInfo, LoweringError> {
    let span = expr.span;
    let params = &func_data.params;
    // The thunk's types are the referenced function's declared ones, in the
    // canonical spelling its body is compiled at: `Option<int>` is `int?`.
    let ret_ty = match &func_data.return_type {
        Some(ret_expr) => ctx.declared_type(ret_expr),
        None => Type::new(TypeKind::Void, span),
    };

    let mut body = Body::new(1 + params.len(), span, ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ret_ty, span));

    let mut thunk_ctx = LoweringContext::new(body, ctx.type_checker, ctx.is_release);
    thunk_ctx.read_source_like(ctx);
    thunk_ctx.use_compilation_ids(ctx.compilation_ids.clone());
    // Read against the enclosing body's parameters, as a lambda's body is.
    thunk_ctx.body.type_params = ctx.body.type_params.clone();
    thunk_ctx.body.open_params = ctx.body.open_params.clone();
    thunk_ctx.body.bound_params = ctx.body.bound_params.clone();
    thunk_ctx.push_param(
        "__env_ptr".to_string(),
        Type::new(TypeKind::RawPtr, span),
        span,
    );

    let func = Operand::function(symbol.clone(), span);
    let ForwardedArguments {
        mut args,
        copies,
        watermark,
    } = forward_parameters(ctx, &mut thunk_ctx, &func, params)?;

    let captures = capture_allocator(&mut thunk_ctx, allocator, span, &mut args);

    let after_call = thunk_ctx.new_basic_block();
    thunk_ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            arg_modes: params
                .iter()
                .map(|param| ArgMode::from_out_flag(param.is_out))
                .collect(),
            arg_handles: Vec::new(),
            destination: Place::new(RETURN_LOCAL),
            target: Some(after_call),
        },
        span,
    ));
    thunk_ctx.set_current_block(after_call);
    for copy in copies {
        thunk_ctx.emit_temp_drop(copy, watermark, span);
    }
    thunk_ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));

    Ok(LambdaInfo {
        symbol: thunk_symbol,
        body: thunk_ctx.body,
        captures,
    })
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

/// The arguments a thunk hands the function it forwards to, with the copies
/// it bound for them and the local count from before the first copy.
struct ForwardedArguments {
    args: Vec<Operand>,
    copies: Vec<Local>,
    watermark: usize,
}

/// Declares the thunk's parameters, one per parameter of `func`, and the
/// arguments it forwards them as: each as it is, or a copy of its own where
/// `func` is a runtime entry point writing it in place. Everything is
/// spanned at the thunk body, the reference it is built for.
fn forward_parameters(
    ctx: &LoweringContext,
    thunk_ctx: &mut LoweringContext,
    func: &Operand,
    params: &[Parameter],
) -> Result<ForwardedArguments, LoweringError> {
    let span = thunk_ctx.body.span;
    let param_tys: Vec<Type> = params
        .iter()
        .map(|param| ctx.declared_type(&param.typ))
        .collect();
    let locals: Vec<Local> = params
        .iter()
        .zip(&param_tys)
        .enumerate()
        .map(|(index, (param, ty))| {
            thunk_ctx.push_param(thunk_param_name(param, index), ty.clone(), span)
        })
        .collect();
    let callee = RuntimeCallee::of(func, &param_tys);
    let watermark = thunk_ctx.body.local_decls.len();
    let mut forwarded = ForwardedArguments {
        args: Vec::with_capacity(params.len()),
        copies: Vec::new(),
        watermark,
    };
    for (position, &local) in locals.iter().enumerate() {
        let (arg, copy) = copy_forwarded_argument(thunk_ctx, callee, position, local, span)?;
        forwarded.args.push(arg);
        forwarded.copies.extend(copy);
    }
    Ok(forwarded)
}
