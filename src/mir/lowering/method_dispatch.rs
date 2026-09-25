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
use crate::type_checker::context::{class_needs_vtable, MethodInfo, TypeDefinition};
use crate::type_checker::TypeChecker;

use super::class_instantiations::is_registered_instantiation;
use super::dispatch_symbols::{instantiation_substitution, trait_default_among, vtable_slot_index};
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

/// The mangled name of one instantiation of a generic class, e.g. `Box` at
/// `[String]` → `Box__String`. The parameter names play no part in the symbol,
/// so only the arguments are needed.
pub(crate) fn mangle_instantiation_name(class_name: &str, type_args: &[Type]) -> String {
    let pairs: Vec<(String, Type)> = type_args
        .iter()
        .map(|ty| (String::new(), ty.clone()))
        .collect();
    mangle_generic_name(class_name, &pairs)
}

/// The token [`type_kind_to_mangle_str`] yields for a type it cannot name.
///
/// It is the one answer callers test against: a type that spells this has no
/// per-instantiation symbol, so whatever needs one has to fall back to the
/// shared generic body. Every caller that decides whether a body can be named
/// asks [`crate::mir::lowering::has_a_monomorphized_spelling`], which is this
/// comparison — so the spelling and the decision can never drift apart. The
/// leading underscores keep it out of reach of a user type that spells itself
/// the same way, which would otherwise lose its own body to this answer.
pub(crate) const UNSPELLABLE_TYPE_TOKEN: &str = "__unspellable";

/// How deep [`type_kind_to_mangle_str`] descends into a type's components
/// before giving up on naming it.
///
/// A type nested past this is spelled [`UNSPELLABLE_TYPE_TOKEN`], which costs
/// it a per-instantiation body and nothing else — the shared generic one still
/// compiles, and the verifier's exemption reads the same answer. Bounding the
/// descent keeps a deeply nested type in a source file from being a way to
/// exhaust the compiler's stack.
const MAX_TOKEN_DEPTH: usize = 64;

/// The token one type argument contributes to a mangled name.
///
/// A built-in kind spells itself. A user-defined type spells its own name, so
/// two instantiations of the same generic at two different classes get two
/// symbols: sharing one would make the second instantiation run the first one's
/// body against its own field layout.
///
/// A type built out of others — an instantiated generic class, an optional, a
/// tuple — spells its own token followed by its components', because those
/// components are what a body compiled for it addresses. A `List<String>`
/// element and a `List<int>` element that shared the token `List` would share a
/// body, and the one holding strings would then be filled without taking a
/// reference to any of them.
///
/// A component that has no token of its own makes the whole type unspellable:
/// a name built from [`UNSPELLABLE_TYPE_TOKEN`] would be the same name for
/// every type that contains one.
fn type_kind_to_mangle_str(kind: &TypeKind) -> Cow<'static, str> {
    type_kind_token(kind, 0)
}

/// [`type_kind_to_mangle_str`] one level into a type, carrying how far the
/// descent has already gone.
fn type_kind_token(kind: &TypeKind, depth: usize) -> Cow<'static, str> {
    if depth >= MAX_TOKEN_DEPTH {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    if let Some(value_expr) = crate::type_checker::generics::extract_value_generic_kind(kind) {
        return expression_token(value_expr, depth + 1);
    }
    let token: &'static str = match kind {
        TypeKind::Int => "int",
        TypeKind::Float | TypeKind::F64 => "float",
        TypeKind::F32 => "f32",
        TypeKind::F16 => "f16",
        TypeKind::Boolean => "bool",
        TypeKind::String => STRING_TYPE_NAME,
        TypeKind::Void => "void",
        TypeKind::Custom(name, None) => return Cow::Owned(name.clone()),
        TypeKind::Custom(name, Some(args)) => return compound_token(name, args.iter(), depth),
        TypeKind::List(inner) => {
            return compound_token(
                BuiltinCollectionKind::List.name(),
                std::iter::once(&**inner),
                depth,
            )
        }
        TypeKind::Set(inner) => {
            return compound_token(
                BuiltinCollectionKind::Set.name(),
                std::iter::once(&**inner),
                depth,
            )
        }
        TypeKind::Array(inner, size) => {
            return compound_token(
                BuiltinCollectionKind::Array.name(),
                [&**inner, &**size].into_iter(),
                depth,
            )
        }
        TypeKind::Map(key, value) => {
            return compound_token(
                BuiltinCollectionKind::Map.name(),
                [&**key, &**value].into_iter(),
                depth,
            )
        }
        TypeKind::Option(inner) => {
            return join_tokens(
                "option",
                std::iter::once(type_kind_token(&inner.kind, depth + 1)),
            )
        }
        TypeKind::Tuple(elements) => return compound_token("tuple", elements.iter(), depth),
        TypeKind::I8 => "i8",
        TypeKind::I16 => "i16",
        TypeKind::I32 => "i32",
        TypeKind::I64 => "i64",
        TypeKind::I128 => "i128",
        TypeKind::U8 => "u8",
        TypeKind::U16 => "u16",
        TypeKind::U32 => "u32",
        TypeKind::U64 => "u64",
        TypeKind::U128 => "u128",
        TypeKind::Generic(_, _, _)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_)
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Error => UNSPELLABLE_TYPE_TOKEN,
    };
    Cow::Borrowed(token)
}

/// The token for a type written as `head` applied to `args`, e.g. `Map<String,
/// int>` → `Map_String_int`.
fn compound_token<'a>(
    head: &str,
    args: impl Iterator<Item = &'a Expression>,
    depth: usize,
) -> Cow<'static, str> {
    join_tokens(head, args.map(|arg| expression_token(arg, depth + 1)))
}

/// Join `head` and each component token with `_`, collapsing to
/// [`UNSPELLABLE_TYPE_TOKEN`] as soon as a component has no token of its own.
fn join_tokens(head: &str, parts: impl Iterator<Item = Cow<'static, str>>) -> Cow<'static, str> {
    let mut out = String::from(head);
    for part in parts {
        if part == UNSPELLABLE_TYPE_TOKEN {
            return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
        }
        out.push('_');
        out.push_str(&part);
    }
    Cow::Owned(out)
}

/// The token a generic argument expression contributes to a mangled name.
///
/// A type argument spells its type. A value generic — the `3` in `Array<T, 3>`
/// — spells the constant itself, so two sizes of the same element type get two
/// symbols instead of sharing one body between them. Anything else has no
/// token: a size that is still an unfolded expression names no single
/// instantiation.
fn expression_mangle_token(arg: &Expression) -> Cow<'static, str> {
    expression_token(arg, 0)
}

/// [`expression_mangle_token`] one level into a type, carrying how far the
/// descent has already gone.
fn expression_token(arg: &Expression, depth: usize) -> Cow<'static, str> {
    if depth >= MAX_TOKEN_DEPTH {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    match &arg.node {
        ExpressionKind::Type(ty, _) => type_kind_token(&ty.kind, depth),
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(value)) => {
            Cow::Owned(value.to_i128().to_string())
        }
        _ => Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN),
    }
}

/// Whether the symbol mangler has a token for `kind`. See
/// [`UNSPELLABLE_TYPE_TOKEN`].
pub(crate) fn kind_has_a_mangled_token(kind: &TypeKind) -> bool {
    type_kind_to_mangle_str(kind) != UNSPELLABLE_TYPE_TOKEN
}

/// Whether the symbol mangler has a token for the generic argument `arg`. See
/// [`expression_mangle_token`].
pub(crate) fn argument_has_a_mangled_token(arg: &Expression) -> bool {
    expression_mangle_token(arg) != UNSPELLABLE_TYPE_TOKEN
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
///
/// TODO: a default a subclass's own trait supplies is preferred over a method
/// its base class declares, and resolved to a `{Subclass}_{method}` copy that
/// is never lowered when the base already declares the method, so the call
/// fails to link (`class Child extends Base implements Named`, where both
/// `Base` and `Named` supply `name()`).
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

/// The default `method_name` a class's directly-implemented traits supply, by
/// [`trait_default_among`]. The concrete-caller / abstract-definer rule mirrors
/// the class-chain case.
fn resolve_via_class_traits(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    traits: &[String],
    method_name: &str,
    class_name: &str,
    caller_is_abstract: bool,
) -> Option<(String, MethodInfo)> {
    let (defining_trait, info) = trait_default_among(type_defs, traits, method_name)?;
    let defining = if caller_is_abstract {
        defining_trait
    } else {
        class_name
    };
    Some((defining.to_string(), info.clone()))
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
    let mono = match &m.obj.node {
        ExpressionKind::Super => resolve_super_monomorph(ctx, m.method_name, m.method_info),
        _ => resolve_generic_class_monomorph(ctx, m.obj_ty, m.method_name, m.method_info),
    };
    if mono.is_some() {
        ctx.record_class_instantiations(m.obj_ty);
    }
    let return_ty = call_result_type(ctx, &m, &mono);
    let obj_watermark = ctx.body.local_decls.len();
    let (self_op, obj_temp_local) =
        prepare_method_self(ctx, m.obj, m.obj_ty, m.method_name, *m.span)?;
    let (destination, op) = super::dispatch::call_destination(ctx, return_ty, dest, *m.span);

    if should_use_virtual_dispatch(ctx, m.obj, m.class_name) {
        if let Some(slot) = vtable_slot_index(
            &ctx.vtable_layout(),
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
        None => {
            // Optimization: avoid format! overhead in symbol mangling on hot method-dispatch paths.
            let mut s = String::with_capacity(m.defining_class.len() + 1 + m.method_name.len());
            s.push_str(m.defining_class);
            s.push('_');
            s.push_str(m.method_name);
            s
        }
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
/// The element layout decides it: an element that is not one value word — a
/// narrower or wider scalar, or an inline vector the collection holds by its
/// components — changes the operand width, and a float changes the register
/// class outright, so either makes the shared body's signature disagree with
/// the call site and hand the runtime a word where the element is something
/// else. `int` matches the fallback exactly, and every managed type is passed
/// as a pointer, so both agree on layout — a managed argument needs its own
/// body for a different reason, spelled out in
/// [`shared_body_would_borrow_a_managed_element`].
fn differs_from_pointer_width_fallback(kind: &TypeKind) -> bool {
    let layout = crate::ast::types::element_layout(kind);
    layout.is_address
        || layout.payload != crate::ast::types::VALUE_WORD_BYTES
        || crate::type_checker::float_literals::is_float_width(kind)
}

/// Whether a type argument sorts differently from the signed integer an
/// unmonomorphized generic body falls back to.
///
/// A list the shared body builds carries no element order of its own, so the
/// runtime reads its elements as signed values. That is wrong for an unsigned
/// integer with its top bit set and for every negative float. Most such types
/// already differ in layout; `u64` does not, and only its order asks for a body
/// of its own.
fn orders_differently_from_signed_fallback(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
    )
}

/// Whether a built-in collection's `method_name` settles the ownership of the
/// elements it touches by calling the runtime, which makes its shared generic
/// body correct at every element type.
///
/// A method the collection declares itself pairs each element read with the
/// runtime call that hands the container's own reference over (`pop` and
/// `remove_at` do), and only a body that can name the intrinsic can pair with
/// it; re-lowering one against an owning read would leave that donated reference
/// with no one to release it.
///
/// A declared method that takes a function value does not. It builds its result
/// from what that function hands back and from the collection's own element
/// reads — ordinary Miri code, like a trait default — so no intrinsic accounts
/// for either, and neither does a method the collection only inherits.
pub(crate) fn is_settled_by_the_runtime(
    class_def: &crate::type_checker::context::ClassDefinition,
    method_name: &str,
) -> bool {
    class_def.methods.get(method_name).is_some_and(|method| {
        !method
            .params
            .iter()
            .any(|(_, param_ty)| matches!(param_ty.kind, TypeKind::Function(_)))
    })
}

/// Whether the shared generic body of `method_name` would read `elem_kind` as a
/// borrow the resulting value does not own.
///
/// A method written in ordinary Miri code reaches an element only through
/// `element_at`, and stores it in a new collection, returns it, or hands it to a
/// function value. Lowered once per receiver class, its element type stays a
/// type parameter, so Perceus reads it as unmanaged: it takes no reference to
/// what it stores and releases nothing a function value returns — while the call
/// site, which knows the concrete element, releases every element of the
/// collection it gets back. Giving the body the concrete element makes both
/// sides agree.
fn shared_body_would_borrow_a_managed_element(
    class_def: &crate::type_checker::context::ClassDefinition,
    method_name: &str,
    elem_kind: &TypeKind,
) -> bool {
    !is_settled_by_the_runtime(class_def, method_name)
        && crate::mir::rc::is_field_managed(elem_kind)
}

/// Whether a built-in collection instantiated at `resolved` needs a
/// per-instantiation body for `method_name`, rather than the shared generic one.
///
/// Three things ask for one. The shared body types every type-parameter
/// position at the pointer-width integer fallback, so a differently-laid-out
/// argument makes its signature disagree with the call site, and an argument
/// ordered unlike a signed integer would sort wrongly in any list the body
/// builds. And a body written in ordinary Miri code that reads a managed
/// element takes no reference to it, while the call site releases every element
/// of the collection it gets back.
///
/// Every other class always needs one, so it passes straight through.
fn builtin_collection_needs_its_own_body(
    class_name: &str,
    class_def: &crate::type_checker::context::ClassDefinition,
    method_name: &str,
    resolved: &[Type],
) -> bool {
    if BuiltinCollectionKind::from_name(class_name).is_none() {
        return true;
    }
    resolved
        .iter()
        // A value generic occupies a parameter slot but describes no storage:
        // an `Array`'s size neither changes the width of a type-parameter
        // position nor holds an element anyone has to release. Both questions
        // below are about a type argument, and asking them of a size would
        // answer from the marker's own spelling.
        .filter(|arg| crate::type_checker::generics::extract_value_generic(arg).is_none())
        .any(|arg| {
            differs_from_pointer_width_fallback(&arg.kind)
                || orders_differently_from_signed_fallback(&arg.kind)
                || shared_body_would_borrow_a_managed_element(class_def, method_name, &arg.kind)
        })
}

/// One argument of a generic-class reference as the instantiation registry
/// records it: the argument's type, or the marker standing for a value generic.
/// `None` when the argument is neither.
pub(crate) fn resolve_generic_argument(tc: &TypeChecker, arg: &Expression) -> Option<Type> {
    match tc.extract_type_from_expression(arg) {
        Ok(ty) => Some(ty),
        Err(_) => crate::type_checker::generics::value_generic_slot(arg),
    }
}

/// The symbol an operator calls for `owner`'s `method_name` on a receiver of
/// `receiver_ty`, and the type that call returns.
///
/// An operator reaches the same body a written call to the method does. For an
/// instantiation of a generic class that is the body compiled for it: the
/// shared one reads every type-parameter value as an unmanaged word, so its
/// `self.value == other.value` over two `String`s compares their addresses.
/// A method the class inherits from another class is named by that class, at
/// the type arguments the `extends` chain maps the receiver's onto — so the
/// operator, a written call and a container's element thunk all reach one body.
pub(crate) fn operator_method_callee(
    ctx: &mut LoweringContext,
    receiver_ty: &Type,
    owner: &str,
    method_name: &str,
    method: &MethodInfo,
) -> (String, Type) {
    match resolve_generic_class_monomorph(ctx, receiver_ty, method_name, method) {
        Some(callee) => {
            ctx.record_class_instantiations(receiver_ty);
            callee
        }
        None => {
            // Optimization: pre-allocate exact capacity for static method symbol to eliminate format! parsing overhead.
            let mut s = String::with_capacity(owner.len() + 1 + method_name.len());
            s.push_str(owner);
            s.push('_');
            s.push_str(method_name);
            (s, method.return_type.clone())
        }
    }
}

/// Resolve a generic-class method call to its per-instantiation monomorphized
/// symbol and concrete return type, or `None` when the plain generic body applies.
///
/// The symbol names the class that declares the method, at the type arguments
/// that class is instantiated at — which the `extends` chain maps from the
/// receiver's own. A receiver's instantiation is what gets registered, and the
/// pipeline derives its ancestors' from it, so the named body is lowered.
fn resolve_generic_class_monomorph(
    ctx: &LoweringContext,
    obj_ty: &Type,
    method_name: &str,
    method_info: &MethodInfo,
) -> Option<(String, Type)> {
    let (name, resolved) = receiver_instantiation(ctx, obj_ty)?;
    monomorph_for_instantiation(ctx, &name, &resolved, method_name, method_info)
}

/// Resolve a `super.method(...)` call inside a generic class to the body
/// compiled for the base class's own instantiation.
///
/// `super` is typed as the bare base-class name, carrying none of the type
/// arguments the `extends` clause gives it, so the receiver type alone would
/// name the shared body — which reads a managed type argument as an unmanaged
/// word and so stores a field without taking a reference to it. The enclosing
/// class's instantiation is what supplies them.
fn resolve_super_monomorph(
    ctx: &LoweringContext,
    method_name: &str,
    method_info: &MethodInfo,
) -> Option<(String, Type)> {
    let self_type = ctx.self_type.as_ref()?;
    let (class_name, class_args) = receiver_instantiation(ctx, self_type)?;
    let (base, base_args) =
        crate::mir::lowering::inherited_instantiation::base_class_instantiation(
            ctx.type_checker.type_definitions(),
            &class_name,
            &class_args,
        )?;
    monomorph_for_instantiation(ctx, &base, &base_args, method_name, method_info)
}

/// The class and concrete type arguments a receiver's type names, or `None`
/// when it names no monomorphizable instantiation of a generic class.
///
/// A class that declares no parameters is reached at no arguments and still
/// names an instantiation: the class it extends may pin the parent's
/// parameters (`class Child extends Base<String>`), and every body the child
/// inherits belongs to that instantiation of the parent.
fn receiver_instantiation(ctx: &LoweringContext, obj_ty: &Type) -> Option<(String, Vec<Type>)> {
    let TypeKind::Custom(name, arg_exprs) = &obj_ty.kind else {
        return None;
    };
    let defs = &ctx.type_checker.type_definitions();
    let Some(TypeDefinition::Class(class_def)) = defs.get(name.as_str()) else {
        return None;
    };
    let Some(gens) = class_def.generics.as_ref() else {
        return Some((name.clone(), Vec::new()));
    };
    let arg_exprs = arg_exprs.as_ref()?;
    let resolved: Vec<Type> = arg_exprs
        .iter()
        .map(|e| resolve_generic_argument(ctx.type_checker, e))
        .collect::<Option<_>>()?;
    if resolved.len() != gens.len()
        || !resolved
            .iter()
            .all(|t| is_monomorphizable_type_argument(&t.kind, defs))
    {
        return None;
    }
    Some((name.clone(), resolved))
}

/// Resolve the monomorphized symbol and return type for `method_name` called on
/// `name` instantiated at `resolved`.
fn monomorph_for_instantiation(
    ctx: &LoweringContext,
    name: &str,
    resolved: &[Type],
    method_name: &str,
    method_info: &MethodInfo,
) -> Option<(String, Type)> {
    let defs = &ctx.type_checker.type_definitions();
    let Some(TypeDefinition::Class(class_def)) = defs.get(name) else {
        return None;
    };
    if !builtin_collection_needs_its_own_body(name, class_def, method_name, resolved) {
        return None;
    }
    // An instantiated body reaches instantiations the registry was never told
    // about; the caller records the receiver's so the pipeline registers it.
    // A receiver that declares no parameters carries no instantiation to
    // register — its own bodies are lowered once, unconditionally — so only a
    // generic receiver has to be found in the registry.
    let is_recorded =
        resolved.is_empty() || is_registered_instantiation(ctx.type_checker, name, resolved);
    if !is_recorded && ctx.generic_subs.is_empty() {
        return None;
    }
    // An inherited method has no copy of its own: its body belongs to the
    // ancestor that declares it, compiled at that ancestor's type arguments.
    let (owner, owner_args) =
        crate::mir::lowering::inherited_instantiation::declaring_class_instantiation(
            defs,
            name,
            resolved,
            method_name,
        )?;
    let Some(TypeDefinition::Class(owner_def)) = defs.get(owner.as_str()) else {
        return None;
    };
    let owner_gens = owner_def.generics.as_ref()?;
    if owner_args.len() != owner_gens.len()
        || !owner_args
            .iter()
            .all(|t| is_monomorphizable_type_argument(&t.kind, defs))
    {
        return None;
    }
    let owner_subs: HashMap<String, Type> = owner_gens
        .iter()
        .zip(&owner_args)
        .map(|(g, t)| (g.name.clone(), t.clone()))
        .collect();
    let mangled = mangle_instantiation_name(&format!("{owner}_{method_name}"), &owner_args);
    let subs = instantiation_substitution(ctx.type_checker, &owner, method_name, &owner_subs);
    let return_ty = apply_generic_sub(&method_info.return_type, &subs);
    Some((mangled, return_ty))
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

#[cfg(test)]
mod mangled_token_tests {
    use super::*;
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::ast::types::FunctionTypeData;
    use crate::ast::IdNode;
    use crate::error::syntax::Span;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn ty(kind: TypeKind) -> Type {
        Type::new(kind, span())
    }

    fn type_arg(kind: TypeKind) -> Expression {
        IdNode::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
    }

    fn size_arg(value: i64) -> Expression {
        IdNode::new(
            0,
            ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I64(value))),
            span(),
        )
    }

    fn class_ref(name: &str, args: Vec<Expression>) -> TypeKind {
        TypeKind::Custom(name.to_string(), Some(args))
    }

    fn token(kind: TypeKind) -> String {
        type_kind_to_mangle_str(&kind).into_owned()
    }

    /// The size decides which body a call reaches, so two sizes of one element
    /// type must not share a symbol.
    #[test]
    fn two_array_sizes_mangle_to_two_symbols() {
        let three = class_ref("Array", vec![type_arg(TypeKind::String), size_arg(3)]);
        let four = class_ref("Array", vec![type_arg(TypeKind::String), size_arg(4)]);
        assert_eq!(token(three.clone()), "Array_String_3");
        assert_ne!(token(three), token(four));
    }

    /// A value generic reaches the mangler through the instantiation registry as
    /// a marker type; it must spell the same constant the class reference does.
    #[test]
    fn a_value_generic_marker_spells_its_constant() {
        let marker = crate::type_checker::generics::value_generic_marker_type(size_arg(3));
        assert_eq!(token(marker.kind), "3");
    }

    /// Two lists that differ only in their element are two different types, and
    /// a body compiled for one reads the other's elements at the wrong
    /// ownership.
    #[test]
    fn two_list_elements_mangle_to_two_symbols() {
        let ints = class_ref("List", vec![type_arg(TypeKind::Int)]);
        let strings = class_ref("List", vec![type_arg(TypeKind::String)]);
        assert_eq!(token(ints.clone()), "List_int");
        assert_ne!(token(ints), token(strings));
    }

    #[test]
    fn an_optional_spells_its_payload() {
        let strings = TypeKind::Option(Box::new(ty(TypeKind::String)));
        let ints = TypeKind::Option(Box::new(ty(TypeKind::Int)));
        assert_eq!(token(strings.clone()), "option_String");
        assert_ne!(token(strings), token(ints));
    }

    #[test]
    fn a_tuple_spells_every_component() {
        let pair = TypeKind::Tuple(vec![type_arg(TypeKind::Int), type_arg(TypeKind::String)]);
        assert_eq!(token(pair), "tuple_int_String");
    }

    /// A closure type has no token, and neither has anything built around one:
    /// a name assembled from the unspellable token would be one name for every
    /// such type.
    #[test]
    fn a_component_without_a_token_makes_the_whole_type_unspellable() {
        let closure = TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: Vec::new(),
            return_type: None,
        }));
        assert!(!kind_has_a_mangled_token(&closure));
        assert!(!kind_has_a_mangled_token(&class_ref(
            "List",
            vec![type_arg(closure)]
        )));
    }

    /// The verifier stays quiet about a receiver no per-instantiation body
    /// could be named for, so what it exempts is exactly what this predicate
    /// declines. A sized array and a list of structural elements are both
    /// nameable now, which is what puts them back inside the check.
    #[test]
    fn a_sized_array_and_a_structural_element_can_both_be_named() {
        assert!(crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "Array",
            vec![type_arg(TypeKind::String), size_arg(3)]
        )));
        let pair = TypeKind::Tuple(vec![type_arg(TypeKind::Int), type_arg(TypeKind::String)]);
        assert!(crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "List",
            vec![type_arg(pair)]
        )));
        assert!(crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "List",
            vec![type_arg(class_ref("List", vec![type_arg(TypeKind::Int)]))]
        )));
    }

    /// A size still written as an expression names no single instantiation, so
    /// the receiver holding it gets no per-instantiation body.
    #[test]
    fn an_unfolded_size_has_no_token() {
        let unfolded = IdNode::new(0, ExpressionKind::Identifier("N".to_string(), None), span());
        assert!(!argument_has_a_mangled_token(&unfolded));
        assert!(!crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "Array",
            vec![type_arg(TypeKind::String), unfolded]
        )));
    }
}
