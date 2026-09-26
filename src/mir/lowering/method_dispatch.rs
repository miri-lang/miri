// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method dispatch lowering — name mangling, inheritance resolution, virtual/static dispatch.

use crate::ast::expression::Expression;
use crate::ast::statement::BindingResidency;
use crate::ast::types::{FunctionTypeData, STRING_TYPE_NAME, TUPLE_TYPE_NAME};
use crate::ast::{ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::Symbol;
use crate::mir::{Local, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind};
use crate::runtime_fns::cow_fn;
use crate::type_checker::context::{class_needs_vtable, MethodInfo, TypeDefinition};

use super::class_instantiations::is_registered_instantiation;
use super::dispatch_symbols::{instantiation_substitution, trait_default_among, vtable_slot_index};
use super::{apply_generic_sub, lower_expression, LoweringContext};
use crate::ast::BuiltinCollectionKind;
use crate::ast::BuiltinCollectionKind as Collection;
use std::borrow::Cow;
use std::collections::HashMap;

/// The mangled name of one instantiation of a generic class, e.g. `Box` at
/// `[String]` → `miri.Box$String`. The parameter names play no part in the
/// symbol, so only the arguments are needed.
pub(crate) fn mangle_instantiation_name(class_name: &str, type_args: &[Type]) -> String {
    Symbol::function(class_name, type_args).link_name()
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
pub(crate) const MAX_TOKEN_DEPTH: usize = 64;

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
/// Two different types never share a token. The token is a run of segments
/// joined by `_`, and each segment says what it is by its first character:
///
/// - A segment that starts with a letter is a leaf or the head of a declared
///   generic: a primitive's spelling, or a declared type's name when that name
///   is a letter followed by letters and digits (see [`is_plain_type_name`]). A
///   declared head takes the fixed number of arguments its declaration gives it.
/// - A segment made only of digits (after an optional `-`) is a value generic's
///   constant.
/// - A segment of digits followed by letters is structure no identifier can
///   spell, since an identifier never starts with a digit. The digits count
///   what follows. `<n>tuple`, `<n>fn`, `1option`, `2result`, `1future` and
///   `1out` are built-in heads taking `n` component tokens (a closure's `n`
///   counts its parameters; its return follows them); `<n>x` is the declared
///   name of the next `n` characters, spelled that way when it is not plain.
///
/// A component that has no token of its own makes the whole type unspellable:
/// a name built from [`UNSPELLABLE_TYPE_TOKEN`] would be the same name for
/// every type that contains one.
pub(crate) fn type_kind_to_mangle_str(kind: &TypeKind) -> Cow<'static, str> {
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
        TypeKind::Custom(name, None) => return declared_name_token(name),
        TypeKind::Custom(name, Some(args)) => {
            return compound_token(declared_name_token(name), args, depth)
        }
        TypeKind::List(inner) => return collection_token(Collection::List, [&**inner], depth),
        TypeKind::Set(inner) => return collection_token(Collection::Set, [&**inner], depth),
        TypeKind::Array(inner, size) => {
            return collection_token(Collection::Array, [&**inner, &**size], depth)
        }
        TypeKind::Map(key, value) => {
            return collection_token(Collection::Map, [&**key, &**value], depth)
        }
        TypeKind::Option(inner) => {
            return join_tokens(
                built_in_head(OPTION_HEAD, 1),
                [type_kind_token(&inner.kind, depth + 1)].into_iter(),
            )
        }
        TypeKind::Tuple(elements) => {
            return compound_token(built_in_head(TUPLE_HEAD, elements.len()), elements, depth)
        }
        TypeKind::Result(ok, err) => {
            return compound_token(built_in_head(RESULT_HEAD, 2), [&**ok, &**err], depth)
        }
        TypeKind::Future(inner) => {
            return compound_token(built_in_head(FUTURE_HEAD, 1), [&**inner], depth)
        }
        TypeKind::Function(function) => return closure_token(function, depth),
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
        TypeKind::RawPtr => "RawPtr",
        TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_)
        | TypeKind::Identifier
        | TypeKind::Error => UNSPELLABLE_TYPE_TOKEN,
    };
    Cow::Borrowed(token)
}

/// The segment heading a built-in type of `arity` components: the arity, then
/// `tag`. Starting with a digit is what keeps it out of reach of any declared
/// name, and the arity is what tells a decoder where the components end.
fn built_in_head(tag: &str, arity: usize) -> Cow<'static, str> {
    Cow::Owned(format!("{arity}{tag}"))
}

/// Whether a declared type's `name` can spell itself as is: a letter followed
/// by letters and digits, and not a built-in leaf's spelling. Such a name is one
/// segment on its own and cannot be mistaken for anything a built-in spells.
fn is_plain_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric())
        && name != VOID_TOKEN
}

/// The token of a declared type's name: the name itself when it is plain (see
/// [`is_plain_type_name`]), otherwise its length, [`ESCAPED_NAME_TAG`] and the
/// name — `My_Box` is `6xMy_Box` — so an underscore inside it can never be read
/// as the boundary between two components.
fn declared_name_token(name: &str) -> Cow<'static, str> {
    if is_plain_type_name(name) {
        return Cow::Owned(name.to_string());
    }
    Cow::Owned(format!("{}{ESCAPED_NAME_TAG}{name}", name.len()))
}

/// The token for a built-in collection: its class's name followed by its
/// components, the same token a reference to that class spells.
fn collection_token<'a>(
    collection: Collection,
    args: impl IntoIterator<Item = &'a Expression>,
    depth: usize,
) -> Cow<'static, str> {
    compound_token(declared_name_token(collection.name()), args, depth)
}

/// The token for a closure type: a head carrying its arity, each parameter's
/// token — wrapped in a `1out` head for one the closure writes through — and
/// the return's (`void` when it returns nothing): `fn(x int) String` is
/// `1fn_int_String`.
///
/// A closure type declaring type parameters of its own, or taking a parameter
/// resident anywhere but the host, has no token: it crosses a call in a way no
/// instantiation of a class can hold.
fn closure_token(function: &FunctionTypeData, depth: usize) -> Cow<'static, str> {
    let on_host = |param: &crate::ast::common::Parameter| match param.residency {
        None | Some(BindingResidency::Host) => true,
        Some(BindingResidency::Gpu) => false,
    };
    if function.generics.is_some() || !function.params.iter().all(on_host) {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    let params = function.params.iter().flat_map(|param| {
        let written = expression_token(&param.typ, depth + 1);
        let marker = param.is_out.then(|| built_in_head(OUT_PARAMETER_HEAD, 1));
        marker.into_iter().chain(std::iter::once(written))
    });
    let ret = match function.return_type.as_deref() {
        Some(ret) => expression_token(ret, depth + 1),
        None => Cow::Borrowed(VOID_TOKEN),
    };
    join_tokens(
        built_in_head(CLOSURE_HEAD, function.params.len()),
        params.chain(std::iter::once(ret)),
    )
}

/// The token for a type written as `head` applied to `args`, e.g. `Map<String,
/// int>` → `Map_String_int`.
fn compound_token<'a>(
    head: Cow<'static, str>,
    args: impl IntoIterator<Item = &'a Expression>,
    depth: usize,
) -> Cow<'static, str> {
    join_tokens(
        head,
        args.into_iter().map(|arg| expression_token(arg, depth + 1)),
    )
}

/// Join `head` and each component token with `_`, collapsing to
/// [`UNSPELLABLE_TYPE_TOKEN`] as soon as a component has no token of its own.
fn join_tokens(
    head: Cow<'static, str>,
    parts: impl Iterator<Item = Cow<'static, str>>,
) -> Cow<'static, str> {
    let mut out = head.into_owned();
    for part in parts {
        if part == UNSPELLABLE_TYPE_TOKEN {
            return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
        }
        out.push('_');
        out.push_str(&part);
    }
    Cow::Owned(out)
}

/// The tag of the built-in head spelling a tuple of its components.
const TUPLE_HEAD: &str = "tuple";
/// The tag of the built-in head spelling a closure type.
const CLOSURE_HEAD: &str = "fn";
/// The tag of the built-in head spelling an optional of its payload.
const OPTION_HEAD: &str = "option";
/// The tag of the built-in head spelling a result of its two payloads.
const RESULT_HEAD: &str = "result";
/// The tag of the built-in head spelling a future of its payload.
const FUTURE_HEAD: &str = "future";
/// The tag of the built-in head wrapping a closure parameter written through.
const OUT_PARAMETER_HEAD: &str = "out";
/// The tag spelling a declared name that is not plain, after its length.
const ESCAPED_NAME_TAG: &str = "x";
/// The spelling of the empty type, the one built-in leaf spelling a declared
/// type can take for its own name — every other one is a primitive's name,
/// which resolves to the primitive before any declaration is consulted.
const VOID_TOKEN: &str = "void";

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
            Cow::Owned(value.to_string())
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
/// retarget `func_op` to the residency-specialized body, record the
/// specialization on the body for the pipeline to lower, and return the per-arg
/// device handles (positional, sized to `arg_ops`). Otherwise leaves `func_op`
/// untouched and returns an empty vector (an ordinary host call).
///
/// Only a `GpuLaunchSafe` callee is specialized: its buffer touches occur solely
/// inside `forall` (device) context, so the passed buffer is never read on the
/// host — the very property the type checker's residency gate enforces.
pub(crate) fn residency_specialize_call(
    ctx: &mut LoweringContext,
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

    let mut handles = vec![None; arg_ops.len()];
    for &(idx, handle) in &gpu_args {
        if idx < handles.len() {
            handles[idx] = Some(handle);
        }
    }

    if let Operand::Constant(constant) = &*func_op {
        if let crate::ast::literal::Literal::Identifier(_) = &constant.literal {
            let symbol =
                Symbol::function(declared_name(ctx, func_name), &[]).with_residency(&gpu_args);
            *func_op = super::dispatch::runtime_fn_operand(&symbol.link_name(), func.span);
            ctx.body
                .residency_function_calls
                .push(crate::mir::body::ResidencyFunctionCall {
                    symbol,
                    function: func_name.clone(),
                    arg_handles: handles.clone(),
                });
        }
    }
    handles
}

/// The name the function a call writes as `name` is declared under: the
/// original name when `name` is an import alias.
fn declared_name<'n>(ctx: &'n LoweringContext, name: &'n str) -> &'n str {
    ctx.type_checker
        .global_scope()
        .get(name)
        .and_then(|info| info.original_name.as_deref())
        .unwrap_or(name)
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
        None => Symbol::method(m.defining_class, &[], m.method_name, &[]).link_name(),
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
        None => (
            Symbol::method(owner, &[], method_name, &[]).link_name(),
            method.return_type.clone(),
        ),
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

/// The per-instantiation body a call to one method on an instance of a class
/// at concrete type arguments reaches.
pub(crate) struct InstantiatedCallee {
    /// The class compiling the body: the receiver's, or the ancestor it
    /// inherits the method from.
    pub(crate) owner: String,
    /// The owner's parameters at the arguments the receiver reaches it at.
    pub(crate) owner_subs: HashMap<String, Type>,
    /// The link name of the body, `miri.{owner}${args}.{method}`.
    pub(crate) symbol: String,
}

/// The per-instantiation body `method_name` resolves to on `name` instantiated
/// at `resolved` — the one a static call and a vtable slot both name — or
/// `None` when the shared generic body applies: the method belongs to a class
/// declaring no parameters, the owner's arguments have no monomorphized
/// spelling, or a built-in collection's shared body serves. An inherited
/// method's body belongs to the ancestor declaring it, at its own arguments.
pub(crate) fn instantiated_callee(
    defs: &HashMap<String, TypeDefinition>,
    name: &str,
    resolved: &[Type],
    method_name: &str,
) -> Option<InstantiatedCallee> {
    let Some(TypeDefinition::Class(class_def)) = defs.get(name) else {
        return None;
    };
    if !builtin_collection_needs_its_own_body(name, class_def, method_name, resolved) {
        return None;
    }
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
    if !super::is_monomorphized_instantiation(&owner_args, owner_gens.len(), defs) {
        return None;
    }
    let symbol = Symbol::method(&owner, &owner_args, method_name, &[]).link_name();
    let owner_subs = owner_gens
        .iter()
        .zip(owner_args)
        .map(|(g, t)| (g.name.clone(), t))
        .collect();
    Some(InstantiatedCallee {
        owner,
        owner_subs,
        symbol,
    })
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
    let resolved = super::monomorphized_arguments(arg_exprs.as_ref()?, gens.len(), defs)?;
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
    let callee = instantiated_callee(
        ctx.type_checker.type_definitions(),
        name,
        resolved,
        method_name,
    )?;
    let subs = instantiation_substitution(
        ctx.type_checker,
        &callee.owner,
        method_name,
        &callee.owner_subs,
    );
    let return_ty = apply_generic_sub(&method_info.return_type, &subs);
    Some((callee.symbol, return_ty))
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
        assert_eq!(token(strings.clone()), "1option_String");
        assert_ne!(token(strings), token(ints));
    }

    #[test]
    fn a_tuple_spells_every_component() {
        let pair = TypeKind::Tuple(vec![type_arg(TypeKind::Int), type_arg(TypeKind::String)]);
        assert_eq!(token(pair), "2tuple_int_String");
    }

    fn closure(params: Vec<TypeKind>, ret: Option<TypeKind>) -> TypeKind {
        let params = params
            .into_iter()
            .enumerate()
            .map(|(index, kind)| crate::ast::common::Parameter {
                name: format!("p{index}"),
                name_span: span(),
                typ: Box::new(type_arg(kind)),
                guard: None,
                default_value: None,
                is_out: false,
                residency: None,
            })
            .collect();
        TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params,
            return_type: ret.map(|kind| Box::new(type_arg(kind))),
        }))
    }

    /// A closure type spells its arity, each parameter and its return, so two
    /// closure shapes never share a body.
    #[test]
    fn a_closure_type_spells_its_arity_parameters_and_return() {
        let thunk = closure(Vec::new(), Some(TypeKind::String));
        let step = closure(vec![TypeKind::Int], Some(TypeKind::Int));
        let action = closure(vec![TypeKind::Int], None);
        assert_eq!(token(thunk), "0fn_String");
        assert_eq!(token(step.clone()), "1fn_int_int");
        assert_eq!(token(action.clone()), "1fn_int_void");
        assert_ne!(token(step), token(action));
    }

    /// A closure taking an `out` parameter crosses the call by address, so it
    /// never shares a token with one taking the same type by value.
    #[test]
    fn a_closure_with_an_out_parameter_spells_it() {
        let reads = closure(vec![TypeKind::Int], None);
        let mut writes = reads.clone();
        if let TypeKind::Function(function) = &mut writes {
            function.params[0].is_out = true;
        }
        assert_eq!(token(writes.clone()), "1fn_1out_int_void");
        assert_ne!(token(writes), token(reads));
    }

    /// A closure declaring type parameters of its own names no single
    /// instantiation.
    #[test]
    fn a_closure_with_type_parameters_has_no_token() {
        let mut generic = closure(vec![TypeKind::Int], None);
        if let TypeKind::Function(function) = &mut generic {
            function.generics = Some(Vec::new());
        }
        assert!(!kind_has_a_mangled_token(&generic));
    }

    #[test]
    fn a_result_spells_both_payloads() {
        let result = TypeKind::Result(
            Box::new(type_arg(TypeKind::String)),
            Box::new(type_arg(TypeKind::Int)),
        );
        assert_eq!(token(result), "2result_String_int");
    }

    #[test]
    fn a_future_spells_its_payload() {
        let future = TypeKind::Future(Box::new(type_arg(TypeKind::String)));
        assert_eq!(token(future), "1future_String");
    }

    fn named(name: &str) -> TypeKind {
        TypeKind::Custom(name.to_string(), None)
    }

    fn tuple(elements: Vec<TypeKind>) -> TypeKind {
        TypeKind::Tuple(elements.into_iter().map(type_arg).collect())
    }

    fn with_out_parameter(mut closure_type: TypeKind) -> TypeKind {
        if let TypeKind::Function(function) = &mut closure_type {
            function.params[0].is_out = true;
        }
        closure_type
    }

    fn int_pair() -> TypeKind {
        tuple(vec![TypeKind::Int, TypeKind::Int])
    }

    fn step() -> TypeKind {
        closure(vec![TypeKind::Int], Some(TypeKind::Int))
    }

    /// Pairs of a declared type and a built-in one a token writer spelling
    /// built-in heads the way an identifier can would spell alike.
    fn declared_name_pairs() -> Vec<(&'static str, TypeKind, TypeKind)> {
        vec![
            (
                "a declared generic named like a closure head",
                class_ref(
                    "fn1",
                    vec![type_arg(TypeKind::Int), type_arg(TypeKind::Int)],
                ),
                step(),
            ),
            (
                "a declared generic named like a tuple head",
                class_ref(
                    "tuple",
                    vec![type_arg(TypeKind::Int), type_arg(TypeKind::Int)],
                ),
                int_pair(),
            ),
            (
                "a declared generic named like an optional head",
                class_ref("option", vec![type_arg(TypeKind::Int)]),
                TypeKind::Option(Box::new(ty(TypeKind::Int))),
            ),
            (
                "a declared generic named like a result head",
                class_ref(
                    "result",
                    vec![type_arg(TypeKind::Int), type_arg(TypeKind::Int)],
                ),
                TypeKind::Result(
                    Box::new(type_arg(TypeKind::Int)),
                    Box::new(type_arg(TypeKind::Int)),
                ),
            ),
            (
                "a declared generic named like a future head",
                class_ref("future", vec![type_arg(TypeKind::Int)]),
                TypeKind::Future(Box::new(type_arg(TypeKind::Int))),
            ),
            (
                "a declared type named like the empty type",
                named("void"),
                TypeKind::Void,
            ),
            (
                "a declared type named like the unspellable token",
                named(UNSPELLABLE_TYPE_TOKEN),
                TypeKind::Generic(
                    "T".to_string(),
                    None,
                    crate::ast::types::TypeDeclarationKind::None,
                ),
            ),
            (
                "a declared name holding an underscore",
                named("Box_int"),
                class_ref("Box", vec![type_arg(TypeKind::Int)]),
            ),
        ]
    }

    /// Pairs of built-in types a token writer leaving an arity implicit would
    /// spell alike.
    fn structural_pairs() -> Vec<(&'static str, TypeKind, TypeKind)> {
        vec![
            (
                "nested tuples of equal flattened leaves",
                tuple(vec![int_pair(), TypeKind::String, TypeKind::Int]),
                tuple(vec![
                    tuple(vec![TypeKind::Int, TypeKind::Int, TypeKind::String]),
                    TypeKind::Int,
                ]),
            ),
            (
                "a tuple nested first or last",
                tuple(vec![int_pair(), TypeKind::Int]),
                tuple(vec![TypeKind::Int, int_pair()]),
            ),
            (
                "closures of one arity that return a tuple or take one",
                closure(vec![int_pair()], Some(TypeKind::Int)),
                closure(vec![TypeKind::Int], Some(int_pair())),
            ),
            (
                "a closure taking an out parameter or not",
                with_out_parameter(step()),
                step(),
            ),
            (
                "an out parameter or a declared type named like its head",
                with_out_parameter(step()),
                closure(vec![named("out"), TypeKind::Int], Some(TypeKind::Int)),
            ),
        ]
    }

    /// Two different types never share a token, whatever a declaration names
    /// itself and however the types nest.
    #[test]
    fn distinct_types_spell_distinct_tokens() {
        for (case, left, right) in declared_name_pairs().into_iter().chain(structural_pairs()) {
            let (left, right) = (token(left), token(right));
            assert_ne!(left, right, "{case}: both spell `{left}`");
        }
    }

    /// A plain declared name spells itself; one an identifier-shaped built-in
    /// could collide with carries its length.
    #[test]
    fn a_declared_name_that_is_not_plain_carries_its_length() {
        assert_eq!(token(named("Point")), "Point");
        assert_eq!(token(named("Box_int")), "7xBox_int");
        assert_eq!(token(named("void")), "4xvoid");
    }

    /// A value argument past `i128` spells the value written, not the one it
    /// wraps to.
    #[test]
    fn a_value_past_i128_spells_the_value_written() {
        let big = IdNode::new(
            0,
            ExpressionKind::Literal(Literal::Integer(IntegerLiteral::U128(1 << 127))),
            span(),
        );
        assert_eq!(
            expression_mangle_token(&big),
            "170141183460469231731687303715884105728"
        );
    }

    /// A component with no token makes the whole type unspellable: a name
    /// assembled from the unspellable token would be one name for every such
    /// type.
    #[test]
    fn a_component_without_a_token_makes_the_whole_type_unspellable() {
        let open = TypeKind::Generic(
            "T".to_string(),
            None,
            crate::ast::types::TypeDeclarationKind::None,
        );
        assert!(!kind_has_a_mangled_token(&open));
        assert!(!kind_has_a_mangled_token(&class_ref(
            "List",
            vec![type_arg(open)]
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
