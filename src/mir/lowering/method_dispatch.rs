// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Method dispatch lowering — inheritance resolution, virtual/static dispatch.

use crate::ast::expression::Expression;
use crate::ast::types::{STRING_TYPE_NAME, TUPLE_TYPE_NAME};
use crate::ast::{ExpressionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::representation::{written_type_arguments, Representation};
use crate::mir::symbol::Symbol;
use crate::mir::{Local, Operand, Place, Rvalue, StatementKind, Terminator, TerminatorKind};
use crate::runtime_fns::{cow_fn, rt};
use crate::type_checker::context::{
    class_needs_vtable, resolve_method_source, trait_lineage, MethodInfo, MethodSource,
    TypeDefinition,
};
use crate::type_checker::utils::tuple_class_arguments;
use crate::type_checker::value_class::LayoutScope;

use super::class_instantiations::is_registered_instantiation;
use super::dispatch_symbols::{instantiation_substitution, vtable_slot_index};
use super::expression::unshare::{lent_parameter_write, TemporaryRoot};
use super::written_receiver::{lower_pending_receiver, Receiver};
use super::{apply_generic_sub, lower_expression, LoweringContext};
use crate::ast::BuiltinCollectionKind;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

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
///
/// A generic callee is specialized at the instantiation the call `call_expr_id`
/// reaches: the specialization is named by its type arguments and lowered with
/// them, so each instantiation's kernels are compiled at its own types.
pub(crate) fn residency_specialize_call(
    ctx: &mut LoweringContext,
    call_expr_id: usize,
    func: &Expression,
    args: &[Expression],
    func_op: &mut Operand,
    arg_ops: &[Operand],
) -> Result<Vec<Option<crate::mir::body::DeviceHandleId>>, LoweringError> {
    let ExpressionKind::Identifier(func_name, _) = &func.node else {
        return Ok(Vec::new());
    };
    if !matches!(
        ctx.type_checker.callee_residency(func),
        Some(crate::type_checker::FnResidency::GpuLaunchSafe)
    ) {
        return Ok(Vec::new());
    }
    let gpu_args = gpu_resident_call_args(ctx, args);
    if gpu_args.is_empty() {
        return Ok(Vec::new());
    }

    let mut handles = vec![None; arg_ops.len()];
    for &(idx, handle) in &gpu_args {
        if idx < handles.len() {
            handles[idx] = Some(handle);
        }
    }

    if !matches!(func_op, Operand::Function(_)) {
        return Ok(handles);
    }
    let function = ctx.declared_callee(func, func_name)?.clone();
    let type_args = ctx
        .instantiated_call_mapping(call_expr_id)
        .unwrap_or_default();
    let symbol = Symbol::function(
        &function.module,
        &function.name,
        type_args.iter().map(|(_, ty)| ty),
    )
    .with_residency(&gpu_args);
    *func_op = Operand::function(symbol.clone(), func.span);
    ctx.body
        .residency_function_calls
        .push(crate::mir::body::ResidencyFunctionCall {
            symbol,
            function,
            type_args,
            arg_handles: handles.clone(),
        });
    Ok(handles)
}

/// The class or trait whose body a call to `method_name` on a `class_name`
/// receiver names, in the order [`resolve_method_source`] states, with a clone
/// of its [`MethodInfo`] so the caller can mangle the symbol correctly.
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
/// - Default trait methods: when no class in the chain gives the method a body,
///   the default a trait the chain implements supplies.
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

    let caller_is_abstract = is_abstract_class(type_defs, class_name);
    resolve_via_class_chain(type_defs, class_name, method_name, caller_is_abstract)
}

/// The owner a static call names for the body [`resolve_method_source`]
/// resolves `method_name` to on a `class_name` receiver.
///
/// A concrete caller reaching a method an abstract class declares names its
/// own copy; so does one inheriting a trait default. An abstract caller names
/// the declaring class, or the trait that supplies the default.
fn resolve_via_class_chain(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
    caller_is_abstract: bool,
) -> Option<(String, MethodInfo)> {
    let source = resolve_method_source(type_defs, class_name, method_name)?;
    let owner = match source {
        MethodSource::Declared { class, .. } | MethodSource::AbstractOnly { class, .. } => {
            if is_abstract_class(type_defs, class) && !caller_is_abstract {
                class_name
            } else {
                class
            }
        }
        MethodSource::Default(default) if caller_is_abstract => default.trait_name,
        MethodSource::Default(_) => class_name,
    };
    Some((owner.to_string(), source.info().clone()))
}

/// Whether `name` registers an abstract class.
fn is_abstract_class(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    name: &str,
) -> bool {
    matches!(type_defs.get(name), Some(TypeDefinition::Class(class)) if class.is_abstract)
}

/// The nearest trait in `trait_name`'s hierarchy, in [`trait_lineage`] order,
/// that declares `method_name` — abstract or with a default — with a clone of
/// its signature.
fn resolve_in_trait_hierarchy(
    type_defs: &std::collections::HashMap<String, TypeDefinition>,
    trait_name: &str,
    method_name: &str,
) -> Option<(String, MethodInfo)> {
    trait_lineage(type_defs, trait_name).find_map(|(name, trait_def)| {
        trait_def
            .methods
            .get(method_name)
            .map(|method| (name.to_string(), method.clone()))
    })
}

/// Emit a virtual method call through a vtable slot.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_virtual_method_call(
    ctx: &mut LoweringContext,
    vtable_slot: usize,
    self_op: Receiver<'_>,
    user_args: &[Expression],
    method_info: &MethodInfo,
    arg_types: &[Type],
    destination: &Place,
    op: &Operand,
    obj_temp_local: Option<Local>,
    obj_watermark: usize,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let arg_watermark = ctx.body.local_decls.len();
    let call_args = method_call_args(ctx, self_op, user_args, method_info, arg_types)?;

    let arg_modes =
        super::dispatch::build_method_arg_modes(method_info, user_args.len(), call_args.len());
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::VirtualCall {
            vtable_slot,
            args: call_args.clone(),
            arg_modes,
            param_types: arg_types
                .iter()
                .map(|ty| names_a_settled_type(ctx, ty).then(|| ty.clone()))
                .collect(),
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

/// The operands a method call passes: its receiver, its arguments and the
/// allocator. The arguments are lowered first and the receiver made the
/// call's own after them, so no argument runs between the receiver's check
/// and the call.
fn method_call_args(
    ctx: &mut LoweringContext,
    receiver: Receiver<'_>,
    user_args: &[Expression],
    method_info: &MethodInfo,
    arg_types: &[Type],
) -> Result<Vec<Operand>, LoweringError> {
    let arguments = lower_method_args(ctx, user_args, method_info, arg_types)?;
    let mut call_args = vec![receiver.make_own(ctx)?];
    call_args.extend(arguments);
    if let Some(&alloc_local) = ctx.variable_map.get("allocator") {
        call_args.push(Operand::Copy(Place::new(alloc_local)));
    }
    Ok(call_args)
}

/// Emit a static method call (direct function call).
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_static_method_call(
    ctx: &mut LoweringContext,
    symbol: Symbol,
    self_op: Receiver<'_>,
    user_args: &[Expression],
    method_info: &MethodInfo,
    arg_types: &[Type],
    destination: &Place,
    op: &Operand,
    obj_temp_local: Option<Local>,
    obj_watermark: usize,
    span: Span,
) -> Result<Option<Operand>, LoweringError> {
    let arg_watermark = ctx.body.local_decls.len();
    let call_args = method_call_args(ctx, self_op, user_args, method_info, arg_types)?;

    let func_op = Operand::function(symbol, span);

    let arg_modes =
        super::dispatch::build_method_arg_modes(method_info, user_args.len(), call_args.len());
    let target_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: func_op,
            args: call_args.clone(),
            arg_modes,
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

/// The trait a receiver typed as a bounded type parameter is called through.
///
/// A body still written at its open parameter — `x.close()` for
/// `x T` where `T extends Closable` — knows only what the bound promises, so
/// the call dispatches through the trait's vtable exactly as one through a
/// binding of the trait's type would. A body instantiated at a concrete type
/// has its parameter substituted and never reaches this.
fn bounding_trait(ctx: &LoweringContext, obj_ty: &Type) -> Option<Type> {
    let constraint = match &obj_ty.kind {
        TypeKind::Generic(_, Some(constraint), _) => (**constraint).clone(),
        TypeKind::Custom(name, None) => enclosing_parameter_bound(ctx, name)?,
        _ => return None,
    };
    let TypeKind::Custom(trait_name, _) = &constraint.kind else {
        return None;
    };
    matches!(
        ctx.type_checker.type_definitions().get(trait_name.as_str()),
        Some(TypeDefinition::Trait(_))
    )
    .then_some(constraint)
}

/// The bound the enclosing class declares for its type parameter `name`, or
/// `None` when `name` is none of its parameters or carries no bound.
fn enclosing_parameter_bound(ctx: &LoweringContext, name: &str) -> Option<Type> {
    let self_type = ctx.self_type.as_ref()?;
    let TypeKind::Custom(class_name, _) = &self_type.kind else {
        return None;
    };
    let Some(TypeDefinition::Class(class_def)) =
        ctx.type_checker.type_definitions().get(class_name.as_str())
    else {
        return None;
    };
    class_def
        .generics
        .as_ref()?
        .iter()
        .find(|generic| generic.name == name)?
        .constraint
        .clone()
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
    let obj_ty = bounding_trait(ctx, &obj_ty).unwrap_or(obj_ty);
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
        ctx.record_class_instantiations(&receiver_class_type(m.obj_ty));
    }
    let return_ty = call_result_type(ctx, &m, &mono);
    let arg_types = method_argument_types(ctx, &m);
    let obj_watermark = ctx.body.local_decls.len();
    let (self_op, obj_temp_local) =
        prepare_method_self(ctx, m.obj, m.obj_ty, (m.class_name, m.method_name), *m.span)?;
    let (destination, op) = super::dispatch::call_destination(ctx, return_ty, dest, *m.span);
    emit_method_call(
        ctx,
        &m,
        mono,
        (self_op, obj_temp_local, obj_watermark),
        (&destination, &op, &arg_types),
    )
}

/// Emit the call `m` resolves to — through its vtable slot when it is
/// dispatched, otherwise to `mono` or the method's shared body — on the
/// receiver `self_op`.
fn emit_method_call(
    ctx: &mut LoweringContext,
    m: &ResolvedMethod,
    mono: Option<(Symbol, Type)>,
    (self_op, obj_temp_local, obj_watermark): (Receiver<'_>, Option<Local>, usize),
    (destination, op, arg_types): (&Place, &Operand, &[Type]),
) -> Result<Option<Operand>, LoweringError> {
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
                arg_types,
                destination,
                op,
                obj_temp_local,
                obj_watermark,
                *m.span,
            );
        }
    }
    let symbol = match mono {
        Some((mangled, _)) => mangled,
        None => Symbol::method(m.defining_class, &[], m.method_name, &[]),
    };
    emit_static_method_call(
        ctx,
        symbol,
        self_op,
        m.args,
        m.method_info,
        arg_types,
        destination,
        op,
        obj_temp_local,
        obj_watermark,
        *m.span,
    )
}

/// The type each parameter of the method `m` calls is passed at: its declared
/// type with `Self` read as the receiver and the owner's parameters at the
/// arguments the receiver reaches it at.
fn method_argument_types(ctx: &LoweringContext, m: &ResolvedMethod) -> Vec<Type> {
    let defs = ctx.type_checker.type_definitions();
    let (owner, owner_subs) = receiver_instantiation(ctx, m.obj_ty)
        .and_then(|(name, resolved)| instantiated_callee(defs, &name, &resolved, m.method_name))
        .map(|callee| (callee.owner, callee.owner_subs))
        .unwrap_or_else(|| {
            let subs = trait_receiver_substitution(ctx, m.obj_ty, m.defining_class);
            (m.defining_class.to_string(), subs)
        });
    // The body's own substitution: a trait default reads the trait's
    // parameters at what the class's clauses pin, not at a class parameter
    // that happens to share a name.
    let body_subs =
        instantiation_substitution(ctx.type_checker, &owner, m.method_name, &owner_subs);
    m.method_info
        .params
        .iter()
        .map(|(_, declared)| {
            let at_receiver = super::substitute_self_type(declared, m.obj_ty);
            let instantiated = apply_generic_sub(&at_receiver, &body_subs);
            super::variable::canonical_declared_type(ctx.type_checker, &instantiated)
        })
        .collect()
}

/// The arguments a call through a trait-typed receiver binds the trait's own
/// parameters to, when the trait named `defining` declares the method: a
/// `Sink<int>` receiver reads `put(x T?)` as `put(x int?)`. Empty for any
/// other receiver.
fn trait_receiver_substitution(
    ctx: &LoweringContext,
    receiver: &Type,
    defining: &str,
) -> HashMap<String, Type> {
    let TypeKind::Custom(name, Some(args)) = &receiver.kind else {
        return HashMap::new();
    };
    let Some(TypeDefinition::Trait(trait_def)) = ctx.type_checker.type_definitions().get(name)
    else {
        return HashMap::new();
    };
    if name != defining {
        return HashMap::new();
    }
    trait_def
        .generics
        .iter()
        .flatten()
        .zip(args)
        .filter_map(|(generic, arg)| {
            Some((
                generic.name.clone(),
                crate::mir::instantiation::type_argument(arg)?,
            ))
        })
        .collect()
}

/// Lower a method call's arguments, each brought to the type its parameter is
/// passed at — a plain value handed to an optional parameter is wrapped, as a
/// function call's is. An `out` argument is the caller's own place and is
/// passed as it is, and so is one whose parameter type still names a type
/// parameter nothing here binds.
fn lower_method_args(
    ctx: &mut LoweringContext,
    args: &[Expression],
    method_info: &MethodInfo,
    arg_types: &[Type],
) -> Result<Vec<Operand>, LoweringError> {
    let names: Vec<&str> = method_info
        .params
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let mut slots = Vec::with_capacity(args.len());
    for (position, value) in super::dispatch::bind_arguments_to_parameters(args, &names) {
        let watermark = ctx.body.local_decls.len();
        let op = lower_expression(ctx, value, None)?;
        let target = arg_types.get(position).filter(|target| {
            !method_info.is_param_out(position) && names_a_settled_type(ctx, target)
        });
        let op = match target {
            Some(target) => {
                super::dispatch::coerce_arg_to_declared(ctx, op, value, target, watermark)
            }
            None => op,
        };
        super::dispatch::place_at_parameter(&mut slots, position, op);
    }
    let span = args.first().map(|arg| arg.span).unwrap_or_default();
    super::dispatch::operands_in_parameter_order(slots, span)
}

/// Whether `ty` names a type every part of which is known here: no type
/// parameter left unbound, and no name the program does not declare.
fn names_a_settled_type(ctx: &LoweringContext, ty: &Type) -> bool {
    let definitions = ctx.type_checker.type_definitions();
    !crate::type_checker::instantiation_requirements::spells_a_type(&ty.kind, &|kind| {
        matches!(kind, TypeKind::Generic(..) | TypeKind::Error)
            || matches!(kind, TypeKind::Custom(name, _) if !definitions.contains_key(name))
    })
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
    mono: &Option<(Symbol, Type)>,
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

/// Whether a built-in collection's `method_name` has a body the runtime settles,
/// which makes its shared generic body correct at every element type.
///
/// The type checker records the set on the class; the rule is stated once, in
/// [`crate::type_checker::runtime_settled`]. A settled body only hands elements
/// between the caller and the runtime — `push` gives its element to a store
/// that keeps the caller's reference — so lowering it once leaves every
/// reference where the runtime expects it. A method the collection only
/// inherits is never in the set.
pub(crate) fn is_settled_by_the_runtime(
    class_def: &crate::type_checker::context::ClassDefinition,
    method_name: &str,
) -> bool {
    class_def.runtime_settled_methods.contains(method_name)
}

/// Whether a built-in collection instantiated at `resolved` needs a
/// per-instantiation body for `method_name`, rather than the shared generic one.
///
/// The shared body is compiled once, with every type parameter open: each is
/// one uncounted word. An instantiation may share it only when every type the
/// body handles is represented at the instantiation's arguments as it is at
/// that fallback; see [`SharedBody`]. Apart from representation, an argument
/// ordered unlike a signed integer would sort wrongly in any list the body
/// builds.
///
/// Every other class always needs one, so it passes straight through.
fn builtin_collection_needs_its_own_body(
    class_name: &str,
    class_def: &crate::type_checker::context::ClassDefinition,
    method_name: &str,
    resolved: &[Type],
    defs: &HashMap<String, TypeDefinition>,
) -> bool {
    if BuiltinCollectionKind::from_name(class_name).is_none() {
        return true;
    }
    let arguments = ClassArguments::of(class_def, resolved);
    if arguments
        .substitution
        .values()
        .any(|arg| orders_differently_from_signed_fallback(&arg.kind))
    {
        return true;
    }
    let shared = SharedBody {
        fallback: LayoutScope::new(defs, &arguments.parameters),
        program: LayoutScope::new(defs, &arguments.values),
        substitution: &arguments.substitution,
        is_settled: is_settled_by_the_runtime(class_def, method_name),
    };
    let signature =
        crate::type_checker::context::resolve_method_source(defs, class_name, method_name)
            .map(|source| source.info());
    shared.represents_an_argument_differently()
        || signature.is_some_and(|method| shared.represents_signature_differently(method))
}

/// A class's type parameters at the arguments it is instantiated with.
struct ClassArguments {
    /// Each type parameter's argument, by the parameter's name.
    substitution: HashMap<String, Type>,
    /// Every parameter's name: what the shared body leaves open.
    parameters: HashSet<String>,
    /// The value parameters' names. A value generic occupies a parameter slot
    /// but describes no storage — an `Array`'s size changes neither the width
    /// of a type-parameter position nor what anyone releases — so it stays as
    /// written on both sides of the comparison.
    values: HashSet<String>,
}

impl ClassArguments {
    fn of(class_def: &crate::type_checker::context::ClassDefinition, resolved: &[Type]) -> Self {
        let mut arguments = Self {
            substitution: HashMap::new(),
            parameters: HashSet::new(),
            values: HashSet::new(),
        };
        for (generic, arg) in class_def.generics.iter().flatten().zip(resolved) {
            arguments.parameters.insert(generic.name.clone());
            if crate::type_checker::generics::extract_value_generic(arg).is_some() {
                arguments.values.insert(generic.name.clone());
            } else {
                arguments
                    .substitution
                    .insert(generic.name.clone(), arg.clone());
            }
        }
        arguments
    }
}

/// A built-in collection's shared body, set beside one instantiation of it.
///
/// Each type the body handles is compared at the top level: with the class's
/// parameters open, as the shared body reads it, and at the instantiation's
/// arguments, as the caller does. A type written inside another — the element
/// of a `List<T>` the body returns — is compared on its own, so a parameter's
/// argument is only ever compared with the word the body reads it as, never
/// with the types written inside the argument. A body the runtime settles
/// only hands values between the caller and the runtime, so reference
/// counting is left out of the comparison for it.
struct SharedBody<'a> {
    fallback: LayoutScope<'a>,
    program: LayoutScope<'a>,
    substitution: &'a HashMap<String, Type>,
    is_settled: bool,
}

impl SharedBody<'_> {
    /// Whether some parameter's argument is represented unlike the parameter,
    /// bare, optional or doubly optional: what a shared body may hold,
    /// whatever its signature spells. A body the runtime settles holds no
    /// value built from a parameter (see
    /// [`crate::type_checker::runtime_settled`]), so only the bare parameter
    /// is compared for it.
    fn represents_an_argument_differently(&self) -> bool {
        let probes = if self.is_settled { 1 } else { 3 };
        self.substitution.iter().any(|(name, arg)| {
            let mut open = Type::new(TypeKind::Custom(name.clone(), None), Span::default());
            let mut bound = arg.clone();
            for _ in 0..probes {
                if self.differ(&open.kind, &bound.kind) {
                    return true;
                }
                open = optional_of(open);
                bound = optional_of(bound);
            }
            false
        })
    }

    /// Whether a type `method`'s signature names, or a type written inside
    /// one, is represented differently at the instantiation.
    fn represents_signature_differently(&self, method: &MethodInfo) -> bool {
        method
            .params
            .iter()
            .map(|(_, ty)| ty)
            .chain(std::iter::once(&method.return_type))
            .any(|ty| self.represents_type_differently(ty))
    }

    /// Whether `open`, or a type written inside it, is represented
    /// differently with the parameters bound. A type that names no parameter
    /// reads alike on both sides and is not compared.
    fn represents_type_differently(&self, open: &Type) -> bool {
        self.mentions_parameter(&open.kind)
            && (self.differ(&open.kind, &apply_generic_sub(open, self.substitution).kind)
                || written_type_arguments(&open.kind)
                    .any(|part| self.represents_type_differently(part)))
    }

    /// Whether `kind` names a parameter the instantiation binds.
    fn mentions_parameter(&self, kind: &TypeKind) -> bool {
        crate::type_checker::generics::generic_parameter_name(kind)
            .is_some_and(|name| self.substitution.contains_key(name))
            || written_type_arguments(kind).any(|part| self.mentions_parameter(&part.kind))
    }

    /// Whether `open`, read with the parameters open, and `bound`, read at the
    /// instantiation, are represented differently.
    fn differ(&self, open: &TypeKind, bound: &TypeKind) -> bool {
        let shared = Representation::of(open, self.fallback);
        let own = Representation::of(bound, self.program);
        if self.is_settled {
            shared.without_counting() != own.without_counting()
        } else {
            shared != own
        }
    }
}

/// The optional of `ty`.
fn optional_of(ty: Type) -> Type {
    Type::new(TypeKind::Option(Box::new(ty)), Span::default())
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
) -> (Symbol, Type) {
    match resolve_generic_class_monomorph(ctx, receiver_ty, method_name, method) {
        Some(callee) => {
            ctx.record_class_instantiations(receiver_ty);
            callee
        }
        None => (
            Symbol::method(owner, &[], method_name, &[]),
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
) -> Option<(Symbol, Type)> {
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
    /// The body's symbol, linked as `miri.{owner}${args}.{method}`.
    pub(crate) symbol: Symbol,
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
    if let Some(TypeDefinition::Enum(enum_def)) = defs.get(name) {
        return instantiated_enum_callee(defs, name, enum_def, resolved, method_name);
    }
    let Some(TypeDefinition::Class(class_def)) = defs.get(name) else {
        return None;
    };
    // The arguments are the call's resolved ones, read against the program's
    // table: no body's open parameters are known where a callee is named.
    if !builtin_collection_needs_its_own_body(name, class_def, method_name, resolved, defs) {
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
    let symbol = Symbol::method(&owner, &owner_args, method_name, &[]);
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

/// The per-instantiation body of `method_name` on the generic enum `name` at
/// `resolved`, or `None` when the enum declares no parameters or `resolved`
/// has no monomorphized spelling.
///
/// An enum has no ancestors, so the body is its own method compiled at its
/// own arguments: a shared body reads a managed `T` as an unmanaged word and a
/// narrow scalar at the wrong width.
fn instantiated_enum_callee(
    defs: &HashMap<String, TypeDefinition>,
    name: &str,
    enum_def: &crate::type_checker::context::EnumDefinition,
    resolved: &[Type],
    method_name: &str,
) -> Option<InstantiatedCallee> {
    let gens = enum_def.generics.as_ref()?;
    if !super::is_monomorphized_instantiation(resolved, gens.len(), defs) {
        return None;
    }
    Some(InstantiatedCallee {
        owner: name.to_string(),
        owner_subs: gens
            .iter()
            .zip(resolved)
            .map(|(g, t)| (g.name.clone(), t.clone()))
            .collect(),
        symbol: Symbol::method(name, resolved, method_name, &[]),
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
) -> Option<(Symbol, Type)> {
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
    let obj_ty = receiver_class_type(obj_ty);
    let TypeKind::Custom(name, arg_exprs) = &obj_ty.kind else {
        return None;
    };
    let defs = &ctx.type_checker.type_definitions();
    let definition = defs.get(name.as_str())?;
    if !matches!(
        definition,
        TypeDefinition::Class(_) | TypeDefinition::Enum(_)
    ) {
        return None;
    }
    let Some(gens) = definition.generics() else {
        return Some((name.clone(), Vec::new()));
    };
    let resolved = super::monomorphized_arguments(arg_exprs.as_ref()?, gens.len(), defs)?;
    Some((name.clone(), resolved))
}

/// The class type a method call on `obj_ty` is dispatched at.
///
/// A tuple keeps its per-element type, while the type checker gives a tuple
/// whose elements share one type the methods of the tuple class at that type
/// and its length. Every other receiver is its own class type.
fn receiver_class_type(obj_ty: &Type) -> Cow<'_, Type> {
    let TypeKind::Tuple(elements) = &obj_ty.kind else {
        return Cow::Borrowed(obj_ty);
    };
    let args = elements
        .first()
        .map(|first| tuple_class_arguments(first.clone(), elements.len()).to_vec());
    Cow::Owned(Type::new(
        TypeKind::Custom(TUPLE_TYPE_NAME.to_string(), args),
        obj_ty.span,
    ))
}

/// Resolve the monomorphized symbol and return type for `method_name` called on
/// `name` instantiated at `resolved`.
fn monomorph_for_instantiation(
    ctx: &LoweringContext,
    name: &str,
    resolved: &[Type],
    method_name: &str,
    method_info: &MethodInfo,
) -> Option<(Symbol, Type)> {
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

/// The copy-on-write entry point a call of `method_name` of `class_name` on a
/// receiver of type `obj_ty` goes through first, when the call writes a
/// built-in collection receiver; `None` when it writes none.
///
/// TODO: a call through a trait or a type parameter is decided by the static
/// receiver type, which is not a built-in collection, so it gets no
/// copy-on-write here. No trait a collection implements declares a method
/// that writes its receiver today; once one does, a collection reaching it
/// shared stops the program at the method's receiver check (`MER_RT_017`)
/// rather than being unshared by the caller.
fn call_writes_receiver(
    ctx: &LoweringContext,
    obj_ty: &Type,
    class_name: &str,
    method_name: &str,
) -> Option<&'static str> {
    let writes_receiver =
        (ctx.type_checker.receiver_writes).writes_receiver(class_name, method_name);
    obj_ty
        .kind
        .as_builtin_collection()
        .filter(|_| writes_receiver)
        .and_then(cow_fn)
}

/// Whether calling `method_expr` on `obj` writes the collection `obj` names,
/// as [`prepare_method_self`] decides it for the call.
pub(crate) fn is_receiver_written(
    ctx: &LoweringContext,
    obj: &Expression,
    method_expr: &Expression,
) -> bool {
    resolve_method_receiver(ctx, obj, method_expr).is_some_and(|(obj_ty, class, method)| {
        call_writes_receiver(ctx, &obj_ty, &class, &method).is_some()
    })
}

/// Lower the receiver, apply a CoW check when the call writes a collection
/// receiver, and return the self operand plus the receiver temp local (for
/// Perceus drops).
///
/// Whether `method_name` of `class_name` writes its receiver is read off the
/// method's body by the type checker ([`ReceiverWrites`]), so no method is
/// named here.
///
/// [`ReceiverWrites`]: crate::type_checker::receiver_writes::ReceiverWrites
fn prepare_method_self<'e>(
    ctx: &mut LoweringContext,
    obj: &'e Expression,
    obj_ty: &Type,
    (class_name, method_name): (&str, &str),
    span: Span,
) -> Result<(Receiver<'e>, Option<Local>), LoweringError> {
    if let Some(cow) = call_writes_receiver(ctx, obj_ty, class_name, method_name) {
        let (pending, temporary) = lower_pending_receiver(ctx, obj, Some((obj_ty, cow)), span)?;
        let temporary = temporary.map(|temporary| temporary.local());
        return Ok((Receiver::Pending(pending), temporary));
    }
    let self_op = lower_method_receiver(ctx, obj)?;
    let obj_temp_local = if let Operand::Copy(ref p) = self_op {
        Some(p.local)
    } else {
        None
    };
    Ok((Receiver::Ready(self_op), obj_temp_local))
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

/// True when a call on a receiver of this static type may go through the
/// vtable: a trait-typed receiver, or a class whose instances carry one.
/// Whether the method itself is dispatched — declared by a trait or abstract
/// class, or overridden below the receiver — is the slot lookup's to say.
/// `super` calls always dispatch statically.
fn should_use_virtual_dispatch(ctx: &LoweringContext, obj: &Expression, class_name: &str) -> bool {
    if matches!(&obj.node, ExpressionKind::Super) {
        return false;
    }
    let defs = &ctx.type_checker.type_definitions();
    let is_trait = matches!(defs.get(class_name), Some(TypeDefinition::Trait(_)));
    is_trait || class_needs_vtable(class_name, defs)
}

/// Lowers the receiver `obj` of a write ([`lower_pending_receiver`]), hands it
/// to `write` — which lowers the write's other operands, makes the receiver
/// its own and emits the write — then releases the temp the receiver's path
/// started at, so a write into a value no binding holds neither leaks that
/// value nor releases it twice.
pub(super) fn with_mutated_receiver<R>(
    ctx: &mut LoweringContext,
    obj: &Expression,
    cow: Option<(&Type, &'static str)>,
    span: Span,
    write: impl FnOnce(&mut LoweringContext, Receiver<'_>) -> Result<R, LoweringError>,
) -> Result<R, LoweringError> {
    let (pending, temporary) = lower_pending_receiver(ctx, obj, cow, span)?;
    let written = write(ctx, Receiver::Pending(pending))?;
    TemporaryRoot::release(temporary, ctx, span);
    Ok(written)
}

/// The local a write through `obj_op` checks for sharing first: a whole host
/// local, not a GPU-resident binding — reference counts exist on the host
/// only, and such a binding's host copy is detached by residency.
///
/// A parameter whose value its caller still holds is never checked: its
/// count holds only the caller's references, and the write would land in the
/// caller's value. A body writing one binds a copy of it on entry, so reaching
/// one is a write that entry missed, reported as an internal error.
fn cow_checked_local(
    ctx: &LoweringContext,
    obj_op: &Operand,
    span: Span,
) -> Result<Option<Local>, LoweringError> {
    let local = match obj_op {
        Operand::Move(place) if place.projection.is_empty() => place.local,
        Operand::Move(_) | Operand::Copy(_) | Operand::Constant(_) | Operand::Function(_) => {
            return Ok(None)
        }
    };
    let decl = &ctx.body.local_decls[local.0];
    if ctx.body.is_gpu() || decl.device_handle.is_some() {
        return Ok(None);
    }
    if ctx.body.is_lent_param(local) {
        let name = decl.name.as_deref().unwrap_or("?");
        return Err(lent_parameter_write(name, span));
    }
    Ok(Some(local))
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
///
/// The body's own receiver is checked instead of copied ([`emit_receiver_unique_check`]).
pub(super) fn emit_cow_check(
    ctx: &mut LoweringContext,
    obj_op: Operand,
    obj_ty: &Type,
    cow_fn_name: &str,
    span: Span,
) -> Result<Operand, LoweringError> {
    let Some(self_local) = cow_checked_local(ctx, &obj_op, span)? else {
        return Ok(obj_op);
    };
    if ctx.body.receiver == Some(self_local) {
        emit_receiver_unique_check(ctx, self_local, span);
        return Ok(obj_op);
    }
    let cow_result = ctx.push_temp(obj_ty.clone(), span);
    let cow_fn = Operand::runtime(cow_fn_name, span);
    let cow_target = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: cow_fn,
            args: vec![Operand::Move(Place::new(self_local))],
            arg_modes: Vec::new(),
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
    Ok(Operand::Move(Place::new(self_local)))
}

/// Checks that the body's own receiver, about to be written, is held by this
/// call alone, and stops the program when it is not.
///
/// A caller makes a collection the method's own before calling a method that
/// writes it, so the receiver's count is 1 on entry, and a write through it
/// keeps it so. A copy-on-write call would release the reference it replaces,
/// which on a receiver is a reference the method never held: a caller that
/// skipped making it unique would then lose the value under its own name.
/// The check only reads the receiver, so nothing is redefined; it is a call
/// of its own rather than a sharing question, which the passes that answer
/// or hoist those questions would remove.
fn emit_receiver_unique_check(ctx: &mut LoweringContext, receiver: Local, span: Span) {
    let checked = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    let target = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: Operand::runtime(rt::RECEIVER_UNIQUE, span),
            args: vec![Operand::Copy(Place::new(receiver))],
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(checked),
            target: Some(target),
        },
        span,
    ));
    ctx.set_current_block(target);
}
