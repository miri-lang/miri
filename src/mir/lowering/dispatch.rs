// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Call lowering dispatcher, collection intrinsics, and constructor/direct-call fallbacks.

use crate::ast::expression::Expression;
use crate::ast::{BuiltinCollectionKind, ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{
    Local, MathIntrinsic, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::{MethodInfo, TypeDefinition};

use super::constructors::{lower_class_constructor, lower_struct_constructor, COLLECTION_CTORS};
use super::helpers::{coerce_rvalue, gpu_math_return_type, spellings_of_one_value};
use super::{apply_generic_sub, lower_expression, LoweringContext};
use std::collections::HashMap;

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
pub(crate) use super::method_dispatch::{
    extend_subs_with_trait_params, mangle_generic_name, resolve_inherited_method,
};

// Re-export kernel launch functions from the specialized module.
pub(crate) use super::kernel_launch::try_lower_kernel_launch;

// Import private helpers from specialized modules.
use super::method_dispatch::{emit_cow_check, residency_specialize_call};

pub fn lower_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    if let ExpressionKind::Member(obj, method) = &func.node {
        if let Some(op) =
            try_lower_module_alias_call(ctx, span, call_expr_id, obj, method, args, dest.as_ref())?
        {
            return Ok(op);
        }
    }

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

/// Lower a call to a function in another module via its alias: `M.foo(args)`.
fn try_lower_module_alias_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj_expr: &Expression,
    method_expr: &Expression,
    args: &[Expression],
    dest: Option<&Place>,
) -> Result<Option<Operand>, LoweringError> {
    let ExpressionKind::Identifier(alias_name, _) = &obj_expr.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(func_name, _) = &method_expr.node else {
        return Ok(None);
    };
    let Some(module_path) = ctx
        .type_checker
        .modules
        .module_aliases
        .get(alias_name.as_str())
        .cloned()
    else {
        return Ok(None);
    };

    if module_path == "system.math" {
        if let Some(intrinsic) = MathIntrinsic::from_name(func_name.as_str()) {
            return lower_math_intrinsic_call(
                ctx,
                span,
                call_expr_id,
                intrinsic,
                args,
                dest.cloned(),
            )
            .map(Some);
        }
    }
    lower_aliased_function_call(ctx, span, call_expr_id, func_name, args, dest.cloned())
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
    let ExpressionKind::Identifier(type_name, _) = &obj_expr.node else {
        return Ok(None);
    };
    let ExpressionKind::Identifier(method_name, _) = &method_expr.node else {
        return Ok(None);
    };

    // Try to find the static method in a class inheritance chain first
    if let Some((defining_class_name, method_info)) = ctx
        .type_checker
        .find_static_method_in_chain(type_name, method_name.as_str())
    {
        if method_info.is_static {
            return lower_static_method_impl(
                ctx,
                span,
                call_expr_id,
                &defining_class_name,
                method_name,
                args,
                dest,
                &method_info,
            );
        }
    }

    // Try to find the static method on an enum
    if let Some(enum_def) = ctx
        .type_checker
        .type_table
        .global_type_definitions
        .get(type_name)
        .and_then(|def| {
            if let crate::type_checker::context::TypeDefinition::Enum(enum_def) = def {
                Some(enum_def.clone())
            } else {
                None
            }
        })
    {
        if let Some(method_info) = enum_def.methods.get(method_name) {
            if method_info.is_static {
                return lower_static_method_impl(
                    ctx,
                    span,
                    call_expr_id,
                    type_name,
                    method_name,
                    args,
                    dest,
                    method_info,
                );
            }
        }
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn lower_static_method_impl(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    defining_type_name: &str,
    method_name: &str,
    args: &[Expression],
    dest: Option<&Place>,
    method_info: &crate::type_checker::context::MethodInfo,
) -> Result<Option<Operand>, LoweringError> {
    // Static method call with no receiver
    let arg_watermark = ctx.body.local_decls.len();
    let mut arg_ops = lower_plain_args(ctx, args)?;
    push_allocator_arg(ctx, &mut arg_ops);

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, result_op) = call_destination(ctx, return_ty, dest.cloned(), *span);

    // Construct the mangled function name: DefiningType_method_name
    let mut mangled = String::with_capacity(defining_type_name.len() + 1 + method_name.len());
    mangled.push_str(defining_type_name);
    mangled.push('_');
    mangled.push_str(method_name);

    let func_op = Operand::Constant(Box::new(crate::mir::Constant {
        span: *span,
        ty: Type::new(TypeKind::Identifier, *span),
        literal: crate::ast::literal::Literal::Identifier(mangled),
    }));

    // Build out_args from method_info with no receiver offset (static methods have no self).
    let out_args = build_method_out_args_with_offset(method_info, args.len(), arg_ops.len(), 0);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops.clone(),
        out_args,
        Vec::new(), // arg_handles
        destination.clone(),
        *span,
    );
    emit_direct_call_drops(ctx, &arg_ops, arg_watermark, destination.local, *span);
    Ok(Some(result_op))
}

/// Lower a `system.math` intrinsic call to a `MathIntrinsic` rvalue.
fn lower_math_intrinsic_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    intrinsic: MathIntrinsic,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let arg_ops = lower_plain_args(ctx, args)?;
    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let return_ty = gpu_math_return_type(ctx, args, return_ty, *span);
    let (target, ret_op) = call_destination(ctx, return_ty, dest, *span);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(target, Rvalue::MathIntrinsic(intrinsic, arg_ops)),
        span: *span,
    });
    Ok(ret_op)
}

/// Lower a direct call to a function reached through a module alias.
fn lower_aliased_function_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    func_name: &str,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let mangled = match ctx.type_checker.call_generic_mappings.get(&call_expr_id) {
        Some(generic_args) => mangle_generic_name(func_name, generic_args),
        None => func_name.to_string(),
    };
    let func_op = runtime_fn_operand(&mangled, *span);

    let mut arg_ops = lower_plain_args(ctx, args)?;
    push_allocator_arg(ctx, &mut arg_ops);

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Void, *span));
    let (destination, result_op) = call_destination(ctx, return_ty, dest, *span);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops,
        Vec::new(),
        Vec::new(),
        destination,
        *span,
    );
    Ok(Some(result_op))
}

/// Lower call arguments with plain expression lowering (no coercion).
fn lower_plain_args(
    ctx: &mut LoweringContext,
    args: &[Expression],
) -> Result<Vec<Operand>, LoweringError> {
    let mut arg_ops = Vec::with_capacity(args.len());
    for arg in args {
        arg_ops.push(lower_expression(ctx, arg, None)?);
    }
    Ok(arg_ops)
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

/// Build the `out_args` flag list for a method call's argument vector.
pub(super) fn build_method_out_args(
    method_info: &MethodInfo,
    user_arg_count: usize,
    total_call_args: usize,
) -> Vec<bool> {
    build_method_out_args_with_offset(method_info, user_arg_count, total_call_args, 1)
}

/// Build out-parameter flags for a method call, accounting for receiver offset.
/// For instance methods, `receiver_offset` is 1 (self is at slot 0).
/// For static methods, `receiver_offset` is 0 (no self).
pub(super) fn build_method_out_args_with_offset(
    method_info: &MethodInfo,
    user_arg_count: usize,
    total_call_args: usize,
    receiver_offset: usize,
) -> Vec<bool> {
    let mut flags = vec![false; total_call_args];
    for i in 0..user_arg_count {
        if method_info.is_param_out(i) {
            flags[receiver_offset + i] = true;
        }
    }
    flags
}

/// Release the argument temporaries a method call created.
///
/// A managed argument built at the call site — a concatenation, another call's
/// result — lives in a temp that belongs to no scope, and a parameter is a
/// borrow the callee never releases, so the call site is the only place that
/// can drop it. Locals below the watermark existed before the arguments were
/// lowered and belong to their own scope; the destination is the call's result
/// and outlives it.
pub(super) fn emit_method_arg_drops(
    ctx: &mut LoweringContext,
    args: &[Operand],
    watermark: usize,
    dest_local: Local,
    span: Span,
) {
    for op in args {
        if let Operand::Copy(p) | Operand::Move(p) = op {
            if p.local != dest_local {
                ctx.emit_temp_drop(p.local, watermark, span);
            }
        }
    }
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
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op_src = operand_src_local(&obj_op);
    let index_op = lower_expression(ctx, &args[0], None)?;

    let obj_local = store_operand_temp(ctx, move_to_copy(obj_op), obj_ty.clone(), *span);
    let index_local = materialize_index_local(ctx, index_op, args[0].span);

    let mut indexed_place = Place::new(obj_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(index_local));

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
    let sub_expr = |expr: &Expression| -> Expression {
        match &expr.node {
            ExpressionKind::Type(ty, is_ref) => Expression {
                id: expr.id,
                span: expr.span,
                node: ExpressionKind::Type(Box::new(apply_generic_sub(ty, subs)), *is_ref),
            },
            _ => expr.clone(),
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

/// Build a runtime-function callee constant for `name`.
pub(super) fn runtime_fn_operand(name: &str, span: Span) -> Operand {
    Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(name.to_string()),
    }))
}

/// Resolve a kernel operand and name from a gpu fn callee expression.
pub(super) fn resolve_kernel_operand(
    ctx: &LoweringContext,
    callee: &Expression,
    span: Span,
) -> Result<(Operand, String), LoweringError> {
    let ExpressionKind::Identifier(func_name, _) = &callee.node else {
        return Err(LoweringError::unsupported_expression(
            "gpu fn must be called by name".to_string(),
            span,
        ));
    };

    let kernel_name = match ctx.type_checker.call_generic_mappings.get(&callee.id) {
        Some(generic_args) => mangle_generic_name(func_name, generic_args),
        None => func_name.clone(),
    };

    let kernel_op = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(kernel_name.clone()),
    }));

    Ok((kernel_op, kernel_name))
}

/// Lower list.push(item) to miri_rt_list_push.
/// The source local backing a place operand, if any.
fn operand_src_local(op: &Operand) -> Option<Local> {
    match op {
        Operand::Copy(p) | Operand::Move(p) => Some(p.local),
        _ => None,
    }
}

/// Convert a `Move` operand into a `Copy` of the same place.
fn move_to_copy(op: Operand) -> Operand {
    match op {
        Operand::Move(p) => Operand::Copy(p),
        other => other,
    }
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

fn lower_list_push(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    item_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let item_watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = emit_cow_check(ctx, obj_op, obj_ty, rt::LIST_COW, *span);
    let item_op = lower_expression(ctx, item_arg, None)?;

    let item_op_src = operand_src_local(&item_op);
    let item_copy = move_to_copy(item_op);
    let item_ty = item_copy.ty(&ctx.body).clone();
    let item_local = store_operand_temp(ctx, item_copy, item_ty, item_arg.span);
    let func_op = runtime_fn_operand(rt::LIST_PUSH, *span);
    let target_bb = ctx.new_basic_block();
    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, Operand::Copy(Place::new(item_local))],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(dummy_dest),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(src) = item_op_src {
        ctx.emit_temp_drop(src, item_watermark, item_arg.span);
    }
    Ok(Some(Operand::Copy(Place::new(dummy_dest))))
}

/// Donate a reference to a value a container is about to take ownership of.
///
/// Reading the operand by copy makes Perceus retain it into the temp, and the
/// temp is what the container stores. A temporary that only existed to produce
/// the value is then released, so the net effect is one reference handed over:
/// the caller keeps releasing whatever it already owned, and the container
/// releases the donated one through its drop callback.
fn donate_operand_to_container(
    ctx: &mut LoweringContext,
    op: Operand,
    span: Span,
) -> (Operand, Option<Local>) {
    let src = operand_src_local(&op);
    let copied = move_to_copy(op);
    let ty = copied.ty(&ctx.body).clone();
    let local = store_operand_temp(ctx, copied, ty, span);
    (Operand::Copy(Place::new(local)), src)
}

/// Lower map.set(key, value) to miri_rt_map_set.
///
/// The stdlib method would forward its parameters, which are borrowed, into an
/// intrinsic that takes ownership of them, leaving the map holding references it
/// does not own. Lowering the call here donates both instead, matching
/// `lower_list_push`.
fn lower_map_set(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    key_arg: &Expression,
    value_arg: &Expression,
    span: &Span,
) -> Result<Option<Operand>, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = emit_cow_check(ctx, obj_op, obj_ty, rt::MAP_COW, *span);

    let key_op = lower_expression(ctx, key_arg, None)?;
    let (key_op, key_src) = donate_operand_to_container(ctx, key_op, key_arg.span);
    let value_op = lower_expression(ctx, value_arg, None)?;
    let (value_op, value_src) = donate_operand_to_container(ctx, value_op, value_arg.span);

    let func_op = runtime_fn_operand(rt::MAP_SET, *span);
    let target_bb = ctx.new_basic_block();
    let dummy_dest = ctx.push_temp(Type::new(TypeKind::Void, *span), *span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, key_op, value_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(dummy_dest),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(src) = key_src {
        ctx.emit_temp_drop(src, watermark, key_arg.span);
    }
    if let Some(src) = value_src {
        ctx.emit_temp_drop(src, watermark, value_arg.span);
    }
    Ok(Some(Operand::Copy(Place::new(dummy_dest))))
}

/// Lower set.add(element) to miri_rt_set_add, donating the stored element.
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
    let watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = emit_cow_check(ctx, obj_op, obj_ty, rt::SET_COW, *span);

    let elem_op = lower_expression(ctx, elem_arg, None)?;
    let (elem_op, elem_src) = donate_operand_to_container(ctx, elem_op, elem_arg.span);

    // The intrinsic reports whether the element was newly inserted, so the
    // result has to land in the caller's destination when it asked for one.
    let destination = dest
        .unwrap_or_else(|| Place::new(ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span)));
    let func_op = runtime_fn_operand(rt::SET_ADD, *span);
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, elem_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: destination.clone(),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(src) = elem_src {
        ctx.emit_temp_drop(src, watermark, elem_arg.span);
    }
    Ok(Some(Operand::Copy(destination)))
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
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = emit_cow_check(ctx, obj_op, obj_ty, rt::LIST_COW, *span);
    let index_op = lower_expression(ctx, index_arg, None)?;
    let item_op = lower_expression(ctx, item_arg, None)?;

    let item_op_src = operand_src_local(&item_op);
    let item_copy = move_to_copy(item_op);
    let item_ty = item_copy.ty(&ctx.body).clone();
    let item_local = store_operand_temp(ctx, item_copy, item_ty, item_arg.span);
    let func_op = runtime_fn_operand(rt::LIST_INSERT, *span);
    let target_bb = ctx.new_basic_block();
    let result_temp = ctx.push_temp(Type::new(TypeKind::Boolean, *span), *span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, index_op, Operand::Copy(Place::new(item_local))],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(result_temp),
            target: Some(target_bb),
        },
        *span,
    ));
    ctx.set_current_block(target_bb);
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
    let obj_op = lower_expression(ctx, obj, None)?;
    let obj_op = if builtin == Some(BuiltinCollectionKind::List) {
        emit_cow_check(ctx, obj_op, obj_ty, rt::LIST_COW, *span)
    } else {
        obj_op
    };
    let obj_op_src = operand_src_local(&obj_op);
    let index_op = lower_expression(ctx, index_arg, None)?;
    let item_op = lower_expression(ctx, item_arg, None)?;
    let item_op_src = operand_src_local(&item_op);

    let obj_local = store_operand_temp(ctx, move_to_copy(obj_op), obj_ty.clone(), *span);
    let index_local = materialize_index_local(ctx, index_op, index_arg.span);
    let mut indexed_place = Place::new(obj_local);
    indexed_place
        .projection
        .push(crate::mir::PlaceElem::Index(index_local));
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(indexed_place, Rvalue::Use(move_to_copy(item_op))),
        span: *span,
    });

    ctx.emit_temp_drop(obj_local, obj_watermark, *span);
    if let Some(src_local) = obj_op_src {
        ctx.emit_temp_drop(src_local, obj_watermark, *span);
    }
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
        return Err(LoweringError::custom(
            "slice expects a bounded range argument".to_string(),
            *span,
            None,
        ));
    };

    // Fence outstanding device writes and copy the device buffer back to the
    // host array first, so the sub-range read observes the kernel's results.
    // This is the same readback `let h = g` emits; slice is a partial variant.
    super::variable::emit_cross_residency_readback(ctx, Some(obj), *span);

    let obj_op = move_to_copy(lower_expression(ctx, obj, None)?);
    let start_op = lower_expression(ctx, start, None)?;
    let end_op = lower_expression(ctx, end, None)?;

    let result_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or_else(|| Type::new(TypeKind::Int, *span));
    let (destination, op) = call_destination(ctx, result_ty, dest, *span);

    let func_op = runtime_fn_operand(rt::ARRAY_SLICE, *span);
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: vec![obj_op, start_op, end_op],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
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
                // Extract concrete type_args from the overall call expression type
                let call_ty = ctx.type_checker.get_type(call_expr_id);
                let type_args = call_ty.and_then(|ty| {
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
                        call_ty,
                        dest.cloned(),
                    )
                    .map(Some);
                }
            }
        }
    }
    Ok(None)
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
    let func_watermark = ctx.body.local_decls.len();
    let mut func_op = lower_expression(ctx, func, None)?;

    apply_generic_mangling(ctx, &func.node, call_expr_id, &mut func_op, func.span);

    let is_generic_call = ctx
        .type_checker
        .call_generic_mappings
        .contains_key(&call_expr_id);
    let param_types = resolve_param_types(ctx, func.id, is_generic_call);

    let arg_watermark = ctx.body.local_decls.len();
    let mut arg_ops = lower_and_coerce_args(ctx, args, &param_types);

    fill_default_args(ctx, &mut arg_ops, &param_types)?;

    inject_allocator_arg(ctx, &func.node, &func_op, &mut arg_ops);

    // Per-residency device-handle Call-ABI: when a gpu-resident buffer is passed
    // to a `GpuLaunchSafe` callee, retarget the call to a residency-specialized
    // body (lowered by the pipeline monomorph driver) and record each argument's
    // device handle so that body's kernel launches on the same persistent buffer.
    let arg_handles = residency_specialize_call(ctx, func, args, &mut func_op, &arg_ops);

    let return_ty = ctx
        .type_checker
        .get_type(call_expr_id)
        .cloned()
        .unwrap_or(Type::new(TypeKind::Void, *span));
    let (destination, op) = call_destination(ctx, return_ty, dest, *span);

    let is_indirect_call = !matches!(
        func_op,
        Operand::Constant(ref c) if matches!(c.literal, crate::ast::literal::Literal::Identifier(_))
    );
    let func_op_for_drop = func_op.clone();
    let out_args = build_out_args(&param_types, &arg_ops);

    emit_call_terminator(
        ctx,
        func_op,
        arg_ops.clone(),
        out_args,
        arg_handles,
        destination.clone(),
        *span,
    );
    emit_direct_call_drops(ctx, &arg_ops, arg_watermark, destination.local, *span);
    if is_indirect_call {
        if let Operand::Copy(place) | Operand::Move(place) = &func_op_for_drop {
            if place.local != destination.local {
                ctx.emit_temp_drop(place.local, func_watermark, *span);
            }
        }
    }
    Ok(op)
}

/// Emit a `Call` terminator to `destination` and advance to its successor block.
///
/// `arg_handles` records, per positional argument, the persistent device-buffer
/// handle when that argument is a gpu-resident binding reaching a residency-
/// specialized callee (empty for an ordinary host call). It is either empty or
/// exactly `args.len()` long — the invariant validated in [`crate::mir::terminator`].
fn emit_call_terminator(
    ctx: &mut LoweringContext,
    func_op: Operand,
    args: Vec<Operand>,
    out_args: Vec<bool>,
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
            out_args,
            arg_handles,
            destination,
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
}

/// Build the per-arg `out` flag list for a direct call.
fn build_out_args(
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
    arg_ops: &[Operand],
) -> Vec<bool> {
    match param_types {
        Some(params) => arg_ops
            .iter()
            .enumerate()
            .map(|(i, _)| params.get(i).is_some_and(|p| p.is_out))
            .collect(),
        None => Vec::new(),
    }
}

/// Drop each freshly-created argument temp (skipping the call destination).
fn emit_direct_call_drops(
    ctx: &mut LoweringContext,
    arg_ops: &[Operand],
    arg_watermark: usize,
    dest_local: Local,
    span: Span,
) {
    for arg_op in arg_ops {
        if let Operand::Copy(place) | Operand::Move(place) = arg_op {
            if place.local != dest_local {
                ctx.emit_temp_drop(place.local, arg_watermark, span);
            }
        }
    }
}

fn apply_generic_mangling(
    ctx: &mut LoweringContext,
    func_node: &ExpressionKind,
    call_expr_id: usize,
    func_op: &mut Operand,
    func_span: Span,
) {
    if let ExpressionKind::Identifier(func_name, _) = func_node {
        if let Some(generic_args) = ctx.type_checker.call_generic_mappings.get(&call_expr_id) {
            let mangled = mangle_generic_name(func_name, generic_args);
            *func_op = Operand::Constant(Box::new(crate::mir::Constant {
                span: func_span,
                ty: crate::ast::types::Type::new(TypeKind::Identifier, func_span),
                literal: crate::ast::literal::Literal::Identifier(mangled),
            }));
        }
    }
}

fn resolve_param_types(
    ctx: &LoweringContext,
    func_id: usize,
    is_generic_call: bool,
) -> Option<Vec<crate::ast::common::Parameter>> {
    if is_generic_call {
        return None;
    }
    let func_ty = ctx.type_checker.get_type(func_id)?;
    if let TypeKind::Function(func_data) = &func_ty.kind {
        Some(func_data.params.clone())
    } else {
        None
    }
}

/// Retain a value a coercion is about to take the reference of, when the local it
/// came from keeps holding it.
///
/// The coercion re-spells the value's type without copying it, so the reference
/// goes with it and the coerced temp is what gets released. A local variable is
/// released again when its scope ends, so it needs a reference of its own to give
/// away — otherwise one object is freed twice.
///
/// A parameter is not this case: it belongs to the caller, so reading it into a
/// temp already retains it. Neither is a field read, which produces a value the
/// reader owns, nor a constant, which is materialized fresh, nor an anonymous temp
/// that nothing else releases — there the coerced temp is the only holder left, and
/// retaining would strand the extra reference.
fn retain_still_held_value(ctx: &mut LoweringContext, op: &Operand, op_ty: &Type, span: Span) {
    if !ctx.is_perceus_managed(&op_ty.kind) {
        return;
    }
    let (Operand::Copy(place) | Operand::Move(place)) = op else {
        return;
    };
    let is_parameter = place.local.0 >= 1 && place.local.0 <= ctx.body.arg_count;
    let stays_held =
        ctx.body.local_decls[place.local.0].name.is_some() || ctx.is_owned_by_a_scope(place.local);
    if !place.projection.is_empty() || is_parameter || !stays_held {
        return;
    }
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::IncRef(place.clone()),
        span,
    });
}

fn lower_and_coerce_args(
    ctx: &mut LoweringContext,
    args: &[Expression],
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
) -> Vec<Operand> {
    let mut arg_ops = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        let mut op = lower_expression(ctx, arg, None).unwrap_or_else(|_| {
            Operand::Constant(Box::new(crate::mir::Constant {
                span: arg.span,
                ty: Type::new(TypeKind::Void, arg.span),
                literal: crate::ast::literal::Literal::None,
            }))
        });

        if let Some(params) = param_types {
            if i < params.len() {
                let target_ty = ctx.resolved_type(&params[i].typ);
                let op_ty = op.ty(&ctx.body).clone();
                if op_ty.kind != target_ty.kind && !spellings_of_one_value(&op_ty, &target_ty) {
                    let temp = ctx.push_temp(target_ty.clone(), arg.span);
                    retain_still_held_value(ctx, &op, &op_ty, arg.span);
                    ctx.push_statement(crate::mir::Statement {
                        kind: StatementKind::Assign(
                            Place::new(temp),
                            coerce_rvalue(op, &op_ty, &target_ty),
                        ),
                        span: arg.span,
                    });
                    op = Operand::Copy(Place::new(temp));
                }
            }
        }

        let op = match op {
            Operand::Move(p) => Operand::Copy(p),
            other => other,
        };
        arg_ops.push(op);
    }
    arg_ops
}

fn fill_default_args(
    ctx: &mut LoweringContext,
    arg_ops: &mut Vec<Operand>,
    param_types: &Option<Vec<crate::ast::common::Parameter>>,
) -> Result<(), LoweringError> {
    if let Some(params) = param_types {
        for param in params.iter().skip(arg_ops.len()) {
            if let Some(default_expr) = &param.default_value {
                let default_op = lower_expression(ctx, default_expr, None)?;
                arg_ops.push(default_op);
            }
        }
    }
    Ok(())
}

fn inject_allocator_arg(
    ctx: &mut LoweringContext,
    func_node: &ExpressionKind,
    func_op: &Operand,
    arg_ops: &mut Vec<Operand>,
) {
    let is_runtime_fn = if let ExpressionKind::Identifier(name, _) = func_node {
        name.starts_with("miri_")
    } else {
        false
    };
    let is_indirect_call = !matches!(
        func_op,
        Operand::Constant(ref c) if matches!(c.literal, crate::ast::literal::Literal::Identifier(_))
    );

    if is_runtime_fn || is_indirect_call {
        return;
    }

    let is_math_fn = if let ExpressionKind::Identifier(name, _) = func_node {
        MathIntrinsic::from_name(name.as_str()).is_some()
            && ctx
                .type_checker
                .get_variable_module(name.as_str())
                .map(|m| m == "system.math")
                .unwrap_or(false)
    } else {
        false
    };

    if is_math_fn {
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
