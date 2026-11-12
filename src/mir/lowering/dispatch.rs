// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Call lowering dispatcher, collection intrinsics, and constructor/direct-call fallbacks.

use crate::ast::expression::Expression;
use crate::ast::type_arguments::TypeArgument;
use crate::ast::type_arguments::{rewritten, TypeArgKind};
use crate::ast::{BuiltinCollectionKind, ExpressionKind, Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::Symbol;
use crate::mir::{
    ArgMode, Local, MathIntrinsic, Operand, Place, Rvalue, StatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::{MethodInfo, TypeDefinition};
use crate::type_checker::CalleeKind;

use super::constructors::{lower_class_constructor, lower_struct_constructor, COLLECTION_CTORS};
use super::helpers::{
    coerce_rvalue_in, gpu_math_return_type, release_coerced_source, resolve_arg_type,
    spellings_of_one_value, wrap_for_optional_slot,
};
use super::{apply_generic_sub, lower_expression, LoweringContext};
use std::collections::HashMap;
use std::rc::Rc;

/// Context for lowering a collection intrinsic method (push/get/index).
pub(super) struct CollectionIntrinsicCall<'a> {
    pub(super) span: &'a Span,
    pub(super) call_expr_id: usize,
    pub(super) obj: &'a Expression,
    pub(super) obj_ty: &'a Type,
    pub(super) method_name: &'a str,
    pub(super) args: &'a [Expression],
}

// Re-export method dispatch functions from the specialized module.
pub(crate) use super::method_dispatch::resolve_inherited_method;

// Re-export kernel launch functions from the specialized module.
pub(crate) use super::kernel_launch::try_lower_kernel_launch;

// Import private helpers from specialized modules.
use super::expression::function_reference::{adapt_runtime_slot_closures, SlotCall};
use super::generic_method_calls::{generic_method_symbol, GenericMethodSite};
use super::kept_parameters::{
    hand_over_kept_arguments, release_after_kept_call, value_call_convention, HandedOver, KeptCall,
};
use super::method_dispatch::{residency_specialize_call, with_mutated_receiver};
use super::runtime_writes::{self, RuntimeCallee};
use super::sequencing::lower_operand;
use super::written_receiver::{PendingReceiver, Receiver};

pub fn lower_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let Some(op) =
        try_lower_module_alias_call(ctx, span, call_expr_id, func, args, dest.as_ref())?
    {
        return Ok(op);
    }
    let func = written_member_callee(func);

    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) =
            try_lower_static_method_call(ctx, span, call_expr_id, obj, method, args, dest.as_ref())?
        {
            return Ok(op);
        }
    }

    if let ExpressionKind::Member(obj, prop) = &func.node {
        if let Some(op) =
            try_lower_kernel_launch(ctx, span, call_expr_id, obj, prop, args, dest.clone())?
        {
            return Ok(op);
        }
    }

    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) =
            super::drop_hook_call::try_lower_drop_hook_call(ctx, span, obj, method, args)?
        {
            return Ok(op);
        }
    }

    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) = super::derived_operator_call::try_lower_derived_member_call(
            ctx,
            span,
            call_expr_id,
            obj,
            method,
            args,
            dest.as_ref(),
        )? {
            return Ok(op);
        }
    }

    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) = super::method_dispatch::try_lower_method_call(
            ctx,
            span,
            call_expr_id,
            obj,
            method,
            args,
            dest.as_ref().cloned(),
        )? {
            return Ok(op);
        }
    }

    if let Some(op) =
        try_lower_constructor_call(ctx, span, call_expr_id, func, args, dest.as_ref())?
    {
        return Ok(op);
    }

    lower_direct_call(ctx, span, call_expr_id, func, args, dest)
}

/// The name `func` calls a function of another module by, when it calls one
/// through that module's alias: `foo` in `M.foo`.
pub(crate) fn aliased_function<'e>(
    ctx: &LoweringContext,
    func: &'e Expression,
) -> Option<&'e Expression> {
    let func = written_function_callee(ctx, func);
    let ExpressionKind::Member(obj_expr, method_expr) = &func.node else {
        return None;
    };
    let is_aliased_function = matches!(method_expr.node, ExpressionKind::Identifier(..))
        && ctx.type_checker.read_as_module_alias(obj_expr);
    is_aliased_function.then_some(method_expr.as_ref())
}

/// The callee a call writing its type arguments out — `M.foo<int>(x)` —
/// targets: the wrapped name, when the type checker resolved it to a function.
/// A wrapped type (`M.Box<int>()`) stays as written.
fn written_function_callee<'e>(ctx: &LoweringContext, func: &'e Expression) -> &'e Expression {
    let callee = callee_name_expression(func);
    let is_function = matches!(
        ctx.type_checker.get_type(callee.id).map(|ty| &ty.kind),
        Some(TypeKind::Function(_))
    );
    if is_function {
        callee
    } else {
        func
    }
}

/// The member a method call writing its type arguments out — `b.m<int>(5)` —
/// calls, else `func` as written. The type checker records what the written
/// arguments bind against the call itself, which is where they are read.
fn written_member_callee(func: &Expression) -> &Expression {
    let callee = callee_name_expression(func);
    if matches!(callee.node, ExpressionKind::Member(..)) {
        callee
    } else {
        func
    }
}

/// Lower a call to a function in another module via its alias: `M.foo(args)`.
fn try_lower_module_alias_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    let Some(method_expr) = aliased_function(ctx, func) else {
        return Ok(None);
    };
    if let Some(intrinsic) = math_intrinsic_callee(ctx, method_expr) {
        let call = MathCall {
            signature_id: func.id,
            call_expr_id,
            span: *span,
        };
        return lower_math_intrinsic_call(ctx, call, intrinsic, args, dest.cloned()).map(Some);
    }
    lower_aliased_function_call(
        ctx,
        span,
        call_expr_id,
        func.id,
        method_expr,
        args,
        dest.cloned(),
    )
}

/// Lower a call to a static method on a class or enum: `Duration.from_millis(ms)` or `MyEnum.create()`.
fn try_lower_static_method_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj_expr: &Expression,
    method_expr: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    // Static method calls have the form TypeName.method_name(args)
    // where TypeName is an identifier (type name), not an instance
    let ExpressionKind::Identifier(..) = &obj_expr.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(method_name, _) = &method_expr.node else {
        return Ok(None);
    };
    let Some(type_name) = ctx.type_checker.type_identity_named_by(obj_expr) else {
        return Ok(None);
    };

    // Try to find the static method in a class inheritance chain first
    if let Some((defining_class_name, method_info)) = ctx
        .type_checker
        .find_static_method_in_chain(type_name, method_name.as_str())
    {
        if method_info.is_static {
            let site = StaticMethodSite {
                expr: method_expr,
                name: method_name,
                defining_type: &defining_class_name,
                info: &method_info,
            };
            return lower_static_method_impl(ctx, (span, call_expr_id), site, args, dest);
        }
    }

    // Try to find the static method on an enum
    // Borrow `enum_def` directly from `global_type_definitions` instead of cloning
    // the entire `EnumDefinition` node (which includes maps of variants and methods).
    if let Some(TypeDefinition::Enum(enum_def)) = ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(type_name)
    {
        if let Some(method_info) = enum_def.methods.get(method_name) {
            if method_info.is_static {
                let site = StaticMethodSite {
                    expr: method_expr,
                    name: method_name,
                    defining_type: type_name,
                    info: method_info,
                };
                return lower_static_method_impl(ctx, (span, call_expr_id), site, args, dest);
            }
        }
    }

    Ok(None)
}

/// A static method a call names: the expression naming it, its name, the
/// type declaring it and its signature.
struct StaticMethodSite<'a> {
    expr: &'a Expression,
    name: &'a str,
    defining_type: &'a str,
    info: &'a MethodInfo,
}

fn lower_static_method_impl(
    ctx: &mut LoweringContext,
    (span, call_expr_id): (&Span, usize),
    site: StaticMethodSite<'_>,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    let StaticMethodSite {
        expr: method_expr,
        name: method_name,
        defining_type: defining_type_name,
        info: method_info,
    } = site;
    // Static method call with no receiver
    let arg_watermark = ctx.body.local_decls.len();
    let param_names: Vec<&str> = method_info
        .params
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let mut arg_ops = lower_args_by_name(ctx, args, &param_names, *span)?;
    push_allocator_arg(ctx, &mut arg_ops);

    let return_ty = ctx
        .recorded_type(call_expr_id)
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, result_op) = call_destination(ctx, return_ty, dest.cloned(), *span);

    let symbol = if !method_info.declares_type_parameters() {
        Symbol::method(defining_type_name, &[], method_name, &[])
    } else {
        let site = GenericMethodSite {
            receiver: None,
            class_name: defining_type_name,
            defining_class: defining_type_name,
            method_name,
            parameters: &method_info.generics,
            is_static: method_info.is_static,
            call_expr_id,
            span: *span,
        };
        generic_method_symbol(ctx, &site)?
    };
    let func_op = Operand::function(symbol, *span);

    // Build arg_modes from method_info with no receiver offset (static methods have no self).
    let mut arg_modes =
        build_method_arg_modes_with_offset(method_info, args.len(), arg_ops.len(), 0);
    let call = KeptCall {
        span: method_expr.span,
        receiver: None,
        kept: (ctx.type_checker).kept_parameters_of_method(defining_type_name, method_name),
        args,
        arg_watermark,
        destination: destination.local,
    }
    .checked_against_method(method_info)?;
    let handed = hand_over_kept_arguments(ctx, &call, &mut arg_ops, &mut arg_modes)?;
    let arg_locals = operand_locals(&arg_ops);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops,
        arg_modes,
        Vec::new(), // arg_handles
        destination,
        *span,
    );
    release_after_kept_call(ctx, &call, &arg_locals, &handed, *span);
    Ok(Some(result_op))
}

/// The math intrinsic a call to `callee` lowers to: the name must resolve,
/// where it is written, to a declaration made `intrinsic` and carry the name
/// of a [`MathIntrinsic`].
/// The declaration, not the module that holds it, decides — a plain function
/// that happens to share an intrinsic's name stays an ordinary call, and a
/// module body's call to its own intrinsic lowers as one even where the
/// program's imports leave that intrinsic out of its scope.
pub(crate) fn math_intrinsic_callee(
    ctx: &LoweringContext,
    callee: &Expression,
) -> Option<MathIntrinsic> {
    let ExpressionKind::Identifier(name, _) = &callee.node else {
        return None;
    };
    (ctx.callee_kind(callee) == CalleeKind::Intrinsic)
        .then(|| {
            MathIntrinsic::from_name(ctx.type_checker.intrinsic_name(callee.id).unwrap_or(name))
        })
        .flatten()
}

/// A call to a math intrinsic: the expression the type checker typed as the
/// intrinsic (`pow`, or `M.pow` through a module alias), whose parameters the
/// arguments bind, and the call expression itself.
#[derive(Clone, Copy)]
pub(crate) struct MathCall {
    pub(crate) signature_id: usize,
    pub(crate) call_expr_id: usize,
    pub(crate) span: Span,
}

/// Lower a call to a math intrinsic to a `MathIntrinsic` rvalue, however the
/// intrinsic is named: bare or through a module alias.
///
/// Each argument is evaluated in the order it is written and passed at the
/// parameter it binds. `min` and `max` combine their arguments as an operator
/// does: each is converted to the type the checker settled on, which holds
/// all of them.
pub(crate) fn lower_math_intrinsic_call(
    ctx: &mut LoweringContext,
    call: MathCall,
    intrinsic: MathIntrinsic,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let span = call.span;
    let return_ty = ctx
        .type_checker
        .get_type(call.call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, span));
    let combines = matches!(intrinsic, MathIntrinsic::Min | MathIntrinsic::Max);
    let param_names = callee_parameter_names(ctx, call.signature_id);
    let names: Vec<&str> = param_names.iter().map(String::as_str).collect();
    let mut slots = Vec::with_capacity(args.len());
    for (position, value) in crate::ast::expression::bind_arguments_to_parameters(args, &names) {
        let lowered = lower_operand(ctx, value)?;
        let op = if combines {
            convert_to_combined(ctx, lowered, value, &return_ty, span)
        } else {
            lowered
        };
        place_at_parameter(&mut slots, position, op);
    }
    let arg_ops = operands_in_parameter_order(slots, span)?;
    let return_ty = gpu_math_return_type(ctx, args, return_ty, span);
    let (target, ret_op) = call_destination(ctx, return_ty, dest, span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(target, Rvalue::MathIntrinsic(intrinsic, arg_ops)),
        span,
    });
    Ok(ret_op)
}

/// `operand`, the value of `value`, converted to the number type `combined`
/// when it is another number type that `combined` holds.
fn convert_to_combined(
    ctx: &mut LoweringContext,
    operand: Operand,
    value: &Expression,
    combined: &Type,
    span: Span,
) -> Operand {
    let Some(value_ty) = ctx.recorded_type(value.id) else {
        return operand;
    };
    if value_ty.kind == combined.kind
        || crate::ast::types::common_numeric_type(&value_ty.kind, &combined.kind).is_none()
    {
        return operand;
    }
    let target = Type::new(combined.kind.clone(), span);
    let temp = ctx.push_temp(target.clone(), span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(temp), Rvalue::Cast(Box::new(operand), target)),
        span,
    });
    Operand::Copy(Place::new(temp))
}

/// Lower a direct call to the function `callee` names through a module alias.
/// `signature_id` is the expression the type checker typed as the function
/// the alias names (`M.foo`); its parameters are what the arguments bind.
fn lower_aliased_function_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    signature_id: usize,
    callee: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Ok(None);
    };
    let symbol = match generic_function_symbol(ctx, callee, call_expr_id, *span)? {
        Some(symbol) => symbol,
        None => super::expression::identifier_expr::global_function_symbol(ctx, callee, func_name)?,
    };
    let func_op = Operand::function(symbol, *span);

    // The arguments are handled as a direct call's are: each brought to its
    // parameter's type, a named one placed at the parameter it names, an
    // omitted one given its default, and every temp one built released once
    // the call has taken its own reference.
    let arg_watermark = ctx.body.local_decls.len();
    let call = CallSite {
        callee_id: signature_id,
        span: *span,
    };
    let mut arg_ops = lower_call_arguments(ctx, call, call_expr_id, args, &func_op)?;
    if callee_takes_allocator(ctx, callee) {
        push_allocator_arg(ctx, &mut arg_ops);
    }
    let slot_call = SlotCall::at(call_expr_id, arg_watermark);
    adapt_runtime_slot_closures(ctx, slot_call, &func_op, &mut arg_ops)?;

    let return_ty = ctx
        .recorded_type(call_expr_id)
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, result_op) = call_destination(ctx, return_ty, dest, *span);
    let at = (arg_watermark, destination.local);
    let named = (callee, signature_id);
    let call = KeptCall::of(ctx.type_checker, named, &func_op, None, args, at)?;
    let mut arg_modes = Vec::new();
    let handed = hand_over_kept_arguments(ctx, &call, &mut arg_ops, &mut arg_modes)?;

    let arg_locals = operand_locals(&arg_ops);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops,
        arg_modes,
        Vec::new(),
        destination,
        *span,
    );
    end_direct_call(ctx, (callee, *span), &call, &arg_locals, &handed);
    Ok(Some(result_op))
}

/// A call's argument operands in parameter order, for the function `func`
/// the callee expression `call.callee_id` names: each lowered in written
/// order and brought to its parameter's type, a named one placed at the
/// parameter it names, and an omitted one given its default. An argument a
/// runtime entry point writes in place is lowered as the collection the call
/// writes ([`runtime_writes::lower_call_argument`]).
fn lower_call_arguments(
    ctx: &mut LoweringContext,
    call: CallSite,
    call_expr_id: usize,
    args: &[Expression],
    func: &Operand,
) -> Result<Vec<Operand>, LoweringError> {
    let CallSite { callee_id, .. } = call;
    let is_generic_call = ctx
        .type_checker
        .call_generic_mappings
        .contains_key(&call_expr_id);
    let param_types = resolve_param_types(ctx, callee_id);
    let (targets, callee_subs) = if is_generic_call {
        let subs = ctx
            .instantiated_call_mapping(call_expr_id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        (
            generic_call_parameter_types(ctx, callee_id, call_expr_id),
            subs,
        )
    } else {
        (declared_parameter_types(ctx, &param_types), HashMap::new())
    };
    let param_names = callee_parameter_names(ctx, callee_id);
    let callee = RuntimeCallee::of(func, &targets);
    let (mut arg_slots, written) =
        lower_and_coerce_args(ctx, args, &targets, &param_names, callee)?;
    let callee_params = CalleeParameters {
        params: param_types.as_deref().unwrap_or_default(),
        types: &targets,
        subs: callee_subs,
    };
    fill_default_args(ctx, &mut arg_slots, callee_params, call)?;
    for argument in written {
        let op = argument.pending.make_own(ctx)?;
        let op = coerce_written_arg(ctx, op, argument.value, targets.get(argument.position));
        place_at_parameter(&mut arg_slots, argument.position, op);
    }
    operands_in_parameter_order(arg_slots, call.span)
}

/// Lower call arguments with plain expression lowering (no coercion), each
/// in the order it is written and passed at the parameter it binds: a named
/// argument at the parameter of `param_names` it names, every other at its
/// position.
fn lower_args_by_name(
    ctx: &mut LoweringContext,
    args: &[Expression],
    param_names: &[&str],
    span: Span,
) -> Result<Vec<Operand>, LoweringError> {
    let mut slots = Vec::with_capacity(args.len());
    for (position, value) in crate::ast::expression::bind_arguments_to_parameters(args, param_names)
    {
        let op = lower_operand(ctx, value)?;
        place_at_parameter(&mut slots, position, op);
    }
    operands_in_parameter_order(slots, span)
}

/// Append the implicit `allocator` argument unless it is already present.
fn push_allocator_arg(ctx: &LoweringContext, arg_ops: &mut Vec<Operand>) {
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        let already_has_alloc = arg_ops
            .iter()
            .any(|op| matches!(op, Operand::Copy(p) | Operand::Move(p) if p.local == alloc_local));
        if !already_has_alloc {
            arg_ops.push(Operand::Copy(Place::new(alloc_local)));
        }
    }
}

pub(super) fn is_collection_type(name: &str) -> bool {
    matches!(
        BuiltinCollectionKind::from_name(name),
        Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
    )
}

/// Build the per-argument [`ArgMode`] list for a method call's argument vector.
pub(super) fn build_method_arg_modes(
    method_info: &MethodInfo,
    user_arg_count: usize,
    total_call_args: usize,
) -> Vec<ArgMode> {
    build_method_arg_modes_with_offset(method_info, user_arg_count, total_call_args, 1)
}

/// Build the per-argument [`ArgMode`] list for a method call, accounting for
/// receiver offset.
/// For instance methods, `receiver_offset` is 1 (self is at slot 0).
/// For static methods, `receiver_offset` is 0 (no self).
pub(super) fn build_method_arg_modes_with_offset(
    method_info: &MethodInfo,
    user_arg_count: usize,
    total_call_args: usize,
    receiver_offset: usize,
) -> Vec<ArgMode> {
    let mut modes = vec![ArgMode::Borrowed; total_call_args];
    for i in 0..user_arg_count {
        if method_info.is_param_out(i) {
            modes[receiver_offset + i] = ArgMode::Out;
        }
    }
    modes
}

/// Lower element_at/get on List, Array, or Tuple.
fn lower_collection_element_access(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    obj_ty: &Type,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let obj_watermark = ctx.body.local_decls.len();
    // Inside a monomorphized generic-class method, a `List<T>`/`Array<T,N>` field
    // carries the unsubstituted parameter `T`. Resolving it to the concrete
    // instantiation type makes codegen address the element at its true width and
    // stride (a `T = f32` element loads 4 bytes, not the pointer-width fallback).
    let obj_ty = substitute_collection_element_ty(obj_ty, &ctx.generic_subs);
    let obj_op = lower_operand(ctx, obj)?;
    let obj_op_src = obj_op.place().map(|place| place.local);
    let index_op = lower_operand(ctx, &args[0])?;

    let alias_ty = receiver_alias_type(ctx, &obj_op, &obj_ty);
    let obj_local = store_operand_temp(ctx, obj_op, alias_ty, *span);
    let index_local = materialize_index_local(ctx, index_op, args[0].span);

    let mut indexed_place = Place::new(obj_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::index(index_local));

    let elem_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .map(|ty| crate::mir::lowering::apply_generic_sub(&ty, &ctx.generic_subs))
        .unwrap_or_else(|| Type::new(TypeKind::Int, *span));
    let (destination, op) = call_destination(ctx, elem_ty, dest, *span);

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(destination, Rvalue::Use(Operand::Copy(indexed_place))),
        span: *span,
    });

    ctx.emit_temp_drop(obj_local, obj_watermark, *span);
    if let Some(src_local) = obj_op_src {
        ctx.emit_temp_drop(src_local, obj_watermark, *span);
    }
    Ok(op)
}

/// The type the temporary holding a collection receiver read from `obj_op`
/// is declared at.
///
/// A receiver read straight out of a local declared at the bare collection
/// class — `self` in a collection's own shared body, typed `List` — keeps that
/// type: the temporary is an alias of the local, and reference counting must
/// judge the two alike, or the alias is released without the copy ever being
/// retained. Any other receiver is declared at `obj_ty`, the type its element
/// width is read from.
fn receiver_alias_type(ctx: &LoweringContext, obj_op: &Operand, obj_ty: &Type) -> Type {
    let (Operand::Copy(place) | Operand::Move(place)) = obj_op else {
        return obj_ty.clone();
    };
    let declared = &ctx.body.local_decls[place.local.0].ty;
    let is_bare_collection = matches!(
        &declared.kind,
        TypeKind::Custom(name, None) if BuiltinCollectionKind::from_name(name).is_some()
    );
    if place.projection.is_empty() && is_bare_collection {
        declared.clone()
    } else {
        obj_ty.clone()
    }
}

/// Substitute a generic class's type parameters into a collection type's
/// element type. A `List<T>`/`Array<T,N>` field read inside a monomorphized
/// method sees the unsubstituted `T`; mapping it to the concrete instantiation
/// type (`List<f32>`) lets codegen resolve the element's width and stride.
/// Non-collection or already-concrete types pass through unchanged, and an empty
/// substitution (the non-monomorphized path) is a no-op clone.
fn substitute_collection_element_ty(obj_ty: &Type, subs: &HashMap<String, Type>) -> Type {
    if subs.is_empty() {
        return obj_ty.clone();
    }
    let sub_expr = |expr: &TypeArgument| -> TypeArgument {
        match &expr.node {
            TypeArgKind::Type(ty, is_ref) => rewritten(
                expr,
                TypeArgKind::Type(Box::new(apply_generic_sub(ty, subs)), *is_ref),
            ),
            TypeArgKind::Value(_) => expr.clone(),
        }
    };
    let new_kind = match &obj_ty.kind {
        TypeKind::List(elem) => TypeKind::List(Box::new(sub_expr(elem))),
        TypeKind::Array(elem, size) => TypeKind::Array(Box::new(sub_expr(elem)), size.clone()),
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
            ) =>
        {
            TypeKind::Custom(name.clone(), Some(args.iter().map(sub_expr).collect()))
        }
        _ => return obj_ty.clone(),
    };
    Type::new(new_kind, obj_ty.span)
}

/// Materialize an index operand into a bare local, spilling to a temp when it is
/// a projected place or a constant.
fn materialize_index_local(ctx: &mut LoweringContext, index_op: Operand, span: Span) -> Local {
    match index_op {
        Operand::Copy(p) | Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => store_operand_temp(ctx, index_op, Type::new(TypeKind::Int, span), span),
    }
}

/// Resolve a call's destination place + return operand, using `dest` when given.
pub(super) fn call_destination(
    ctx: &mut LoweringContext,
    return_ty: Type,
    dest: Option<Place>,
    span: Span,
) -> (Place, Operand) {
    match dest {
        Some(d) => (d.clone(), Operand::Copy(d)),
        None => {
            let temp = ctx.push_temp(return_ty, span);
            let p = Place::new(temp);
            (p.clone(), Operand::Copy(p))
        }
    }
}

/// The kernel a gpu fn callee expression names.
pub(super) fn resolve_kernel_symbol(
    ctx: &LoweringContext,
    callee: &Expression,
    span: Span,
) -> Result<Symbol, LoweringError> {
    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu fn must be called by name".to_string(),
            span,
        ));
    };

    let type_args = ctx.instantiated_call_mapping(callee.id).unwrap_or_default();
    refuse_unnameable_call_mapping(ctx, func_name, &type_args, span)?;
    let kernel = ctx.declared_callee(callee, func_name)?;
    Ok(Symbol::function(
        &kernel.module,
        &kernel.name,
        type_args.iter().map(|(_, ty)| ty),
    ))
}

/// Store an operand into a fresh temp of `ty`, returning the temp local.
fn store_operand_temp(ctx: &mut LoweringContext, op: Operand, ty: Type, span: Span) -> Local {
    let local = ctx.push_temp(ty, span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(local), Rvalue::Use(op)),
        span,
    });
    local
}

/// The host local a receiver expression names directly, when the local is
/// itself declared as the built-in collection `kind`: a binding, not a capture
/// or a gpu-resident one, read without lowering anything. A local declared at
/// another type — an optional narrowed to the collection, say — is left to the
/// method call, since a read of the header goes by the local's own type.
fn host_binding(
    ctx: &LoweringContext,
    obj: &Expression,
    kind: BuiltinCollectionKind,
) -> Option<Local> {
    let ExpressionKind::Identifier(name, _) = &obj.node else {
        return None;
    };
    let local = *ctx.variable_map.get(name.as_str())?;
    let decl = ctx.body.local_decls.get(local.0)?;
    let is_host =
        decl.device_handle.is_none() && !ctx.body.captures.env_capture_locals.contains(&local);
    let is_declared_kind = decl.ty.kind.as_builtin_collection() == Some(kind);
    (is_host && is_declared_kind && !ctx.body.is_gpu()).then_some(local)
}

/// Lower `collection.length()` on a collection a local holds to a read of the
/// count its header keeps, instead of a call to the method and from it to
/// the runtime. Like the length a `for` loop reads, it is not validated by the
/// heap guard.
fn lower_collection_length(
    ctx: &mut LoweringContext,
    local: Local,
    dest: Option<Place>,
    span: &Span,
) -> Operand {
    let destination =
        dest.unwrap_or_else(|| Place::new(ctx.push_temp(Type::new(TypeKind::Int, *span), *span)));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(destination.clone(), Rvalue::Len(Place::new(local))),
        span: *span,
    });
    Operand::Copy(destination)
}

/// Lower list.push(item) to miri_rt_list_push.
fn lower_list_push(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    item_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let cow = Some((obj_ty, &crate::mir::compiler_intrinsics::LIST_COW));
    with_mutated_receiver(ctx, obj, cow, *span, |ctx, obj_op| {
        emit_list_push(ctx, obj_op, obj_ty, item_arg, span).map(Some)
    })
}

/// Append `item_arg` to the list `obj_op` of type `obj_ty` through
/// `miri_rt_list_push`, which keeps it.
///
/// Shared by `list.push(e)` and a list literal, for the reason
/// [`emit_set_add`] gives.
pub(crate) fn emit_list_push(
    ctx: &mut LoweringContext,
    receiver: Receiver<'_>,
    obj_ty: &Type,
    item_arg: &Expression,
    span: &Span,
) -> Result<Operand, LoweringError> {
    let item_watermark = ctx.body.local_decls.len();
    let (item_op, _) = lower_stored_value(ctx, item_arg, obj_ty, ELEMENT_SLOT)?;

    let item_op_src = item_op.place().map(|place| place.local);
    let obj_op = receiver.make_own(ctx)?;
    let func_op = ctx.declared_runtime_operand(rt::LIST_PUSH, *span)?;
    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
    let args = vec![obj_op, item_op];
    emit_call_terminator(
        ctx,
        func_op,
        args,
        Vec::new(),
        Vec::new(),
        Place::new(dummy_dest),
        *span,
    );
    if let Some(src) = item_op_src {
        ctx.emit_temp_drop(src, item_watermark, item_arg.span);
    }
    Ok(Operand::Copy(Place::new(dummy_dest)))
}

/// Convert `op` to the slot's type when it arrived as something narrower.
///
/// A collection reads an element back at its slot's width, so a value that
/// filled only part of the slot would read back as a different number.
/// The cast extends by the source's signedness, so a negative `int` reaches the
/// wider slot as the same number.
fn widen_operand_to_slot(
    ctx: &mut LoweringContext,
    op: Operand,
    op_ty: &Type,
    slot_ty: &Type,
    span: Span,
) -> Operand {
    if op_ty.kind == slot_ty.kind {
        return op;
    }
    let local = ctx.push_temp(slot_ty.clone(), span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::Cast(Box::new(op), slot_ty.clone()),
        ),
        span,
    });
    Operand::Copy(Place::new(local))
}

/// Store `op` into a fresh temp and hand back the address of that temp.
///
/// A runtime entry point that reads a value's bytes out of memory needs an
/// address for a value that has none of its own. The reference is only read
/// during the call, which copies the bytes out, so the temp outliving the call
/// is all the address needs.
pub(super) fn spill_operand_to_address(
    ctx: &mut LoweringContext,
    op: Operand,
    ty: Type,
    span: Span,
) -> Operand {
    let value_local = store_operand_temp(ctx, op, ty, span);
    let addr_local = ctx.push_temp(Type::new(TypeKind::RawPtr, span), span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(addr_local), Rvalue::Ref(Place::new(value_local))),
        span,
    });
    Operand::Copy(Place::new(addr_local))
}

/// The type argument naming a `List`/`Array`/`Set` element or a `Map` key.
pub(super) const ELEMENT_SLOT: usize = 0;
/// The type argument naming a `Map` value.
pub(super) const MAP_VALUE_SLOT: usize = 1;

/// Lower a value a collection is about to store in the slot its `slot`-th type
/// argument declares, returning the operand and the type it is stored as.
///
/// The type is the one the type checker recorded for the expression rather than
/// the operand's base local, so a projected value (a field read) keeps its own
/// type. A bare value headed for an optional slot is wrapped as `Some` here,
/// because the store writes exactly the operand it receives.
pub(super) fn lower_stored_value(
    ctx: &mut LoweringContext,
    value_arg: &Expression,
    collection_ty: &Type,
    slot: usize,
) -> Result<(Operand, Type), LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let op = lower_operand(ctx, value_arg)?;
    let op_ty = resolve_arg_type(ctx, value_arg, &op);
    let Some(slot_ty) = collection_slot_type(ctx, collection_ty, slot) else {
        return Ok((op, op_ty));
    };
    let (op, op_ty) = wrap_for_optional_slot(ctx, op, op_ty, &slot_ty, watermark, value_arg.span);
    Ok(conform_operand_to_slot(
        ctx,
        op,
        op_ty,
        &slot_ty,
        value_arg.span,
    ))
}

/// Convert `op` to the collection's declared slot type when both are numbers of
/// different types.
///
/// A set or a map copies its whole slot out of the buffer the caller points it
/// at, so an operand narrower than the slot would leave the rest of it filled by
/// whatever lay beside it, and one wider would be read back short. Both the
/// store and every lookup that has to match it go through here, so they agree on
/// the representation whatever the call site wrote.
///
/// Only numbers are converted: a managed slot holds a reference, which is one
/// word wherever it came from, and a slot whose type the enclosing
/// instantiation has not pinned has no width to convert to.
pub(super) fn conform_operand_to_slot(
    ctx: &mut LoweringContext,
    op: Operand,
    op_ty: Type,
    slot_ty: &Type,
    span: Span,
) -> (Operand, Type) {
    if op_ty.kind == slot_ty.kind
        || !ctx.type_checker.is_numeric_type(&op_ty.kind)
        || !ctx.type_checker.is_numeric_type(&slot_ty.kind)
    {
        return (op, op_ty);
    }
    let converted = widen_operand_to_slot(ctx, op, &op_ty, slot_ty, span);
    (converted, slot_ty.clone())
}

/// Convert the value a lookup asks `collection` about to the type `collection`
/// stores its elements as.
///
/// A lookup matches the bytes a store wrote, so it has to spell the element the
/// way the store did. The collection's type is the one the type checker
/// recorded, read through the active instantiation; a lookup against something
/// the checker did not type is left as it was lowered.
pub(super) fn conform_lookup_operand(
    ctx: &mut LoweringContext,
    op: Operand,
    element: &Expression,
    collection: &Expression,
) -> Operand {
    let Some(collection_ty) = ctx.recorded_type(collection.id) else {
        return op;
    };
    let element_ty = resolve_arg_type(ctx, element, &op);
    conform_operand_to_collection_slot(
        ctx,
        op,
        element_ty,
        &collection_ty,
        ELEMENT_SLOT,
        element.span,
    )
    .0
}

/// Convert an operand a collection is about to store or look up to the type its
/// `slot`-th type argument declares.
///
/// The sites that lower an element without going through [`lower_stored_value`]
/// reach the same rule through here.
pub(super) fn conform_operand_to_collection_slot(
    ctx: &mut LoweringContext,
    op: Operand,
    op_ty: Type,
    collection_ty: &Type,
    slot: usize,
    span: Span,
) -> (Operand, Type) {
    let Some(slot_ty) = collection_slot_type(ctx, collection_ty, slot) else {
        return (op, op_ty);
    };
    conform_operand_to_slot(ctx, op, op_ty, &slot_ty, span)
}

/// The type a collection declares for the slot named by its `slot`-th type
/// argument, resolved against the enclosing generic substitution.
///
/// Only a built-in collection answers: a user class's type arguments say
/// nothing about what its own index operator stores.
pub(super) fn collection_slot_type(
    ctx: &LoweringContext,
    collection_ty: &Type,
    slot: usize,
) -> Option<Type> {
    collection_ty.kind.as_builtin_collection()?;
    let arg = if let TypeKind::Custom(_, args) = &collection_ty.kind {
        args.as_ref()?.get(slot)?
    } else if let TypeKind::Map(key, value) = &collection_ty.kind {
        [key, value].get(slot).copied()?
    } else if let TypeKind::List(elem) | TypeKind::Set(elem) | TypeKind::Array(elem, _) =
        &collection_ty.kind
    {
        (slot == ELEMENT_SLOT).then_some(elem)?
    } else {
        return None;
    };
    Some(ctx.resolved_argument_type(arg))
}

/// Lower map.set(key, value) to miri_rt_map_set.
///
/// Lowered here rather than through the stdlib method so the receiver goes
/// through the same copy-on-write seam as `lower_list_push`; the key and the
/// value reach the entry point directly, which keeps both.
fn lower_map_set(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    key_arg: &Expression,
    value_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let cow = Some((obj_ty, &crate::mir::compiler_intrinsics::MAP_COW));
    with_mutated_receiver(ctx, obj, cow, *span, |ctx, obj_op| {
        emit_map_set(ctx, obj_op, obj_ty, key_arg, value_arg, span).map(Some)
    })
}

/// Store `key_arg` → `value_arg` into the map `obj_op` of type `obj_ty` through
/// `miri_rt_map_set`, which keeps both.
///
/// Shared by `map.set(k, v)` and a map literal, which fills the map it has
/// just built one entry at a time, so both seams store an entry the same way.
pub(crate) fn emit_map_set(
    ctx: &mut LoweringContext,
    receiver: Receiver<'_>,
    obj_ty: &Type,
    key_arg: &Expression,
    value_arg: &Expression,
    span: &Span,
) -> Result<Operand, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let (key_op, _) = lower_stored_value(ctx, key_arg, obj_ty, ELEMENT_SLOT)?;
    let key_src = key_op.place().map(|place| place.local);
    let (value_op, _) = lower_stored_value(ctx, value_arg, obj_ty, MAP_VALUE_SLOT)?;
    let value_src = value_op.place().map(|place| place.local);
    let obj_op = receiver.make_own(ctx)?;

    let func_op = ctx.declared_runtime_operand(rt::MAP_SET, *span)?;
    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
    let args = vec![obj_op, key_op, value_op];
    emit_call_terminator(
        ctx,
        func_op,
        args,
        Vec::new(),
        Vec::new(),
        Place::new(dummy_dest),
        *span,
    );
    if let Some(src) = key_src {
        ctx.emit_temp_drop(src, watermark, key_arg.span);
    }
    if let Some(src) = value_src {
        ctx.emit_temp_drop(src, watermark, value_arg.span);
    }
    Ok(Operand::Copy(Place::new(dummy_dest)))
}

/// Lower set.add(element) to miri_rt_set_add, which keeps the element.
///
/// Mirrors [`lower_map_set`]; the intrinsic reports whether the element was
/// newly inserted, so the call keeps its boolean result.
fn lower_set_add(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    elem_arg: &Expression,
    dest: Option<Place>,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let cow = Some((obj_ty, &crate::mir::compiler_intrinsics::SET_COW));
    with_mutated_receiver(ctx, obj, cow, *span, |ctx, obj_op| {
        emit_set_add(ctx, obj_op, obj_ty, elem_arg, dest, span).map(Some)
    })
}

/// Add `elem_arg` to the set `obj_op` of type `obj_ty` through
/// `miri_rt_set_add`, which keeps it, and answer whether it was newly
/// inserted.
///
/// Shared by `set.add(e)` and a set literal, which fills the set it has just
/// built one element at a time, so both seams store an element the same way.
pub(crate) fn emit_set_add(
    ctx: &mut LoweringContext,
    receiver: Receiver<'_>,
    obj_ty: &Type,
    elem_arg: &Expression,
    dest: Option<Place>,
    span: &Span,
) -> Result<Operand, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let (elem_op, _) = lower_stored_value(ctx, elem_arg, obj_ty, ELEMENT_SLOT)?;
    let elem_src = elem_op.place().map(|place| place.local);
    let obj_op = receiver.make_own(ctx)?;

    // The intrinsic reports whether the element was newly inserted, so the
    // result has to land in the caller's destination when it asked for one.
    let destination = dest
        .unwrap_or_else(|| Place::new(ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span)));
    let func_op = ctx.declared_runtime_operand(rt::SET_ADD, *span)?;
    let args = vec![obj_op, elem_op];
    emit_call_terminator(
        ctx,
        func_op,
        args,
        Vec::new(),
        Vec::new(),
        destination.clone(),
        *span,
    );
    if let Some(src) = elem_src {
        ctx.emit_temp_drop(src, watermark, elem_arg.span);
    }
    Ok(Operand::Copy(destination))
}

/// Lower list.insert(index, item) to miri_rt_list_insert.
fn lower_list_insert(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    index_arg: &Expression,
    item_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let item_watermark = ctx.body.local_decls.len();
    let cow = Some((obj_ty, &crate::mir::compiler_intrinsics::LIST_COW));
    with_mutated_receiver(ctx, obj, cow, *span, |ctx, obj_op| {
        emit_list_insert(
            ctx,
            obj_op,
            obj_ty,
            (index_arg, item_arg),
            item_watermark,
            span,
        )
    })
}

/// Insert `item_arg` at `index_arg` into the list `obj_op` of type `obj_ty`
/// through `miri_rt_list_insert`, which keeps it.
fn emit_list_insert(
    ctx: &mut LoweringContext,
    receiver: Receiver<'_>,
    obj_ty: &Type,
    (index_arg, item_arg): (&Expression, &Expression),
    item_watermark: usize,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let index_op = lower_operand(ctx, index_arg)?;
    let (item_op, _) = lower_stored_value(ctx, item_arg, obj_ty, ELEMENT_SLOT)?;

    let item_op_src = item_op.place().map(|place| place.local);
    let obj_op = receiver.make_own(ctx)?;
    let func_op = ctx.declared_runtime_operand(rt::LIST_INSERT, *span)?;
    let result_temp = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    let args = vec![obj_op, index_op, item_op];
    emit_call_terminator(
        ctx,
        func_op,
        args,
        Vec::new(),
        Vec::new(),
        Place::new(result_temp),
        *span,
    );
    if let Some(src) = item_op_src {
        ctx.emit_temp_drop(src, item_watermark, item_arg.span);
    }
    Ok(Some(Operand::Copy(Place::new(result_temp))))
}

/// Lower list/array.set(index, value) to a direct indexed assignment.
fn lower_collection_set(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    index_arg: &Expression,
    item_arg: &Expression,
    builtin: Option<BuiltinCollectionKind>,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let obj_watermark = ctx.body.local_decls.len();
    let cow = builtin
        .map(crate::mir::compiler_intrinsics::copy_on_write_of)
        .map(|cow| (obj_ty, cow));
    with_mutated_receiver(ctx, obj, cow, *span, |ctx, obj_op| {
        emit_collection_set(
            ctx,
            obj_op,
            obj_ty,
            (index_arg, item_arg),
            obj_watermark,
            span,
        )
    })
}

/// Store `item_arg` at `index_arg` of the list or array `obj_op` of type
/// `obj_ty` as a direct indexed assignment.
fn emit_collection_set(
    ctx: &mut LoweringContext,
    receiver: Receiver<'_>,
    obj_ty: &Type,
    (index_arg, item_arg): (&Expression, &Expression),
    obj_watermark: usize,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let index_op = lower_operand(ctx, index_arg)?;
    let (item_op, item_ty) = lower_stored_value(ctx, item_arg, obj_ty, ELEMENT_SLOT)?;
    let item_op_src = item_op.place().map(|place| place.local);
    let obj_op = receiver.make_own(ctx)?;

    let alias_ty = receiver_alias_type(ctx, &obj_op, obj_ty);
    let obj_local = store_operand_temp(ctx, obj_op, alias_ty, *span);
    let index_local = materialize_index_local(ctx, index_op, index_arg.span);
    let mut indexed_place = Place::new(obj_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::index(index_local));

    let item_local = store_operand_temp(ctx, item_op, item_ty, item_arg.span);

    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            indexed_place,
            Rvalue::Use(Operand::Copy(Place::new(item_local))),
        ),
        span: *span,
    });

    ctx.emit_temp_drop(obj_local, obj_watermark, *span);
    // Reading the value into a temp of its own type is what lets reference
    // counting see the value rather than the struct it may have been projected
    // out of, and the read is what takes the reference the temp then owns.
    // Releasing it here is what balances that: without this the value stays
    // owned at the return, which the RC verifier reports as a missing release.
    // A collection whose elements sit inline takes no reference on the write,
    // and an unmanaged element type is never counted at all; both are already
    // excluded, the first by the write itself and the second by this call.
    ctx.emit_temp_drop(item_local, obj_watermark, *span);
    if let Some(item_src) = item_op_src {
        ctx.emit_temp_drop(item_src, obj_watermark, *span);
    }
    Ok(Some(void_none_operand(*span)))
}

/// A `void`-typed `None` constant operand (a unit return value).
fn void_none_operand(span: Span) -> Operand {
    Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Void, span),
        literal: crate::ast::literal::Literal::None,
    }))
}

/// Lower optimized collection methods directly to MIR instructions or intrinsics.
///
/// This prevents monomorphization conflicts when multiple instantiations (e.g., List<int>, List<bool>)
/// try to define the same method, and enables more precise RC analysis by keeping the concrete
/// element type visible at the call site.
pub(super) fn try_lower_collection_intrinsic(
    ctx: &mut LoweringContext,
    call: CollectionIntrinsicCall,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let CollectionIntrinsicCall {
        span,
        call_expr_id,
        obj,
        obj_ty,
        method_name,
        args,
    } = call;
    let builtin = obj_ty.kind.as_builtin_collection();
    let is_indexable_collection = matches!(
        builtin,
        Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
    ) || obj_ty.kind.is_tuple();
    if args.len() == 1 && matches!(method_name, "element_at" | "get") && is_indexable_collection {
        return lower_collection_element_access(ctx, span, call_expr_id, obj, obj_ty, args, dest)
            .map(Some);
    }

    if args.len() == 1 && method_name == "push" && builtin == Some(BuiltinCollectionKind::List) {
        return lower_list_push(ctx, obj, obj_ty, &args[0], span);
    }

    if let (true, "length", Some(kind)) = (args.is_empty(), method_name, builtin) {
        if let Some(local) = host_binding(ctx, obj, kind) {
            return Ok(Some(lower_collection_length(ctx, local, dest, span)));
        }
    }

    if args.len() == 2 && method_name == "insert" && builtin == Some(BuiltinCollectionKind::List) {
        return lower_list_insert(ctx, obj, obj_ty, &args[0], &args[1], span);
    }

    if args.len() == 2
        && method_name == "set"
        && matches!(
            builtin,
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        )
    {
        return lower_collection_set(ctx, obj, obj_ty, &args[0], &args[1], builtin, span);
    }

    if args.len() == 2 && method_name == "set" && builtin == Some(BuiltinCollectionKind::Map) {
        return lower_map_set(ctx, obj, obj_ty, &args[0], &args[1], span);
    }

    if args.len() == 1 && method_name == "add" && builtin == Some(BuiltinCollectionKind::Set) {
        return lower_set_add(ctx, obj, obj_ty, &args[0], dest, span);
    }

    // Try GPU reduce on array with 2 args (init, fold). Only a gpu-resident
    // receiver routes to the device reduction; a host receiver falls through to
    // the CPU `Foldable::reduce`. The residency is read from the binding's local
    // WITHOUT lowering `obj` here, so the single lowering happens inside
    // `try_lower_gpu_reduce` (lowering it here too would double-emit a non-trivial
    // receiver expression).
    if args.len() == 2 && method_name == "reduce" && builtin == Some(BuiltinCollectionKind::Array) {
        if let ExpressionKind::Identifier(name, _) = &obj.node {
            let is_gpu_resident = ctx.variable_map.get(name.as_str()).is_some_and(|&local| {
                ctx.body.local_decls[local.0].residency == crate::mir::body::BindingResidency::Gpu
            });
            if is_gpu_resident {
                return super::reduce_gpu::try_lower_gpu_reduce(
                    ctx,
                    obj,
                    obj_ty,
                    &args[0],
                    &args[1],
                    call_expr_id,
                    dest,
                    span,
                );
            }
        }
    }

    // `g.slice(range)` on a gpu-resident array is a partial readback: copy the
    // selected range of the eagerly-read-back host buffer into a fresh `List`.
    // A host receiver has no `slice` method and never reaches here.
    if args.len() == 1 && method_name == "slice" && builtin == Some(BuiltinCollectionKind::Array) {
        if let ExpressionKind::Identifier(name, _) = &obj.node {
            let is_gpu_resident = ctx.variable_map.get(name.as_str()).is_some_and(|&local| {
                ctx.body.local_decls[local.0].residency == crate::mir::body::BindingResidency::Gpu
            });
            if is_gpu_resident {
                return lower_gpu_slice(ctx, span, call_expr_id, obj, &args[0], dest).map(Some);
            }
        }
    }

    Ok(None)
}

/// Lower `g.slice(start..end)` on a gpu-resident array to a partial readback:
/// `miri_rt_array_slice(host_buffer, start, end)` yielding a fresh `List<T>`.
/// The gpu binding is borrowed (a `Copy` call argument — callers use borrow
/// semantics, so no IncRef), matching the non-consuming readback-copy rule.
fn lower_gpu_slice(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    range: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Range(start, Some(end), _) = &range.node else {
        return Err(LoweringError::coded(
            DiagnosticCode::TypGpuSliceRangeNotBounded,
            "slice expects a bounded range argument".to_string(),
            *span,
            None,
        ));
    };

    // Fence outstanding device writes and copy the device buffer back to the
    // host array first, so the sub-range read observes the kernel's results.
    // The readback pass sees the binding here only as a runtime call's
    // argument, which it leaves alone, so the slice reads it back itself.
    super::variable::emit_cross_residency_readback(ctx, Some(obj), *span);

    let obj_op = lower_operand(ctx, obj)?;
    let start_op = lower_operand(ctx, start)?;
    let end_op = lower_operand(ctx, end)?;

    let result_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Int, *span));
    let (destination, op) = call_destination(ctx, result_ty, dest, *span);

    let func_op = Operand::runtime(crate::mir::compiler_intrinsics::ARRAY_SLICE.callee(), *span);
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, start_op, end_op],
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            flags: crate::mir::CallFlags::default(),
            destination,
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    Ok(op)
}

/// Lower a constructor call for a struct or class.
fn try_lower_constructor_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    if let Some(func_ty) = ctx.type_checker.get_type(func.id) {
        if let TypeKind::Meta(inner) = &func_ty.kind {
            if let TypeKind::Custom(type_name, _) = &inner.kind {
                // Extract concrete type_args from the overall call expression
                // type, which inside an instantiated body names the
                // instantiation's types rather than its generic parameters.
                let call_ty = ctx.recorded_type(call_expr_id);
                let type_args = call_ty.as_ref().and_then(|ty| {
                    if let TypeKind::Custom(_, ta) = &ty.kind {
                        ta.as_ref().map(|v| v.as_slice())
                    } else {
                        None
                    }
                });

                let defs = &ctx.type_checker.type_table.global_type_definitions;
                if let Some(TypeDefinition::Struct(def)) = defs.get(type_name) {
                    return lower_struct_constructor(
                        ctx,
                        span,
                        type_name,
                        def,
                        args,
                        type_args,
                        dest.cloned(),
                    )
                    .map(Some);
                }
                if let Some(TypeDefinition::Class(def)) = defs.get(type_name) {
                    if let Some(kind) = BuiltinCollectionKind::from_name(type_name) {
                        if let Some((_, ctor_fn)) =
                            COLLECTION_CTORS.iter().find(|(k, _)| *k == kind)
                        {
                            return ctor_fn(ctx, span, call_expr_id, args, dest.cloned()).map(Some);
                        }
                    }
                    return lower_class_constructor(
                        ctx,
                        span,
                        type_name,
                        def,
                        args,
                        call_ty.as_ref(),
                        dest.cloned(),
                    )
                    .map(Some);
                }
            }
        }
    }
    Ok(None)
}

/// Lower the callee expression of a call.
///
/// A bare name is resolved to its symbol rather than to a value: a global
/// function stays a linkable constant so the call is emitted directly, and a
/// local keeps its ordinary read. Reading it as a value would instead wrap it in
/// a closure over a forwarding thunk — correct for `let f = foo`, pointless
/// indirection for `foo(1)`.
fn lower_callee(ctx: &mut LoweringContext, func: &Expression) -> Result<Operand, LoweringError> {
    if matches!(func.node, ExpressionKind::Identifier(_, _)) {
        return crate::mir::lowering::expression::identifier_expr::lower_identifier_symbol(
            ctx, func, None,
        );
    }
    lower_expression(ctx, func, None)
}

/// The expression naming what a call targets.
///
/// A call may write its type arguments out — `f<int>(x)` — and that spelling
/// parses as a generic type reference wrapping the name. The type checker binds
/// the callee's parameters from the arguments it writes and records the
/// instantiation against the call itself, which is where it is read from, so
/// the name is all that is wanted here. Leaving the wrapper in place asks for a
/// type to be lowered as a value, which is refused.
fn callee_name_expression(func: &Expression) -> &Expression {
    let (ExpressionKind::GenericType(base, _, _) | ExpressionKind::TypeDeclaration(base, _, _, _)) =
        &func.node
    else {
        return func;
    };
    base
}

/// Lower a direct function call (global function, lambda, or generic instantiation).
fn lower_direct_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let tc = ctx.type_checker;
    let func_watermark = ctx.body.local_decls.len();
    let callee = callee_name_expression(func);
    let mut func_op = lower_callee(ctx, callee)?;

    apply_generic_mangling(ctx, callee, call_expr_id, &mut func_op)?;

    let arg_watermark = ctx.body.local_decls.len();
    let call = CallSite {
        callee_id: func.id,
        span: *span,
    };
    let mut arg_ops = lower_call_arguments(ctx, call, call_expr_id, args, &func_op)?;

    inject_allocator_arg(ctx, callee, &func_op, &mut arg_ops);
    let slot_call = SlotCall::at(call_expr_id, arg_watermark);
    adapt_runtime_slot_closures(ctx, slot_call, &func_op, &mut arg_ops)?;

    // Per-residency device-handle Call-ABI: when a gpu-resident buffer is passed
    // to a `GpuLaunchSafe` callee, retarget the call to a residency-specialized
    // body (lowered by the pipeline monomorph driver) and record each argument's
    // device handle so that body's kernel launches on the same persistent buffer.
    let arg_handles =
        residency_specialize_call(ctx, call_expr_id, func, args, &mut func_op, &arg_ops)?;

    // Read through the active instantiation substitution: a call inside an
    // instantiated generic body is recorded once against that body's own
    // parameters, and a destination typed at the bare parameter would be sized
    // and reference counted as no instantiation's value is. The static method
    // and aliased calls read it the same way, so every declaration of one
    // monomorphized symbol agrees on its return type.
    let return_ty = ctx
        .recorded_type(call_expr_id)
        .unwrap_or(Type::new(TypeKind::Void, *span));
    let (destination, op) = call_destination(ctx, return_ty, dest, *span);

    let func_op_for_drop = func_op.clone();
    // Which parameters are `out` is declared, not instantiated, so a generic
    // call reads it off the declaration too: the instance's body takes those
    // parameters by address, and a call that passed them by value would hand
    // it a value to write through.
    let mut arg_modes = build_arg_modes(&resolve_param_types(ctx, callee.id), &arg_ops);
    let at = (arg_watermark, destination.local);
    let convention = value_call_convention(ctx, callee, &func_op);
    let named = (callee, callee.id);
    let call = KeptCall::of(tc, named, &func_op, convention.as_deref(), args, at)?;
    let handed = hand_over_kept_arguments(ctx, &call, &mut arg_ops, &mut arg_modes)?;

    let arg_locals = operand_locals(&arg_ops);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops,
        arg_modes,
        arg_handles,
        destination.clone(),
        *span,
    );
    end_direct_call(ctx, (callee, *span), &call, &arg_locals, &handed);
    release_called_value(
        ctx,
        &func_op_for_drop,
        destination.local,
        func_watermark,
        *span,
    );
    Ok(op)
}

/// Releases the temporary an indirect call read its callee from, unless the
/// call wrote its result into that same local; a named function holds nothing.
fn release_called_value(
    ctx: &mut LoweringContext,
    callee: &Operand,
    destination: Local,
    watermark: usize,
    span: Span,
) {
    let (Operand::Copy(place) | Operand::Move(place)) = callee else {
        return;
    };
    if place.local != destination {
        ctx.emit_temp_drop(place.local, watermark, span);
    }
}

/// Emit a `Call` terminator to `destination` and advance to its successor block.
///
/// `arg_modes` are the modes the lowering decides (an `out` parameter's);
/// a runtime entry point's declared modes are laid over them where the
/// terminator is set ([`LoweringContext::set_terminator`]).
///
/// `arg_handles` records, per positional argument, the persistent device-buffer
/// handle when that argument is a gpu-resident binding reaching a residency-
/// specialized callee (empty for an ordinary host call). It is either empty or
/// exactly `args.len()` long — the invariant validated in [`crate::mir::terminator`].
pub(crate) fn emit_call_terminator(
    ctx: &mut LoweringContext,
    func_op: Operand,
    args: Vec<Operand>,
    arg_modes: Vec<ArgMode>,
    arg_handles: Vec<Option<crate::mir::body::DeviceHandleId>>,
    destination: Place,
    span: Span,
) {
    // Enforce the empty-or-equal-length `arg_handles` invariant at the sole
    // construction seam, so a future edit that mis-sizes the vector is caught in
    // debug/test builds instead of silently corrupting the device-handle ABI.
    debug_assert!(
        crate::mir::terminator::validate_call_arg_handles(&args, &arg_handles).is_ok(),
        "Call arg_handles must be empty or equal to args.len()"
    );
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args,
            arg_modes,
            arg_handles,
            flags: crate::mir::CallFlags::default(),
            destination,
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
}

/// Build the per-argument [`ArgMode`] list for a direct call.
fn build_arg_modes(
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
    arg_ops: &[Operand],
) -> Vec<ArgMode> {
    match param_types {
        Some(params) => (0..arg_ops.len())
            .map(|i| ArgMode::from_out_flag(params.get(i).is_some_and(|p| p.is_out)))
            .collect(),
        None => Vec::new(),
    }
}

/// The local each of `operands` reads, in order, for the ones reading a
/// place: what a call's argument temporaries are released from once the
/// operands themselves are the call's.
pub(super) fn operand_locals(operands: &[Operand]) -> Vec<Local> {
    (operands.iter())
        .filter_map(|operand| match operand {
            Operand::Copy(place) | Operand::Move(place) => Some(place.local),
            Operand::Constant(_) | Operand::Function(_) => None,
        })
        .collect()
}

/// Drop each freshly-created argument temp among `arg_locals`, skipping
/// `held`: the call's destination and every temp the call took over by
/// `move`, which it ended already.
pub(super) fn emit_direct_call_drops(
    ctx: &mut LoweringContext,
    arg_locals: &[Local],
    arg_watermark: usize,
    held: &[Local],
    span: Span,
) {
    // A temporary is the call's once however many arguments it fills: a
    // parameter's default may read the value an earlier argument passes, and
    // both slots then name the same temporary.
    let mut released = std::collections::HashSet::new();
    for &local in arg_locals {
        if !held.contains(&local) && released.insert(local) {
            ctx.emit_temp_drop(local, arg_watermark, span);
        }
    }
}

/// End the block a call to `callee` continues in as unreachable when the
/// callee never returns, and go on lowering in a fresh block nothing reaches.
/// No path leaves such a call, so nothing written after it runs, and no
/// analysis may follow a path out of it: a binding the call's arguments moved
/// would read as used after the move along it.
fn seal_after_diverging_call(ctx: &mut LoweringContext, callee: &Expression, span: Span) {
    if ctx.type_checker.callee_never_returns(callee) {
        ctx.set_terminator(Terminator::new(TerminatorKind::Unreachable, span));
        ctx.leave_terminated_block();
    }
}

/// What follows a direct call to `callee` once it returns: the continuation
/// sealed when the callee never returns, then the release every call owning
/// or lending its operands makes ([`release_after_kept_call`]).
fn end_direct_call(
    ctx: &mut LoweringContext,
    (callee, span): (&Expression, Span),
    call: &KeptCall<'_>,
    arg_locals: &[Local],
    handed: &HandedOver,
) {
    seal_after_diverging_call(ctx, callee, span);
    release_after_kept_call(ctx, call, arg_locals, handed, span);
}

fn apply_generic_mangling(
    ctx: &mut LoweringContext,
    callee: &Expression,
    call_expr_id: usize,
    func_op: &mut Operand,
) -> Result<(), LoweringError> {
    let func_span = callee.span;
    if let Some(symbol) = generic_function_symbol(ctx, callee, call_expr_id, func_span)? {
        *func_op = Operand::function(symbol, func_span);
    }
    Ok(())
}

/// The symbol of the generic function instantiation a call through the
/// identifier `callee` targets, recorded on the body so the pipeline lowers
/// that instantiation.
///
/// `None` when the callee is no identifier or no generic function. A call
/// pinning a parameter to a type with no name to compile a body at is refused.
/// A call to a generic function the type checker recorded no instantiation for
/// is an internal error: the shared body it would otherwise link is laid out
/// for no caller's types.
fn generic_function_symbol(
    ctx: &mut LoweringContext,
    callee: &Expression,
    call_expr_id: usize,
    span: Span,
) -> Result<Option<Symbol>, LoweringError> {
    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Ok(None);
    };
    let Some(type_args) = ctx.instantiated_call_mapping(call_expr_id) else {
        return refuse_uninstantiated_generic_call(ctx, callee, func_name, span);
    };
    refuse_unnameable_call_mapping(ctx, func_name, &type_args, span)?;
    let function = ctx.declared_callee(callee, func_name)?.clone();
    let symbol = Symbol::function(
        &function.module,
        &function.name,
        type_args.iter().map(|(_, ty)| ty),
    );
    ctx.requests
        .generic_function_calls
        .push(crate::mir::lowering::requests::GenericFunctionCall {
            symbol: symbol.clone(),
            function,
            type_args,
        });
    Ok(Some(symbol))
}

/// Refuse a call through `callee` to a generic function whose instantiation
/// the type checker did not record; `Ok(None)` for any other callee.
fn refuse_uninstantiated_generic_call(
    ctx: &LoweringContext,
    callee: &Expression,
    func_name: &str,
    span: Span,
) -> Result<Option<Symbol>, LoweringError> {
    if calls_a_generic_function(ctx, callee) {
        return Err(uninstantiated_generic_call(func_name, span));
    }
    Ok(None)
}

/// Whether `callee` names a function this compilation lowers that declares
/// type parameters.
fn calls_a_generic_function(ctx: &LoweringContext, callee: &Expression) -> bool {
    let declares_type_parameters = matches!(
        ctx.type_checker.get_type(callee.id).map(|ty| &ty.kind),
        Some(TypeKind::Function(signature))
            if signature.generics.as_ref().is_some_and(|generics| !generics.is_empty())
    );
    declares_type_parameters && ctx.callee_kind(callee) == CalleeKind::Program
}

/// The internal error for a call to the generic `func_name` that reached
/// lowering with no instantiation: the shared body it would otherwise link is
/// laid out for no caller's types.
fn uninstantiated_generic_call(func_name: &str, span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!(
            "the call to the generic function `{func_name}` reached lowering with no \
             instantiation recorded for it"
        ),
        span,
    )
}

/// Refuse the generic `func_name` pinned by a call to `type_args` when one of
/// them has no name to compile a body at. Only a call written at such a type
/// is refused here; one a body lowered for an instantiation computes from its
/// substitution is recorded, left unlowered, and refused by the pipeline once
/// the bounds on instantiations have had the chance to report the growth that
/// reached it.
pub(super) fn refuse_unnameable_call_mapping(
    ctx: &LoweringContext,
    func_name: &str,
    type_args: &[(String, Type)],
    span: Span,
) -> Result<(), LoweringError> {
    if !ctx.generic_subs.is_empty() {
        return Ok(());
    }
    let args: Vec<Type> = type_args.iter().map(|(_, ty)| ty.clone()).collect();
    ctx.refuse_unnameable_type_arguments(func_name, &args, span)
}

fn resolve_param_types(
    ctx: &LoweringContext,
    func_id: usize,
) -> Option<Vec<crate::ast::common::Parameter>> {
    let func_ty = ctx.type_checker.get_type(func_id)?;
    if let TypeKind::Function(func_data) = &func_ty.kind {
        Some(func_data.params.clone())
    } else {
        None
    }
}

/// The type of the value `op` carries for the argument `arg`.
///
/// A local holds exactly its declared type, but a projected place — an element
/// or a field read in place — does not: [`Operand::ty`] reports the base local's
/// type, a collection or an aggregate, which would make every such argument look
/// like it needs converting. The type checker's type for the argument is the
/// element's own.
pub(crate) fn argument_type(ctx: &LoweringContext, op: &Operand, arg: &Expression) -> Type {
    match op {
        Operand::Copy(place) | Operand::Move(place) if !place.projection.is_empty() => {
            ctx.resolved_type(arg)
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => {
            op.ty(&ctx.body).clone()
        }
    }
}

/// The type each parameter of a non-generic callee is passed at.
fn declared_parameter_types(
    ctx: &LoweringContext,
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
) -> Vec<Type> {
    param_types
        .iter()
        .flatten()
        .map(|param| ctx.declared_type(&param.typ))
        .collect()
}

/// The type each parameter of the generic function a call at `call_expr_id`
/// reaches is passed at, read at the instantiation the call binds.
///
/// A parameter written as a bare `T` is passed at whatever `T` is bound to:
/// a value handed where `T` is `int?` is passed as an optional, which the
/// callee reads it as.
fn generic_call_parameter_types(
    ctx: &LoweringContext,
    func_id: usize,
    call_expr_id: usize,
) -> Vec<Type> {
    let Some(mapping) = ctx.instantiated_call_mapping(call_expr_id) else {
        return Vec::new();
    };
    let callee_subs: HashMap<String, Type> = mapping.into_iter().collect();
    let Some(TypeKind::Function(signature)) = ctx.type_checker.get_type(func_id).map(|ty| &ty.kind)
    else {
        return Vec::new();
    };
    signature
        .params
        .iter()
        .map(|param| ctx.callee_parameter_type(&param.typ, &callee_subs))
        .collect()
}

/// The name of each parameter of the function `func_id` is typed as, in
/// declaration order; empty when the callee's type is not a function.
fn callee_parameter_names(ctx: &LoweringContext, func_id: usize) -> Vec<String> {
    let Some(TypeKind::Function(signature)) = ctx.type_checker.get_type(func_id).map(|ty| &ty.kind)
    else {
        return Vec::new();
    };
    signature
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect()
}

/// Put an operand lowered for the parameter at `position` into its slot,
/// growing the slots to reach it.
pub(super) fn place_at_parameter(slots: &mut Vec<Option<Operand>>, position: usize, op: Operand) {
    if slots.len() <= position {
        slots.resize(position + 1, None);
    }
    slots[position] = Some(op);
}

/// The call's operands in parameter order, once every slot up to the last
/// supplied one holds a value.
///
/// A hole means a parameter reached lowering with neither an argument nor a
/// default, which the type checker refuses.
pub(super) fn operands_in_parameter_order(
    slots: Vec<Option<Operand>>,
    span: Span,
) -> Result<Vec<Operand>, LoweringError> {
    slots
        .into_iter()
        .enumerate()
        .map(|(position, slot)| {
            slot.ok_or_else(|| {
                LoweringError::internal(
                    DiagnosticCode::MirCustomLoweringError,
                    format!(
                        "a call reached lowering with no argument or default for parameter {position}"
                    ),
                    span,
                )
            })
        })
        .collect()
}

/// Lower a call's written arguments in the order they are written, so their
/// side effects happen as the source says, and place each at the parameter it
/// binds, brought to the type that parameter is passed at.
///
/// An argument the callee writes in place is lowered up to its check and
/// returned beside the slots, its slot holding a placeholder, for the caller
/// to make the call's own once every other argument is lowered.
fn lower_and_coerce_args<'e>(
    ctx: &mut LoweringContext,
    args: &'e [Expression],
    targets: &[Type],
    param_names: &[String],
    callee: Option<RuntimeCallee<'_>>,
) -> Result<(Vec<Option<Operand>>, Vec<WrittenArgument<'e>>), LoweringError> {
    let names: Vec<&str> = param_names.iter().map(String::as_str).collect();
    let mut slots = Vec::with_capacity(args.len());
    let mut written = Vec::new();
    for (position, value) in crate::ast::expression::bind_arguments_to_parameters(args, &names) {
        let watermark = ctx.body.local_decls.len();
        let op = match runtime_writes::lower_call_argument(ctx, callee, position, value)? {
            runtime_writes::CallArgument::Lowered(op) => super::sequencing::hold(ctx, op, value),
            runtime_writes::CallArgument::Written(pending) => {
                written.push(WrittenArgument {
                    position,
                    value,
                    pending,
                });
                place_at_parameter(&mut slots, position, void_none_operand(value.span));
                continue;
            }
        };
        let op = match targets.get(position) {
            Some(target_ty) => coerce_arg_to_declared(ctx, op, value, target_ty, watermark),
            None => op,
        };
        place_at_parameter(&mut slots, position, op);
    }
    Ok((slots, written))
}

/// An argument a runtime entry point writes in place, lowered up to its
/// check: the parameter it binds, the expression written for it, and the
/// receiver still to be made the call's own.
struct WrittenArgument<'e> {
    position: usize,
    value: &'e Expression,
    pending: PendingReceiver<'e>,
}

/// A written argument, made the call's own, brought to the type its
/// parameter is passed at.
fn coerce_written_arg(
    ctx: &mut LoweringContext,
    op: Operand,
    value: &Expression,
    target: Option<&Type>,
) -> Operand {
    let watermark = ctx.body.local_decls.len();
    match target {
        Some(target_ty) => coerce_arg_to_declared(ctx, op, value, target_ty, watermark),
        None => op,
    }
}

/// Bring a lowered argument to the type the callee declares for it.
///
/// An argument written at a shallower optional depth than the parameter, or at
/// any other type the two are not spellings of, is boxed into a temp of the
/// declared type; anything already at that type passes through untouched, since
/// boxing it again would put the value a layer too deep.
///
/// `watermark` is the local count from before the argument was lowered, so a
/// source the coercion stops owning is released against its own scope.
pub(super) fn coerce_arg_to_declared(
    ctx: &mut LoweringContext,
    op: Operand,
    arg: &Expression,
    target_ty: &Type,
    watermark: usize,
) -> Operand {
    // A binding declared with the written `Option<T>` keeps that spelling;
    // compared as written it would read as a value the optional wraps.
    let op_ty =
        super::variable::canonical_declared_type(ctx.type_checker, &argument_type(ctx, &op, arg));
    if op_ty.kind == target_ty.kind || spellings_of_one_value(&op_ty, target_ty) {
        return op;
    }
    let temp = ctx.push_temp(target_ty.clone(), arg.span);
    let rvalue = coerce_rvalue_in(ctx, op.clone(), &op_ty, target_ty, arg.span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(Place::new(temp), rvalue),
        span: arg.span,
    });
    release_coerced_source(ctx, &op, &op_ty, target_ty, watermark, arg.span);
    Operand::Copy(Place::new(temp))
}

/// Supply the default of every parameter no written argument bound. A named
/// argument may skip over a defaulted parameter, so the gap can sit anywhere,
/// not only after the last written argument.
///
/// A default reads the callee's names, not the caller's: its earlier
/// parameters ([`earlier_parameter_scope`]) and, for a generic callee, its
/// own type parameters at the arguments this call binds them to.
fn fill_default_args(
    ctx: &mut LoweringContext,
    slots: &mut Vec<Option<Operand>>,
    callee: CalleeParameters<'_>,
    call: CallSite,
) -> Result<(), LoweringError> {
    for (position, param) in callee.params.iter().enumerate() {
        if slots.get(position).is_some_and(Option::is_some) {
            continue;
        }
        if let Some(default_expr) = &param.default_value {
            let scope =
                earlier_parameter_scope(ctx, &callee, position, slots, default_expr, call.span)?;
            let caller_scope = std::mem::replace(&mut ctx.variable_map, scope);
            let caller_subs = std::mem::replace(&mut ctx.generic_subs, callee.subs.clone());
            let lowered = lower_default_argument(ctx, default_expr, call);
            ctx.variable_map = caller_scope;
            ctx.generic_subs = caller_subs;
            place_at_parameter(slots, position, lowered?);
        }
    }
    Ok(())
}

/// The names a parameter's default is lowered against: each parameter
/// declared before it that the default names, bound to the value this call
/// passes for it.
///
/// The default is written in the callee, so it reads the callee's names and
/// none of the caller's: a caller binding that happens to share a name with a
/// parameter is not in sight. The body's allocator stays bound, since the
/// code the default runs is part of the calling body. An argument already held in a binding of its
/// own is read from there; any other one (a constant, a field) is put in a
/// temporary first, which the call then passes in its place, so the call and
/// the default read one value.
fn earlier_parameter_scope(
    ctx: &mut LoweringContext,
    callee: &CalleeParameters<'_>,
    position: usize,
    slots: &mut [Option<Operand>],
    default_expr: &Expression,
    span: Span,
) -> Result<HashMap<Rc<str>, Local>, LoweringError> {
    let read = crate::type_checker::hygiene::names::References::of_expression(default_expr);
    let mut scope = HashMap::new();
    // The allocator is not a name the default can write: it is the body's own,
    // which every call the default makes passes on.
    if let Some(&allocator) = ctx.variable_map.get("allocator") {
        scope.insert(Rc::from("allocator"), allocator);
    }
    let earlier = callee.params.get(..position).unwrap_or_default();
    for (position, (param, slot)) in earlier.iter().zip(slots.iter_mut()).enumerate() {
        let Some(op) = slot.as_mut().filter(|_| read.contains(&param.name)) else {
            continue;
        };
        let local = match op {
            Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
                place.local
            }
            _ => {
                let ty = callee.types.get(position).cloned().ok_or_else(|| {
                    LoweringError::internal(
                        DiagnosticCode::MirCustomLoweringError,
                        format!(
                            "a default reads parameter '{}', whose declared type the call does not know",
                            param.name
                        ),
                        span,
                    )
                })?;
                let temp = store_operand_temp(ctx, op.clone(), ty, span);
                *op = Operand::Copy(Place::new(temp));
                temp
            }
        };
        scope.insert(Rc::from(param.name.as_str()), local);
    }
    Ok(scope)
}

/// The parameters of the function a call reaches: as declared, the type the
/// call passes each at, and the arguments it binds the function's type
/// parameters to (none for a function that has none).
struct CalleeParameters<'a> {
    params: &'a [crate::ast::common::Parameter],
    types: &'a [Type],
    subs: HashMap<String, Type>,
}

/// A call that may give an omitted parameter its default: the expression
/// naming the callee and where the call is written.
#[derive(Clone, Copy)]
struct CallSite {
    callee_id: usize,
    span: Span,
}

/// Lower the default `default_expr` of a parameter of the callee `call`
/// names, at that call.
///
/// The expression is written in the callee's file, so it is lowered
/// reading spans against that file: a location it bakes in, or a closure it
/// holds, names the callee's file. What it adds to the calling body runs at
/// the call, so when the callee's file is another one those statements are
/// placed at the call, keeping every span of the body an offset into the
/// body's own file.
fn lower_default_argument(
    ctx: &mut LoweringContext,
    default_expr: &Expression,
    call: CallSite,
) -> Result<Operand, LoweringError> {
    let callee_file = ctx
        .type_checker
        .declared_callee(call.callee_id)
        .and_then(|callee| ctx.module_file(&callee.module));
    let Some(callee_file) = callee_file.filter(|file| file.id != ctx.body.source) else {
        return lower_expression(ctx, default_expr, None);
    };
    let start = EmissionPoint::of(ctx);
    let caller_file = ctx.read_source_file(callee_file);
    let lowered = lower_expression(ctx, default_expr, None);
    ctx.read_source_file(caller_file);
    start.place_emitted_code_at(ctx, call.span);
    lowered
}

/// Where lowering appends the next statement of a body: the current block,
/// its statement count and the number of blocks.
struct EmissionPoint {
    block: usize,
    statement: usize,
    blocks: usize,
}

impl EmissionPoint {
    fn of(ctx: &LoweringContext) -> Self {
        let block = ctx.current_block.0;
        Self {
            block,
            statement: ctx
                .body
                .basic_blocks
                .get(block)
                .map_or(0, |data| data.statements.len()),
            blocks: ctx.body.basic_blocks.len(),
        }
    }

    /// Give every statement and terminator lowered since this point `span`.
    fn place_emitted_code_at(&self, ctx: &mut LoweringContext, span: Span) {
        let blocks = ctx.body.basic_blocks.iter_mut().enumerate();
        for (index, data) in
            blocks.filter(|(index, _)| *index == self.block || *index >= self.blocks)
        {
            let first = if index == self.block {
                self.statement
            } else {
                0
            };
            for statement in data.statements.iter_mut().skip(first) {
                statement.span = span;
            }
            if let Some(terminator) = data.terminator.as_mut() {
                terminator.span = span;
            }
        }
    }
}

/// Whether a call to the function `callee` names carries the implicit
/// trailing allocator parameter every Miri-defined function is lowered with.
///
/// Runtime C functions and math intrinsics are declared without it: the first is
/// a foreign symbol with a fixed signature, the second never reaches a call at
/// all because it lowers to a `MathIntrinsic` rvalue. Which functions are
/// runtime ones is what the declaration the name resolved to says, never how it
/// is spelled: a Miri function may be named anything a runtime export is.
pub(super) fn callee_takes_allocator(ctx: &LoweringContext, callee: &Expression) -> bool {
    ctx.callee_kind(callee) != CalleeKind::Runtime && math_intrinsic_callee(ctx, callee).is_none()
}

fn inject_allocator_arg(
    ctx: &mut LoweringContext,
    callee: &Expression,
    func_op: &Operand,
    arg_ops: &mut Vec<Operand>,
) {
    let is_indirect_call = !matches!(func_op, Operand::Function(_));
    if is_indirect_call || !callee_takes_allocator(ctx, callee) {
        return;
    }

    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        let already_has_alloc = arg_ops.iter().any(|op| {
            if let Operand::Copy(p) | Operand::Move(p) = op {
                p.local == alloc_local
            } else {
                false
            }
        });
        if !already_has_alloc {
            arg_ops.push(Operand::Copy(Place::new(alloc_local)));
        }
    }
}
