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
use crate::mir::lowering::kept_parameters::{
    end_handed_over, hand_over_owned_locals, record_and_bind_value_parameters, value_convention_in,
};
use crate::mir::lowering::runtime_writes::{copy_forwarded_argument, RuntimeCallee};
use crate::mir::place_type::PlaceTypes;

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
    let target = ForwardingTarget {
        symbol,
        kept: ctx.type_checker.kept_parameters_of_callee(callee.id),
        func_data,
    };
    let bodies = build_forwarding_thunk(
        ctx,
        expr,
        &target,
        thunk_symbol.clone(),
        forwarded_allocator.as_ref(),
    )?;
    ctx.lambda_bodies.extend(bodies);

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

/// The function a thunk forwards to: its symbol, the parameters it keeps
/// and its declared type.
struct ForwardingTarget<'t> {
    symbol: &'t Symbol,
    /// Which parameters the function owns at the calls naming it, in
    /// parameter order; empty when it owns none.
    kept: &'t [bool],
    func_data: &'t FunctionTypeData,
}

/// Build the thunk body: `(env_ptr, p0..pN) -> symbol(p0..pN)`.
///
/// The thunk is the body every call through the value reaches, so it owns
/// what the value's calls hand over: every parameter whose declared type
/// hands its reference on
/// ([`crate::type_checker::kept_parameters::function_value_convention`]),
/// each bound to a local of its own on entry. It hands its target, by
/// `move`, exactly the parameters the target keeps at a call naming it, and
/// the rest end where the thunk's owned locals do, once the target returns:
/// the thunk releases what it was handed and its target did not keep.
///
/// A parameter a runtime entry point writes in place is forwarded as a copy
/// the thunk makes its own (`runtime_writes::copy_forwarded_argument`),
/// released once the call returns, so the write never reaches the caller's
/// value; a closure it hands a runtime closure slot is adapted
/// ([`adapt_runtime_slot_closures`]).
///
/// Answers the thunk's body, then the body of every adapter it builds.
fn build_forwarding_thunk(
    ctx: &LoweringContext,
    expr: &Expression,
    target: &ForwardingTarget<'_>,
    thunk_symbol: Symbol,
    allocator: Option<&AllocatorCapture>,
) -> Result<Vec<LambdaInfo>, LoweringError> {
    let span = expr.span;
    let func_data = target.func_data;
    let mut thunk_ctx = thunk_context(ctx, func_data, span);

    let params = declare_thunk_parameters(ctx, &mut thunk_ctx, func_data);
    let convention = value_convention_in(&thunk_ctx, func_data);
    let named = params.names.iter().map(|name| (name.as_str(), span));
    let (owned, owned_locals) =
        record_and_bind_value_parameters(&mut thunk_ctx, &convention, named, span)?;

    let func = Operand::function(target.symbol.clone(), span);
    let ForwardedArguments {
        mut args,
        copies,
        watermark,
    } = forward_parameters(&mut thunk_ctx, &func, &params, &owned_locals)?;

    let captures = capture_allocator(&mut thunk_ctx, allocator, span, &mut args);
    let slot_call = SlotCall {
        id: expr.id,
        arg_watermark: thunk_ctx.body.local_decls.len(),
        named_in: Some(ctx),
    };
    let adapters = adapt_runtime_slot_closures(&mut thunk_ctx, slot_call, &func, &mut args)?;
    let mut arg_modes: Vec<ArgMode> = (func_data.params.iter())
        .map(|param| ArgMode::from_out_flag(param.is_out))
        .collect();
    let handed = hand_over_owned_locals(
        target.kept,
        &owned_locals,
        (&mut args, &mut arg_modes),
        span,
    )?;

    let after_call = thunk_ctx.new_basic_block();
    thunk_ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args,
            arg_modes,
            arg_handles: Vec::new(),
            flags: crate::mir::CallFlags::default(),
            destination: Place::new(RETURN_LOCAL),
            target: Some(after_call),
        },
        span,
    ));
    thunk_ctx.set_current_block(after_call);
    end_handed_over(&mut thunk_ctx, &handed, span);
    for copy in copies.into_iter().chain(adapters) {
        thunk_ctx.emit_temp_drop(copy, watermark, span);
    }
    owned.close(&mut thunk_ctx, span);
    thunk_ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    thunk_ctx.body.captures.closure = Some(thunk_symbol.clone());

    let thunk = LambdaInfo {
        symbol: thunk_symbol,
        body: thunk_ctx.body,
        requests: thunk_ctx.requests,
        captures,
    };
    Ok(std::iter::once(thunk)
        .chain(thunk_ctx.lambda_bodies)
        .collect())
}

/// A fresh context for the thunk body of a function of type `func_data`,
/// holding its return slot and its environment pointer.
fn thunk_context<'a>(
    ctx: &LoweringContext<'a>,
    func_data: &FunctionTypeData,
    span: crate::error::syntax::Span,
) -> LoweringContext<'a> {
    // The thunk's types are the referenced function's declared ones, in the
    // canonical spelling its body is compiled at: `Option<int>` is `int?`.
    let ret_ty = match &func_data.return_type {
        Some(ret_expr) => ctx.declared_type(ret_expr),
        None => Type::new(TypeKind::Void, span),
    };

    let mut body = Body::new(1 + func_data.params.len(), span, ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ret_ty, span));

    let mut thunk_ctx = LoweringContext::new(body, ctx.type_checker, ctx.is_release);
    thunk_ctx.read_source_like(ctx);
    thunk_ctx.use_compilation_ids(ctx.compilation_ids.clone());
    // Read against the enclosing body's parameters, as a lambda's body is.
    thunk_ctx.body.generics.type_params = ctx.body.generics.type_params.clone();
    thunk_ctx.body.generics.open_params = ctx.body.generics.open_params.clone();
    thunk_ctx.body.generics.bound_params = ctx.body.generics.bound_params.clone();
    thunk_ctx.push_param(
        "__env_ptr".to_string(),
        Type::new(TypeKind::RawPtr, span),
        span,
    );
    thunk_ctx
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
    thunk_ctx.body.captures.env_capture_locals.push(local);
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

/// The parameters a thunk declares, one per parameter of the function type
/// it is built for: each one's name, type and local.
struct ThunkParameters {
    names: Vec<String>,
    tys: Vec<Type>,
    locals: Vec<Local>,
}

/// Declares the parameters of a thunk built for a function of type
/// `func_data`, after its environment, at the types the enclosing body reads
/// them at. Everything is spanned at the thunk body.
fn declare_thunk_parameters(
    ctx: &LoweringContext,
    thunk_ctx: &mut LoweringContext,
    func_data: &FunctionTypeData,
) -> ThunkParameters {
    let span = thunk_ctx.body.span;
    let tys: Vec<Type> = (func_data.params.iter())
        .map(|param| ctx.declared_type(&param.typ))
        .collect();
    let names: Vec<String> = (func_data.params.iter().enumerate())
        .map(|(index, param)| thunk_param_name(param, index))
        .collect();
    let locals = (names.iter().zip(&tys))
        .map(|(name, ty)| thunk_ctx.push_param(name.clone(), ty.clone(), span))
        .collect();
    ThunkParameters { names, tys, locals }
}

/// The arguments a thunk hands the function it forwards to, with the copies
/// it bound for them and the local count from before the first copy.
struct ForwardedArguments {
    args: Vec<Operand>,
    copies: Vec<Local>,
    watermark: usize,
}

/// The arguments a thunk forwards its parameters, `params`, to `func` as:
/// the local each owned one was bound to, `owned_locals`, or the parameter,
/// each as it is, or a copy of its own where `func` is a runtime entry point
/// writing it in place.
fn forward_parameters(
    thunk_ctx: &mut LoweringContext,
    func: &Operand,
    params: &ThunkParameters,
    owned_locals: &[Option<Local>],
) -> Result<ForwardedArguments, LoweringError> {
    let span = thunk_ctx.body.span;
    let callee = RuntimeCallee::of(func, &params.tys);
    let mut forwarded = ForwardedArguments {
        args: Vec::with_capacity(params.locals.len()),
        copies: Vec::new(),
        watermark: thunk_ctx.body.local_decls.len(),
    };
    let sources =
        (params.locals.iter().zip(owned_locals)).map(|(&param, owned)| owned.unwrap_or(param));
    for (position, local) in sources.enumerate() {
        let (arg, copy) = copy_forwarded_argument(thunk_ctx, callee, position, local, span)?;
        forwarded.args.push(arg);
        forwarded.copies.extend(copy);
    }
    Ok(forwarded)
}

/// Wraps each closure `args` hands a runtime function in an adapter, when
/// the closure owns a parameter: the runtime calls a closure lending every
/// argument (`@closure`), while a closure owns each one its type hands a
/// reference on at ([`value_convention_in`]). The adapter lends its
/// parameters and calls the closure through the value's convention, handing
/// each owned position a copy funded with a reference of its own, so the
/// closure releases what it was handed and the runtime's elements keep
/// theirs. Answers the temporaries holding the adapters, each of which the
/// caller releases once the call returns.
///
/// Every call reaching a runtime function is lowered through here once its
/// operands are final, and nowhere else: a direct call
/// (`dispatch::lower_direct_call`), a call through a module alias
/// (`dispatch::lower_aliased_function_call`) and the forwarding thunk of a
/// runtime function named as a value ([`build_forwarding_thunk`]). The
/// terminator setter cannot own this: it has no call to name an adapter
/// after, no block to release it in, and no way to report a failure.
///
/// The adapter for argument `position` is named after the call, `call.id`,
/// in the context the call is lowered at, `call.named_in`, or `ctx` itself;
/// a closure it replaces that the call alone holds, past
/// `call.arg_watermark`, is released once the adapter holds it.
pub(crate) fn adapt_runtime_slot_closures(
    ctx: &mut LoweringContext,
    call: SlotCall<'_, '_>,
    func: &Operand,
    args: &mut [Operand],
) -> Result<Vec<Local>, LoweringError> {
    let takes_a_closure =
        (func.called_runtime_effect()).is_some_and(|effect| !effect.closure_positions().is_empty());
    if !takes_a_closure || ctx.body.is_gpu() {
        return Ok(Vec::new());
    }
    let named_in = call.named_in.unwrap_or(ctx);
    let symbols: Vec<Symbol> = (0..args.len())
        .map(|position| named_in.closure_symbol(ClosureKind::SlotAdapter { position }, call.id))
        .collect();
    let mut adapters = Vec::new();
    for (arg, symbol) in args.iter_mut().zip(symbols) {
        let Some((closure, function_ty, convention)) = owning_closure(ctx, arg) else {
            continue;
        };
        let adapter = SlotAdapter {
            symbol,
            closure,
            function_ty,
            convention,
        };
        let local = adapter.lower(ctx, call.arg_watermark)?;
        *arg = Operand::Copy(Place::new(local));
        adapters.push(local);
    }
    Ok(adapters)
}

/// The call [`adapt_runtime_slot_closures`] adapts the closures of.
pub(crate) struct SlotCall<'n, 'a> {
    /// The AST node of the call, or of the reference a thunk forwards.
    pub id: usize,
    /// The first local the call's own argument temporaries live in.
    pub arg_watermark: usize,
    /// The context the call is lowered at, when it is not the one its
    /// statements are written into: a forwarding thunk's call is named at
    /// the instantiation its reference is lowered at.
    pub named_in: Option<&'n LoweringContext<'a>>,
}

impl SlotCall<'_, '_> {
    /// The call `id`, its argument temporaries from `arg_watermark` on,
    /// named in the context its statements are written into.
    pub(crate) fn at(id: usize, arg_watermark: usize) -> Self {
        Self {
            id,
            arg_watermark,
            named_in: None,
        }
    }
}

/// The closure `arg` carries, as a local, with its function type and the
/// positions its convention owns, when it owns any.
fn owning_closure(ctx: &mut LoweringContext, arg: &Operand) -> Option<(Local, Type, Vec<bool>)> {
    let (Operand::Copy(place) | Operand::Move(place)) = arg else {
        return None;
    };
    let ty = PlaceTypes::of(&ctx.body).declared(place)?;
    let TypeKind::Function(function) = &ty.kind else {
        return None;
    };
    let convention = value_convention_in(ctx, function);
    if !convention.contains(&true) {
        return None;
    }
    let local = if place.projection.is_empty() {
        place.local
    } else {
        let copy = ctx.push_temp(ty.clone(), ctx.body.span);
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(copy),
                Rvalue::Use(Operand::Copy(place.clone())),
            ),
            span: ctx.body.span,
        });
        copy
    };
    Some((local, ty, convention))
}

/// The adapter a runtime closure slot calls in place of a closure owning a
/// parameter.
struct SlotAdapter {
    symbol: Symbol,
    /// The local holding the closure the adapter calls, which it captures.
    closure: Local,
    function_ty: Type,
    /// The positions the closure owns.
    convention: Vec<bool>,
}

/// The name the adapter's body captures the closure it calls under.
const ADAPTED_CLOSURE: &str = "__adapted_closure";

impl SlotAdapter {
    /// Registers the adapter's body and builds the closure value over it,
    /// capturing the adapted closure, then releases the temporary the
    /// adapted closure was read into, past `arg_watermark`: the adapter
    /// holds a reference of its own.
    fn lower(
        self,
        ctx: &mut LoweringContext,
        arg_watermark: usize,
    ) -> Result<Local, LoweringError> {
        let span = ctx.body.span;
        let body = self.body(ctx)?;
        ctx.lambda_bodies.push(body);
        let target = Place::new(ctx.push_temp(self.function_ty.clone(), span));
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                target.clone(),
                Rvalue::Aggregate(
                    AggregateKind::Closure(self.symbol, self.function_ty),
                    vec![Operand::Copy(Place::new(self.closure))],
                ),
            ),
            span,
        });
        ctx.emit_temp_drop(self.closure, arg_watermark, span);
        Ok(target.local)
    }

    /// The adapter's body: `(env_ptr, p0..pN) -> closure(p0..pN)`, lending
    /// every parameter and passing each position the closure owns
    /// [`ArgMode::Owned`] by copy, which reference counting funds.
    fn body(&self, ctx: &LoweringContext) -> Result<LambdaInfo, LoweringError> {
        let span = ctx.body.span;
        let TypeKind::Function(function) = &self.function_ty.kind else {
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                "a runtime closure slot is adapted for a value of no function type",
                span,
            ));
        };
        let mut adapter_ctx = thunk_context(ctx, function, span);
        let params = declare_thunk_parameters(ctx, &mut adapter_ctx, function);
        let captured =
            adapter_ctx.push_param(ADAPTED_CLOSURE.to_string(), self.function_ty.clone(), span);
        adapter_ctx.body.captures.env_capture_locals.push(captured);
        let arg_modes = (function.params.iter().zip(&self.convention))
            .map(|(param, &is_owned)| {
                if is_owned {
                    ArgMode::Owned
                } else {
                    ArgMode::from_out_flag(param.is_out)
                }
            })
            .collect();
        let after_call = adapter_ctx.new_basic_block();
        adapter_ctx.set_terminator(Terminator::new(
            TerminatorKind::Call {
                func: Operand::Copy(Place::new(captured)),
                args: (params.locals.iter())
                    .map(|&local| Operand::Copy(Place::new(local)))
                    .collect(),
                arg_modes,
                arg_handles: Vec::new(),
                flags: crate::mir::CallFlags::default(),
                destination: Place::new(RETURN_LOCAL),
                target: Some(after_call),
            },
            span,
        ));
        adapter_ctx.set_current_block(after_call);
        adapter_ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
        adapter_ctx.body.captures.closure = Some(self.symbol.clone());
        Ok(LambdaInfo {
            symbol: self.symbol.clone(),
            body: adapter_ctx.body,
            requests: adapter_ctx.requests,
            captures: vec![CapturedVar {
                name: ADAPTED_CLOSURE.into(),
                lambda_local: captured,
                outer_local: self.closure,
            }],
        })
    }
}
