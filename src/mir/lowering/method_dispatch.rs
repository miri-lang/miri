// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method dispatch lowering — name mangling, inheritance resolution, virtual/static dispatch.

use crate::ast::expression::Expression;
use crate::ast::types::{STRING_TYPE_NAME, TUPLE_TYPE_NAME};
use crate::ast::{ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::{Local, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind};
use crate::runtime_fns::cow_fn;
use crate::type_checker::context::{
    class_needs_vtable, vtable_slot_index, MethodInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;

use super::{
    apply_generic_sub, is_monomorphizable_type_argument, lower_expression, LoweringContext,
};
use crate::ast::BuiltinCollectionKind;
use std::borrow::Cow;
use std::collections::HashMap;

/// Produce a mangled function name for a generic instantiation.
///
/// Example: `identity` with `[("T", int)]` → `identity__int`
pub(crate) fn mangle_generic_name(
    base: &str,
    type_args: &[(String, crate::ast::types::Type)],
) -> String {
    if type_args.is_empty() {
        return base.to_string();
    }

    let mut total_len = base.len();
    let mangled_types: Vec<Cow<'static, str>> = type_args
        .iter()
        .map(|(_, ty)| {
            let s = type_kind_to_mangle_str(&ty.kind);
            total_len += 2 + s.len();
            s
        })
        .collect();

    let mut path = String::with_capacity(total_len);
    path.push_str(base);
    for s in &mangled_types {
        path.push_str("__");
        path.push_str(s);
    }
    path
}

/// The token one type argument contributes to a mangled name.
///
/// A built-in kind spells itself. A user-defined type spells its own name, so
/// two instantiations of the same generic at two different classes get two
/// symbols: sharing one would make the second instantiation run the first one's
/// body against its own field layout. Only the built-in tokens are borrowed;
/// a named type has to own its string.
fn type_kind_to_mangle_str(kind: &TypeKind) -> Cow<'static, str> {
    let token: &'static str = match kind {
        TypeKind::Int => "int",
        TypeKind::Float | TypeKind::F64 => "float",
        TypeKind::F32 => "f32",
        TypeKind::Boolean => "bool",
        TypeKind::String => STRING_TYPE_NAME,
        TypeKind::Void => "void",
        TypeKind::Custom(name, _) => return Cow::Owned(name.clone()),
        TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => {
            unreachable!("collection types are normalized to Custom before this point")
        }
        TypeKind::Option(_) => "option",
        TypeKind::I8 => "i8",
        TypeKind::I16 => "i16",
        TypeKind::I32 => "i32",
        TypeKind::I64 => "i64",
        TypeKind::U8 => "u8",
        TypeKind::U16 => "u16",
        TypeKind::U32 => "u32",
        TypeKind::U64 => "u64",
        _ => "unknown",
    };
    Cow::Borrowed(token)
}

/// Residency-mangled name for a call that passes gpu-resident buffers into a
/// `GpuLaunchSafe` callee. Each gpu-resident argument contributes its argument
/// position and device handle, so distinct buffers monomorphize to distinct
/// bodies (and the same buffer reused across calls maps to one). The `__gpu`
/// segment can never appear in a user identifier, so the name cannot collide
/// with a user function or a generic instantiation. The original name is
/// recoverable as the substring before the first `__`.
pub(crate) fn residency_mangled_name(
    base: &str,
    handles: &[(usize, crate::mir::body::DeviceHandleId)],
) -> String {
    let mut name = String::from(base);
    name.push_str("__gpu");
    for (idx, handle) in handles {
        name.push_str(&format!("_p{}h{}", idx, handle.0));
    }
    name
}

/// Positional arguments that are gpu-resident bindings carrying a device handle.
/// Only bare identifier arguments bound to a `Gpu`-residency local qualify — the
/// buffer must reach the callee as the persistent device buffer, not a temp.
fn gpu_resident_call_args(
    ctx: &LoweringContext,
    args: &[Expression],
) -> Vec<(usize, crate::mir::body::DeviceHandleId)> {
    let mut out = Vec::new();
    for (idx, arg) in args.iter().enumerate() {
        let ExpressionKind::Identifier(name, _) = &arg.node else {
            continue;
        };
        let Some(local) = ctx.variable_map.get(name.as_str()) else {
            continue;
        };
        let decl = &ctx.body.local_decls[local.0];
        if matches!(decl.residency, crate::mir::body::BindingResidency::Gpu) {
            if let Some(handle) = decl.device_handle {
                out.push((idx, handle));
            }
        }
    }
    out
}

/// If a direct call passes gpu-resident buffers into a `GpuLaunchSafe` callee,
/// retarget `func_op` to the residency-specialized body and return the per-arg
/// device handles (positional, sized to `arg_ops`). Otherwise leaves `func_op`
/// untouched and returns an empty vector (an ordinary host call).
///
/// Only a `GpuLaunchSafe` callee is specialized: its buffer touches occur solely
/// inside `forall` (device) context, so the passed buffer is never read on the
/// host — the very property the type checker's residency gate enforces.
pub(crate) fn residency_specialize_call(
    ctx: &LoweringContext,
    func: &Expression,
    args: &[Expression],
    func_op: &mut Operand,
    arg_ops: &[Operand],
) -> Vec<Option<crate::mir::body::DeviceHandleId>> {
    let ExpressionKind::Identifier(func_name, _) = &func.node else {
        return Vec::new();
    };
    if !matches!(
        ctx.type_checker.fn_residencies().get(func_name.as_str()),
        Some(crate::type_checker::FnResidency::GpuLaunchSafe)
    ) {
        return Vec::new();
    }
    let gpu_args = gpu_resident_call_args(ctx, args);
    if gpu_args.is_empty() {
        return Vec::new();
    }

    if let Operand::Constant(constant) = &*func_op {
        if let crate::ast::literal::Literal::Identifier(base) = &constant.literal {
            let mangled = residency_mangled_name(base, &gpu_args);
            *func_op = super::dispatch::runtime_fn_operand(&mangled, func.span);
        }
    }

    let mut handles = vec![None; arg_ops.len()];
    for (idx, handle) in gpu_args {
        if idx < handles.len() {
            handles[idx] = Some(handle);
        }
    }
    handles
}

/// Walk the inheritance chain starting at `class_name` to find the first class
/// or trait that directly declares `method_name`. Returns the defining class/trait
/// name and a clone of its [`MethodInfo`] so the caller can mangle the symbol correctly.
///
/// This is the core of inherited method resolution: if `Dog extends Animal` and
/// only `Animal` defines `speak`, the returned defining class is `"Animal"` and
/// the call is mangled to `Animal_speak`.
///
/// **Concrete caller / abstract definer rule**: when the original `class_name` is a
/// *concrete* class and the method is found in an *abstract* ancestor, the caller's
/// name is returned instead of the ancestor's name.  This ensures static dispatch
/// goes to the per-concrete-class compiled version (e.g. `Array_is_empty`) rather
/// than the abstract-class version (`Collection_is_empty`), which would use virtual
/// dispatch internally and crash for objects that have no vtable pointer (Array, List).
///
/// Also handles:
/// - Trait-typed receivers: walks the trait hierarchy to find the method.
/// - Default trait methods: if the class doesn't define the method, checks all
///   implemented traits (and their parent traits) for a default (non-abstract) impl.
pub(crate) fn resolve_inherited_method(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    if matches!(type_defs.get(class_name), Some(TypeDefinition::Trait(_))) {
        return resolve_in_trait_hierarchy(type_defs, class_name, method_name);
    }

    if let Some(TypeDefinition::Enum(enum_def)) = type_defs.get(class_name) {
        if let Some(method_info) = enum_def.methods.get(method_name) {
            return Some((class_name.to_string(), method_info.clone()));
        }
        return None;
    }

    let caller_is_abstract = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(cd)) if cd.is_abstract
    );
    resolve_via_class_chain(type_defs, class_name, method_name, caller_is_abstract)
}

/// Walk the class's inheritance chain (and each class's traits) for `method_name`.
fn resolve_via_class_chain(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
    caller_is_abstract: bool,
) -> Option<(String, MethodInfo)> {
    let mut current = class_name.to_string();
    loop {
        let class_def = match type_defs.get(&current) {
            Some(TypeDefinition::Class(cd)) => {
                if let Some(method_info) = cd.methods.get(method_name) {
                    let defining = if cd.is_abstract && !caller_is_abstract {
                        class_name.to_string()
                    } else {
                        current.clone()
                    };
                    return Some((defining, method_info.clone()));
                }
                cd
            }
            _ => return None,
        };
        if let Some(found) = resolve_via_class_traits(
            type_defs,
            &class_def.traits,
            method_name,
            class_name,
            caller_is_abstract,
        ) {
            return Some(found);
        }
        match &class_def.base_class {
            Some(b) => current = b.clone(),
            None => return None,
        }
    }
}

/// Scan a class's directly-implemented traits for a default `method_name`. The
/// concrete-caller / abstract-definer rule mirrors the class-chain case.
fn resolve_via_class_traits(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    traits: &[String],
    method_name: &str,
    class_name: &str,
    caller_is_abstract: bool,
) -> Option<(String, MethodInfo)> {
    for trait_name in traits {
        if let Some((defining_trait, info)) =
            resolve_trait_default_method(type_defs, trait_name, method_name)
        {
            let defining = if caller_is_abstract {
                defining_trait
            } else {
                class_name.to_string()
            };
            return Some((defining, info));
        }
    }
    None
}

/// Walk the trait hierarchy to find `method_name`. Returns the defining trait
/// name and method info (abstract or concrete).
fn resolve_in_trait_hierarchy(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    trait_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    let mut to_check = vec![trait_name];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(t_name) {
            if let Some(method_info) = td.methods.get(method_name) {
                return Some((t_name.to_string(), method_info.clone()));
            }
            to_check.extend(td.parent_traits.iter().map(|s| s.as_str()));
        }
    }
    None
}

/// Walk the trait hierarchy (starting from `trait_name`) to find a non-abstract
/// (default) implementation of `method_name`. Returns None if only abstract
/// declarations exist or the method is not found.
fn resolve_trait_default_method(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    trait_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    let mut to_check = vec![trait_name];
    let mut visited = std::collections::HashSet::new();
    while let Some(t_name) = to_check.pop() {
        if !visited.insert(t_name) {
            continue;
        }
        if let Some(TypeDefinition::Trait(td)) = type_defs.get(t_name) {
            if let Some(method_info) = td.methods.get(method_name) {
                if !method_info.is_abstract {
                    return Some((t_name.to_string(), method_info.clone()));
                }
            }
            to_check.extend(td.parent_traits.iter().map(|s| s.as_str()));
        }
    }
    None
}

/// Emit a virtual method call through a vtable slot.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_virtual_method_call(
    ctx: &mut LoweringContext,
    vtable_slot: usize,
    self_op: Operand,
    user_args: &[Expression],
    method_info: &MethodInfo,
    destination: &Place,
    op: &Operand,
    obj_temp_local: Option<Local>,
    obj_watermark: usize,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let mut call_args = vec![self_op];
    let arg_watermark = ctx.body.local_decls.len();
    for arg in user_args {
        call_args.push(lower_expression(ctx, arg, None)?);
    }
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }

    let out_args =
        super::dispatch::build_method_out_args(method_info, user_args.len(), call_args.len());
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::VirtualCall {
            vtable_slot,
            args: call_args.clone(),
            out_args,
            destination: destination.clone(),
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(local) = obj_temp_local {
        ctx.emit_temp_drop(local, obj_watermark, span);
    }
    super::dispatch::emit_method_arg_drops(
        ctx,
        &call_args[1..],
        arg_watermark,
        destination.local,
        span,
    );
    Ok(Some(op.clone()))
}

/// Emit a static method call (direct function call).
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_static_method_call(
    ctx: &mut LoweringContext,
    symbol: &str,
    self_op: Operand,
    user_args: &[Expression],
    method_info: &MethodInfo,
    destination: &Place,
    op: &Operand,
    obj_temp_local: Option<Local>,
    obj_watermark: usize,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let mangled_name = symbol.to_string();
    let mut call_args = vec![self_op];
    let arg_watermark = ctx.body.local_decls.len();
    for arg in user_args {
        call_args.push(lower_expression(ctx, arg, None)?);
    }
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }

    let func_op = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(mangled_name),
    }));

    let out_args =
        super::dispatch::build_method_out_args(method_info, user_args.len(), call_args.len());
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args.clone(),
            out_args,
            arg_handles: Vec::new(),
            destination: destination.clone(),
            target: Some(target_bb),
        },
        span,
    ));
    ctx.set_current_block(target_bb);
    if let Some(local) = obj_temp_local {
        ctx.emit_temp_drop(local, obj_watermark, span);
    }
    super::dispatch::emit_method_arg_drops(
        ctx,
        &call_args[1..],
        arg_watermark,
        destination.local,
        span,
    );
    Ok(Some(op.clone()))
}

/// Resolve the receiver type override for inherited methods in abstract classes.
pub(super) fn resolve_receiver_override(
    ctx: &LoweringContext,
    raw_obj_ty: &Type,
    obj: &Expression,
) -> Option<Type> {
    if let TypeKind::Custom(name, _) = &raw_obj_ty.kind {
        let type_defs = ctx.type_checker.type_definitions();
        let needs_override = matches!(
            type_defs.get(name.as_str()),
            Some(TypeDefinition::Class(cd)) if cd.is_abstract
        ) || matches!(
            ctx.type_checker
                .type_table
                .global_type_definitions
                .get(name.as_str()),
            Some(TypeDefinition::Trait(_))
        );
        if needs_override {
            if let ExpressionKind::Identifier(var_name, _) = &obj.node {
                if let Some(&local) = ctx.variable_map.get(var_name.as_str()) {
                    return Some(ctx.body.local_decls[local.0].ty.clone());
                }
            }
        }
    }
    None
}

/// Extract the class name from a type, handling builtins and custom types.
pub(super) fn extract_class_name(obj_ty: &Type) -> Option<String> {
    match &obj_ty.kind {
        TypeKind::String => Some(STRING_TYPE_NAME.to_string()),
        TypeKind::Tuple(_) => Some(TUPLE_TYPE_NAME.to_string()),
        TypeKind::Custom(name, _) => Some(name.clone()),
        k => k.as_builtin_collection().map(|b| b.name().to_string()),
    }
}

/// Lower a method call on a class or trait object.
///
/// This handles inheritance resolution, virtual vs static dispatch, and specialized
/// collection intrinsics (`push`, `get`, etc.).
pub(super) fn try_lower_method_call(
    ctx: &mut LoweringContext,
    span: &Span,
    call_expr_id: usize,
    obj: &Expression,
    method_expr: &Expression,
    args: &[Expression],
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let Some((obj_ty, class_name, method_name)) = resolve_method_receiver(ctx, obj, method_expr)
    else {
        return Ok(None);
    };

    if let Some(op) = super::dispatch::try_lower_collection_intrinsic(
        ctx,
        super::dispatch::CollectionIntrinsicCall {
            span,
            call_expr_id,
            obj,
            obj_ty: &obj_ty,
            method_name: &method_name,
            args,
        },
        dest.as_ref().cloned(),
    )? {
        return Ok(Some(op));
    }

    let Some((defining_class, method_info)) = resolve_inherited_method(
        ctx.type_checker.type_definitions(),
        &class_name,
        &method_name,
    ) else {
        return Ok(None);
    };

    emit_resolved_method_call(
        ctx,
        ResolvedMethod {
            span,
            call_expr_id,
            obj,
            obj_ty: &obj_ty,
            class_name: &class_name,
            method_name: &method_name,
            defining_class: &defining_class,
            method_info: &method_info,
            args,
        },
        dest,
    )
}

/// Resolve a method call's receiver type (applying abstract/trait overrides),
/// class name, and method name. Returns owned values to avoid borrowing `ctx`.
fn resolve_method_receiver(
    ctx: &LoweringContext,
    obj: &Expression,
    method_expr: &Expression,
) -> Option<(Type, String, String)> {
    let raw_obj_ty = ctx.recorded_type(obj.id)?;
    let obj_ty = resolve_receiver_override(ctx, &raw_obj_ty, obj).unwrap_or(raw_obj_ty);
    let class_name = extract_class_name(&obj_ty)?;
    let method_name = match &method_expr.node {
        ExpressionKind::Identifier(name, _) => name.clone(),
        _ => return None,
    };
    Some((obj_ty, class_name, method_name))
}

/// A method call whose receiver type and target method have been resolved.
struct ResolvedMethod<'a> {
    span: &'a Span,
    /// Expression id of the whole call, used to read the concrete type the type
    /// checker inferred for its result.
    call_expr_id: usize,
    obj: &'a Expression,
    obj_ty: &'a Type,
    class_name: &'a str,
    method_name: &'a str,
    defining_class: &'a str,
    method_info: &'a MethodInfo,
    args: &'a [Expression],
}

/// Emit a resolved user-method call via virtual (vtable) or static dispatch.
fn emit_resolved_method_call(
    ctx: &mut LoweringContext,
    m: ResolvedMethod,
    dest: Option<Place>,
) -> Result<Option<Operand>, LoweringError> {
    let mono = resolve_generic_class_monomorph(ctx, m.obj_ty, m.method_name, m.method_info);
    let return_ty = call_result_type(ctx, &m, &mono);
    let obj_watermark = ctx.body.local_decls.len();
    let (self_op, obj_temp_local) =
        prepare_method_self(ctx, m.obj, m.obj_ty, m.method_name, *m.span)?;
    let (destination, op) = super::dispatch::call_destination(ctx, return_ty, dest, *m.span);

    if should_use_virtual_dispatch(ctx, m.obj, m.class_name) {
        if let Some(slot) = vtable_slot_index(
            m.class_name,
            m.method_name,
            ctx.type_checker.type_definitions(),
        ) {
            return emit_virtual_method_call(
                ctx,
                slot,
                self_op,
                m.args,
                m.method_info,
                &destination,
                &op,
                obj_temp_local,
                obj_watermark,
                *m.span,
            );
        }
    }
    let symbol = match mono {
        Some((mangled, _)) => mangled,
        None => format!("{}_{}", m.defining_class, m.method_name),
    };
    emit_static_method_call(
        ctx,
        &symbol,
        self_op,
        m.args,
        m.method_info,
        &destination,
        &op,
        obj_temp_local,
        obj_watermark,
        *m.span,
    )
}

/// The type to give the local that receives a method call's result.
///
/// A generic method declares its result in the class's own parameters — `V?`
/// for `Map<String, Node>.get` — and a local typed that way tells Perceus
/// nothing about what it holds, so the result is never released. The type
/// checker already inferred the concrete type at this call site; the declared
/// return type is the fallback for a call it did not record.
fn call_result_type(
    ctx: &LoweringContext,
    m: &ResolvedMethod,
    mono: &Option<(String, Type)>,
) -> Type {
    if let Some(inferred) = ctx.type_checker.get_type(m.call_expr_id) {
        let resolved = ctx.resolve_self_in(inferred);
        return apply_generic_sub(&resolved, &ctx.generic_subs);
    }
    match mono {
        Some((_, concrete_return)) => concrete_return.clone(),
        None => m.method_info.return_type.clone(),
    }
}

/// Whether a type argument is laid out differently from the pointer-width
/// integer that an unmonomorphized generic body falls back to.
///
/// A narrower or wider integer changes the operand width, and a float changes
/// the register class outright, so either makes the shared body's signature
/// disagree with the call site. `int` matches the fallback exactly, and every
/// managed type is passed as a pointer, so both are already correct.
fn differs_from_pointer_width_fallback(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Boolean
    )
}

/// Resolve a generic-class method call to its per-instantiation monomorphized
/// symbol and concrete return type, or `None` when the plain generic body applies.
fn resolve_generic_class_monomorph(
    ctx: &LoweringContext,
    obj_ty: &Type,
    method_name: &str,
    method_info: &MethodInfo,
) -> Option<(String, Type)> {
    let TypeKind::Custom(name, Some(arg_exprs)) = &obj_ty.kind else {
        return None;
    };
    let defs = &ctx.type_checker.type_definitions();
    let Some(TypeDefinition::Class(class_def)) = defs.get(name.as_str()) else {
        return None;
    };
    let gens = class_def.generics.as_ref()?;
    let resolved: Vec<Type> = arg_exprs
        .iter()
        .map(|e| ctx.type_checker.extract_type_from_expression(e))
        .collect::<Result<_, _>>()
        .ok()?;
    if resolved.len() != gens.len()
        || !resolved
            .iter()
            .all(|t| is_monomorphizable_type_argument(&t.kind, defs))
    {
        return None;
    }
    // A builtin collection only needs a per-instantiation body where the shared
    // generic one is ABI-wrong. That body types every type-parameter position at
    // the pointer-width integer fallback, which is already correct for `int` and
    // for any managed element (a pointer), so monomorphizing those would copy
    // dozens of methods per element type for no change in generated code.
    if BuiltinCollectionKind::from_name(name.as_str()).is_some()
        && !resolved
            .iter()
            .any(|arg| differs_from_pointer_width_fallback(&arg.kind))
    {
        return None;
    }
    // A method taking a function parameter keeps the shared generic body:
    // lowering a lambda argument inside a per-instantiation copy is not
    // supported, and mangling here without an emitted body would leave the call
    // referencing a symbol nothing defines.
    if method_info
        .params
        .iter()
        .any(|(_, param_ty)| matches!(param_ty.kind, TypeKind::Function(_)))
    {
        return None;
    }
    let recorded = ctx
        .type_checker
        .generic_class_instantiations
        .get(name.as_str())?;
    let is_recorded = recorded.iter().any(|tuple| {
        tuple.len() == resolved.len() && tuple.iter().zip(&resolved).all(|(a, b)| a.kind == b.kind)
    });
    if !is_recorded {
        return None;
    }
    let mut subs = HashMap::new();
    let type_args: Vec<(String, Type)> = gens
        .iter()
        .zip(&resolved)
        .map(|(g, t)| {
            subs.insert(g.name.clone(), t.clone());
            (g.name.clone(), t.clone())
        })
        .collect();
    let mangled = mangle_generic_name(&format!("{name}_{method_name}"), &type_args);
    extend_subs_with_trait_params(ctx.type_checker, name, &mut subs);
    let return_ty = apply_generic_sub(&method_info.return_type, &subs);
    Some((mangled, return_ty))
}

/// Add `trait-param → concrete` entries to a class-instantiation substitution,
/// resolving each directly-implemented trait's `implements Trait<args>` binding
/// through the existing class-param map. See
/// [`TypeChecker::class_trait_param_bindings`] for the binding source.
pub(crate) fn extend_subs_with_trait_params(
    tc: &TypeChecker,
    class_name: &str,
    subs: &mut HashMap<String, Type>,
) {
    let bindings = tc.class_trait_param_bindings(class_name);
    for (trait_param, class_arg) in bindings {
        let concrete = apply_generic_sub(&class_arg, subs);
        subs.insert(trait_param, concrete);
    }
}

/// Lower the receiver, apply a CoW check for mutating collection methods, and
/// return the self operand plus the receiver temp local (for Perceus drops).
fn prepare_method_self(
    ctx: &mut LoweringContext,
    obj: &Expression,
    obj_ty: &Type,
    method_name: &str,
    span: Span,
) -> Result<(Operand, Option<Local>), LoweringError> {
    let self_op = lower_method_receiver(ctx, obj)?;
    let self_op = match obj_ty
        .kind
        .as_builtin_collection()
        .filter(|k| k.mutates_method(method_name))
        .and_then(cow_fn)
    {
        Some(cow) => emit_cow_check(ctx, self_op, obj_ty, cow, span),
        None => self_op,
    };
    let obj_temp_local = if let Operand::Copy(ref p) = self_op {
        Some(p.local)
    } else {
        None
    };
    Ok((self_op, obj_temp_local))
}

/// Lower a method receiver, resolving `super` to the `self` binding.
fn lower_method_receiver(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Operand, LoweringError> {
    if matches!(&obj.node, ExpressionKind::Super) {
        if let Some(&self_local) = ctx.variable_map.get("self") {
            return Ok(Operand::Copy(Place::new(self_local)));
        }
    }
    lower_expression(ctx, obj, None)
}

/// True when the receiver's static type requires vtable (virtual) dispatch:
/// an abstract class with a vtable, or a trait-typed receiver. `super` calls
/// always dispatch statically.
fn should_use_virtual_dispatch(ctx: &LoweringContext, obj: &Expression, class_name: &str) -> bool {
    if matches!(&obj.node, ExpressionKind::Super) {
        return false;
    }
    let defs = &ctx.type_checker.type_definitions();
    let abstract_with_vtable = class_needs_vtable(class_name, defs)
        && matches!(defs.get(class_name), Some(TypeDefinition::Class(cd)) if cd.is_abstract);
    let is_trait = matches!(defs.get(class_name), Some(TypeDefinition::Trait(_)));
    abstract_with_vtable || is_trait
}

/// Emit a Copy-on-Write check before a mutation operation on a collection local.
///
/// If the receiver is a simple local variable (`Move` with no projection), emits a call to
/// `cow_fn_name` that returns either the same pointer (RC ≤ 1 → no copy) or a fresh exclusive
/// clone (RC > 1 → clone + decrement old RC). The result is stored back into the receiver local
/// so the subsequent mutation operates on an exclusively-owned collection.
///
/// `Assign` (not `Reassign`) is used for the write-back so Perceus does not DecRef the old
/// value; `Move` is used for the cow_result so Perceus does not IncRef it. No `StorageDead` is
/// emitted for the cow_result temp — its ownership is transferred to self_local.
pub(super) fn emit_cow_check(
    ctx: &mut LoweringContext,
    obj_op: Operand,
    obj_ty: &Type,
    cow_fn_name: &str,
    span: Span,
) -> Operand {
    let self_local = match &obj_op {
        Operand::Move(p) if p.projection.is_empty() => p.local,
        _ => return obj_op,
    };
    let cow_result = ctx.push_temp(obj_ty.clone(), span);
    let cow_fn = Operand::Constant(Box::new(crate::mir::Constant {
        span,
        ty: Type::new(TypeKind::Identifier, span),
        literal: crate::ast::literal::Literal::Identifier(cow_fn_name.to_string()),
    }));
    let cow_target = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: cow_fn,
            args: vec![Operand::Move(Place::new(self_local))],
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(cow_result),
            target: Some(cow_target),
        },
        span,
    ));
    ctx.set_current_block(cow_target);
    ctx.push_statement(crate::mir::Statement {
        kind: StatementKind::Assign(
            Place::new(self_local),
            Rvalue::Use(Operand::Move(Place::new(cow_result))),
        ),
        span,
    });
    Operand::Move(Place::new(self_local))
}
