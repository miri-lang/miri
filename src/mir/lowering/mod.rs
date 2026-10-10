// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering - converts AST to MIR (Mid-level Intermediate Representation).
//!
//! This module is organized into focused sub-modules:
//! - `context`: Lowering context and state management
//! - `control_flow`: Control flow constructs (if, while, for, break, continue)
//! - `expression`: Expression lowering (~1600 lines)
//! - `statement`: Statement lowering (~350 lines)
//! - `variable`: Variable declaration lowering
//! - `helpers`: Utility functions (resolve_type, bind_pattern, etc.)

pub mod class_instantiations;
pub mod compilation_ids;
pub mod constructors;
pub mod context;
pub mod control_flow;
mod derived_hash_call;
mod derived_operator_call;
pub mod dispatch;
pub mod dispatch_symbols;
mod drop_hook_call;
pub mod element_equality;
pub mod expression;
pub mod forall_cpu;
pub mod forall_gpu;
pub mod gpu_frame;
pub mod helpers;
pub mod inherited_instantiation;
pub mod instance_demand;
pub mod instantiation_limits;
pub mod kernel_launch;
pub mod loops;
pub mod method_dispatch;
pub mod reduce_gpu;
pub mod statement;
pub mod variable;
pub mod vtable_demand;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::mir::lambda::LambdaInfo;
use crate::mir::{
    BinOp, Body, Constant, Discriminant, ExecutionModel, LocalDecl, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, StorageClass, Terminator, TerminatorKind,
};
use crate::type_checker::context::GenericDefinition;
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

// Re-export commonly used items from submodules
pub(crate) use crate::mir::instantiation::{
    apply_generic_sub, has_a_monomorphized_spelling, instantiated_member_type,
    instantiation_argument, is_monomorphizable_type_argument, is_monomorphized_instantiation,
    monomorphized_arguments, type_argument,
};
pub use compilation_ids::{new_shared_compilation_ids, SharedCompilationIds};
pub use context::LoweringContext;
pub use expression::lower_expression;
pub use helpers::{bind_pattern, literal_to_u128, lower_as_return, lower_to_local, resolve_type};
pub use statement::lower_statement;

/// Lower an AST function declaration to a MIR Body.
///
/// This is the main entry point for MIR lowering. It creates a lowering context,
/// processes parameters, emits guard checks, and lowers the function body.
///
/// # Arguments
///
/// * `ast_func` - The AST statement containing the function declaration
/// * `tc` - The type checker, used to resolve types and look up definitions
/// * `is_release` - Whether this is a release build (strips debug names)
/// * `inject_allocator` - Whether to inject an implicit allocator parameter
///
/// # Errors
///
/// Returns `LoweringError` if the statement is not a function declaration,
/// if expression lowering fails, or if the resulting MIR fails validation.
/// Resolve a function's return type: explicit annotation, then the type
/// checker's inferred function type, else `void`.
fn resolve_function_return_type(
    tc: &TypeChecker,
    ret_type_expr: Option<&Expression>,
    name: &str,
    span: crate::error::syntax::Span,
) -> Type {
    if let Some(ret_expr) = ret_type_expr {
        return resolve_declared_annotation(tc, ret_expr);
    }
    match tc.get_variable_type(name).map(|t| &t.kind) {
        Some(TypeKind::Function(func)) => match &func.return_type {
            Some(rt) => resolve_type(tc, rt),
            None => Type::new(TypeKind::Void, span),
        },
        _ => Type::new(TypeKind::Void, span),
    }
}

/// A written return or parameter type in the canonical form the body's slot
/// for it needs.
///
/// `Option<int?>` reaches lowering as a named type with an argument, not as an
/// optional. Left that way, a `return` into the slot cannot tell that the value
/// must be boxed as `Some`, so a bare or one-layer-short optional is stored raw
/// and the caller reads its payload as the address of an optional. A parameter
/// left that way reads as a value no optional holds, so storing it into an
/// optional local wraps it in a second `Some`.
fn resolve_declared_annotation(tc: &TypeChecker, annotation: &Expression) -> Type {
    variable::canonical_declared_type(tc, &resolve_type(tc, annotation))
}

pub fn lower_function(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_function_with_compilation_ids(
        ast_func,
        tc,
        is_release,
        inject_allocator,
        new_shared_compilation_ids(),
    )
}

/// Lower a function while allocating any GPU kernel names from `compilation_ids`,
/// the compilation-wide sequence. The compilation driver passes one shared
/// namer for every body so kernel names are unique within the build and stable
/// across builds; [`lower_function`] wraps this with a private per-call namer.
pub fn lower_function_with_compilation_ids(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_function_body(ast_func, tc, is_release, inject_allocator, compilation_ids)
        .map_err(|error| in_declaring_file(error, ast_func, tc))
}

/// `error`, raised lowering the body of `declaration`, rendered against the
/// file that declares it: a span indexes the file it was read from, and an
/// imported body's span means nothing against the program's own file.
fn in_declaring_file(
    error: LoweringError,
    declaration: &Statement,
    tc: &TypeChecker,
) -> LoweringError {
    error.in_source(tc.declaration_source(declaration.id))
}

fn lower_function_body(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    let StatementKind::FunctionDeclaration(decl) = &ast_func.node else {
        return Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration".to_string(),
            ast_func.span,
        ));
    };
    let name = &decl.name;
    let params = &decl.params;
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let execution_model = resolve_execution_model(props);

    // A `gpu fn`'s declared/inferred return type is `Kernel` — the host-side
    // launch handle produced by referencing the kernel, not a value the device
    // code returns. The device body itself returns nothing, so its MIR return
    // local is `void` (writes flow to `out` storage buffers, not a return slot).
    let ret_ty = if execution_model == ExecutionModel::GpuKernel {
        Type::new(TypeKind::Void, ast_func.span)
    } else {
        resolve_function_return_type(tc, ret_type_expr.as_deref(), name, ast_func.span)
    };

    // Initialize lowering context
    let body = Body::new(params.len(), ast_func.span, execution_model);
    let mut ctx = LoweringContext::new(body, tc, is_release);
    ctx.use_compilation_ids(compilation_ids);

    // Populate generic type parameter names so that `is_managed_type` can
    // distinguish unresolved generic placeholders from concrete user types.
    ctx.body.type_params = collect_type_params(decl, tc);
    ctx.body.open_params = ctx.body.type_params.clone();

    // _0: Return value
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_func.span));

    // Lower parameters and record out-param flags.
    ctx.body.out_params = params.iter().map(|p| p.is_out).collect();
    for param in params.iter() {
        let param_ty = resolve_declared_annotation(tc, &param.typ);
        ctx.push_param(param.name.clone(), param_ty, param.typ.span);
    }
    assign_gpu_param_storage_classes(&mut ctx, params.len());

    // Implicit Allocator Injection — supports the "Call Site Allocator Injection" strategy.
    // GPU kernels run device-side and have no CPU allocator, so they never carry
    // the implicit param (the WGSL backend would otherwise see an unmappable param).
    if inject_allocator && ctx.body.execution_model != ExecutionModel::GpuKernel {
        inject_allocator_param(&mut ctx, name, ast_func.span);
    }

    // Emit guard checks for parameters with guards
    emit_parameter_guards(&mut ctx, params)?;

    // Lower body with support for implicit return
    if let Some(body_box) = body_stmt {
        lower_as_return(&mut ctx, body_box, &ret_ty)?;
    }

    finalize_body(&mut ctx, ast_func.span)
}

/// Replace the `Self` type keyword with the enclosing type throughout `ty`.
///
/// `Self` is a keyword, not a declared type, so it is absent from
/// `type_definitions`. Codegen resolves a field projection by looking the
/// receiver's type name up in that map, and a miss falls back to pointer-sized
/// fields at offset zero — which omits the vtable pointer that a
/// trait-implementing class stores in its first slot. A `Self`-typed binding
/// left unsubstituted therefore reads every field one slot early and returns
/// wrong values with no diagnostic, so substitution happens here, before any
/// type reaches layout.
///
/// Recurses through the wrappers a `Self` can nest inside (`Self?`,
/// `List<Self>`, tuples, results, futures) so nested spellings resolve too.
pub(crate) fn substitute_self_type(ty: &Type, self_type: &Type) -> Type {
    let substituted_kind = match &ty.kind {
        TypeKind::Custom(name, None) if name == SELF_TYPE_NAME => return self_type.clone(),
        TypeKind::Custom(name, Some(args)) => TypeKind::Custom(
            name.clone(),
            Some(substitute_self_in_type_args(args, self_type)),
        ),
        TypeKind::Option(inner) => {
            TypeKind::Option(Box::new(substitute_self_type(inner, self_type)))
        }
        TypeKind::Linear(inner) => {
            TypeKind::Linear(Box::new(substitute_self_type(inner, self_type)))
        }
        TypeKind::Tuple(elements) => {
            TypeKind::Tuple(substitute_self_in_type_args(elements, self_type))
        }
        TypeKind::OneOf(members) => {
            TypeKind::OneOf(substitute_self_in_type_args(members, self_type))
        }
        TypeKind::Result(ok, err) => {
            let mut both = substitute_self_in_type_args(&[*ok.clone(), *err.clone()], self_type);
            let substituted_err = both.pop().unwrap_or_else(|| *err.clone());
            let substituted_ok = both.pop().unwrap_or_else(|| *ok.clone());
            TypeKind::Result(Box::new(substituted_ok), Box::new(substituted_err))
        }
        TypeKind::Future(inner) => TypeKind::Future(Box::new(
            substitute_self_in_type_args(std::slice::from_ref(inner), self_type)
                .pop()
                .unwrap_or_else(|| *inner.clone()),
        )),

        // A `Self` spelled inside a function type would have to be rewritten
        // through the parameter list, which this substitution does not walk. A
        // function-typed binding is not a class instance, so it never reaches
        // field layout, which is what makes an unsubstituted `Self` dangerous.
        TypeKind::Function(_) => return ty.clone(),

        // The parser-only collection spellings are normalized to `Custom` with
        // type arguments before MIR lowering runs, so their element types are
        // substituted through the `Custom` arm above.
        TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_) => {
            return ty.clone()
        }

        // A generic parameter is resolved by generic substitution, a metatype
        // names a type rather than holding a value, and `Custom(name, None)`
        // that is not `Self` is an ordinary named type. None of these can
        // contain a nested `Self` to rewrite.
        TypeKind::Generic(_, _, _) | TypeKind::Meta(_) | TypeKind::Custom(_, None) => {
            return ty.clone()
        }

        // Scalars, strings, pointers and the empty types hold no nested type.
        // Listed rather than matched with `_` so that adding a type-carrying
        // variant fails to compile here instead of silently skipping
        // substitution — an unsubstituted `Self` reads fields at the wrong
        // offsets with no diagnostic.
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => return ty.clone(),
    };
    Type::new(substituted_kind, ty.span)
}

/// Substitute `Self` inside type-argument expressions, preserving each
/// argument's expression id and span so the MIR type cache keeps resolving it.
/// Arguments that do not wrap a type (a value generic's size literal) pass
/// through untouched.
fn substitute_self_in_type_args(args: &[Expression], self_type: &Type) -> Vec<Expression> {
    args.iter()
        .map(|arg| {
            let ExpressionKind::Type(inner, is_nullable) = &arg.node else {
                return arg.clone();
            };
            let mut substituted = arg.clone();
            substituted.node = ExpressionKind::Type(
                Box::new(substitute_self_type(inner, self_type)),
                *is_nullable,
            );
            substituted
        })
        .collect()
}

/// Apply a generic substitution to each type argument of a call's recorded
/// mapping, keeping the callee's parameter names.
pub(crate) fn substitute_call_mapping(
    mapping: &[(String, Type)],
    subs: &HashMap<String, Type>,
) -> Vec<(String, Type)> {
    mapping
        .iter()
        .map(|(name, ty)| (name.clone(), apply_generic_sub(ty, subs)))
        .collect()
}

/// The type a field of `class_name` has inside `instance`, given each class's
/// type parameters in declaration order.
///
/// One rule with two readers, which must agree: MIR lowering decides from it
/// whether a store into the field releases what it replaces, and Perceus decides
/// from it whether the field it projects is reference counted. Disagreeing, a
/// store claims a value nothing releases, or releases one nothing claimed.
///
/// A non-generic class, an instance whose arguments are not known, or a field
/// that is not declared at any of the class's parameters leaves the declared
/// spelling in place.
pub(crate) fn field_type_in_instance(
    class_type_params: &HashMap<String, Vec<String>>,
    class_name: &str,
    instance: Option<&Type>,
    field_ty: &Type,
) -> Type {
    let (Some(params), Some(TypeKind::Custom(_, Some(args)))) = (
        class_type_params.get(class_name),
        instance.map(|ty| &ty.kind),
    ) else {
        return field_ty.clone();
    };
    instantiated_member_type(params.iter().map(String::as_str), args, field_ty)
}

// The three helpers below are the shared substitution machinery for generic
// class monomorphization. `build_class_generic_substitution` drives constructor
// field-type substitution; the single-type and `type_params`-rebuild
// convenience wrappers are not yet consumed and carry a dead-code allowance
// until their step lands.

/// Build the substitution map from a generic class's declared parameters to the
/// concrete type arguments of a resolved instantiation.
///
/// `resolved_class_ty` is the resolved `Custom(name, Some(args))` produced by
/// type resolution: each argument expression wraps a concrete `Type`
/// (`ExpressionKind::Type`). Every declared generic (`T`, `K`, …) is paired
/// positionally with its argument, extracted through
/// [`TypeChecker::extract_type_from_expression`] so no type-name string table is
/// hand-rolled. A value-generic (size) slot whose argument is not a type
/// expression is skipped, leaving that name unmapped.
pub(crate) fn build_class_generic_substitution(
    tc: &TypeChecker,
    def_generics: &[GenericDefinition],
    resolved_class_ty: &Type,
) -> HashMap<String, Type> {
    let mut subs = HashMap::new();
    let TypeKind::Custom(_, Some(args)) = &resolved_class_ty.kind else {
        return subs;
    };
    for (generic, arg) in def_generics.iter().zip(args) {
        if let Ok(concrete) = tc.extract_type_from_expression(arg) {
            subs.insert(generic.name.clone(), concrete);
        }
    }
    subs
}

/// Substitute a generic class's parameters inside a single field/return `Type`.
///
/// Convenience over [`build_class_generic_substitution`] + [`apply_generic_sub`]
/// for the single-type case (a field type, a method return type). Callers that
/// substitute many types should build the map once and reuse [`apply_generic_sub`].
#[allow(dead_code)]
pub(crate) fn substitute_class_generics(
    tc: &TypeChecker,
    def_generics: &[GenericDefinition],
    resolved_class_ty: &Type,
    ty: &Type,
) -> Type {
    let subs = build_class_generic_substitution(tc, def_generics, resolved_class_ty);
    apply_generic_sub(ty, &subs)
}

/// Rebuild a monomorphized class body's `type_params`, dropping every generic
/// name pinned to a concrete type by `subs`.
///
/// The names left behind are the still-opaque generics — an unsubstituted
/// value-generic size or a nested generic — that Perceus must keep treating as
/// unresolved. A substituted scalar `T` is removed so Perceus sees it as the
/// concrete scalar the instantiation monomorphized to rather than an opaque
/// (managed) placeholder.
#[allow(dead_code)]
pub(crate) fn rebuild_class_type_params(
    def_generics: &[GenericDefinition],
    subs: &HashMap<String, Type>,
) -> Vec<String> {
    def_generics
        .iter()
        .map(|generic| generic.name.clone())
        .filter(|name| !subs.contains_key(name))
        .collect()
}

/// Whether a per-instantiation body can be named for a receiver typed `kind`.
///
/// Only the class-reference spelling (`Custom("List", Some([...]))`) carries the
/// arguments a mangled symbol is built from; the canonical variants
/// (`TypeKind::List(...)`) name no arguments to mangle. Every argument must have
/// a token too — a type argument's type, or a value generic's constant.
pub(crate) fn can_be_monomorphized_at(kind: &TypeKind) -> bool {
    let TypeKind::Custom(_, Some(args)) = kind else {
        return false;
    };
    args.iter()
        .all(crate::mir::symbol::token::argument_has_a_mangled_token)
}

/// Lower a generic function with concrete type substitutions to produce a
/// specialised MIR Body.
///
/// This is used by the monomorphisation pass in the pipeline after all call
/// sites have been lowered. `mangled_name` is the already-computed symbol
/// (e.g. `identity__int`) and `subs` maps each generic parameter name to its
/// concrete type.
/// Resolve a generic function's return type (same precedence as
/// [`resolve_function_return_type`]) with the generic substitution applied.
fn resolve_generic_return_type(
    tc: &TypeChecker,
    ret_type_expr: Option<&Expression>,
    name: &str,
    span: crate::error::syntax::Span,
    subs: &HashMap<String, Type>,
) -> Type {
    if let Some(ret_expr) = ret_type_expr {
        return apply_generic_sub(&resolve_declared_annotation(tc, ret_expr), subs);
    }
    match tc.get_variable_type(name).map(|t| &t.kind) {
        Some(TypeKind::Function(func)) => match &func.return_type {
            Some(rt) => apply_generic_sub(&resolve_type(tc, rt), subs),
            None => Type::new(TypeKind::Void, span),
        },
        _ => Type::new(TypeKind::Void, span),
    }
}

pub fn lower_generic_instantiation(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    subs: &HashMap<String, Type>,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_generic_instantiation_with_compilation_ids(
        ast_func,
        tc,
        is_release,
        inject_allocator,
        subs,
        new_shared_compilation_ids(),
    )
}

/// [`lower_generic_instantiation`] that allocates GPU kernel names from the
/// compilation-wide `compilation_ids` instead of a private per-call one.
pub fn lower_generic_instantiation_with_compilation_ids(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    subs: &HashMap<String, Type>,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_instantiation_core(
        ast_func,
        tc,
        is_release,
        inject_allocator,
        subs,
        &[],
        compilation_ids,
    )
}

/// Lower a residency-specialized instantiation of a `GpuLaunchSafe` function.
///
/// `param_handles[i]` is the persistent device handle for parameter `i` when
/// that parameter received a gpu-resident buffer at the call site. Stamping the
/// handle (and `Gpu` residency) on the parameter local before the body is
/// lowered means the body's `forall` capture resolves to that same device
/// buffer — the whole GPU launch path is reused unchanged. A generic function
/// is specialized at the instantiation `subs` the call reaches, so its kernels
/// are compiled at those types; `subs` is empty for a function that is not
/// generic. The closures written in the body are emitted again for each
/// specialization, so `param_handles` reaches their symbols too: the residency
/// pattern tells apart the copies of lowerings at one instantiation.
pub fn lower_residency_instantiation_with_compilation_ids(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    subs: &HashMap<String, Type>,
    param_handles: &[Option<crate::mir::body::DeviceHandleId>],
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_instantiation_core(
        ast_func,
        tc,
        is_release,
        inject_allocator,
        subs,
        param_handles,
        compilation_ids,
    )
}

/// Shared core for generic and residency instantiations. `subs` monomorphizes
/// generic type parameters; `param_handles` stamps device handles onto gpu-
/// resident parameters. The two axes are independent and may combine.
fn lower_instantiation_core(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    subs: &HashMap<String, Type>,
    param_handles: &[Option<crate::mir::body::DeviceHandleId>],
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_instantiation_body(
        ast_func,
        tc,
        is_release,
        inject_allocator,
        subs,
        param_handles,
        compilation_ids,
    )
    .map_err(|error| in_declaring_file(error, ast_func, tc))
}

fn lower_instantiation_body(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    subs: &HashMap<String, Type>,
    param_handles: &[Option<crate::mir::body::DeviceHandleId>],
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    let StatementKind::FunctionDeclaration(decl) = &ast_func.node else {
        return Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration".to_string(),
            ast_func.span,
        ));
    };
    let name = &decl.name;
    let params = &decl.params;
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let ret_ty =
        resolve_generic_return_type(tc, ret_type_expr.as_deref(), name, ast_func.span, subs);

    let execution_model = resolve_execution_model(props);

    let body = Body::new(params.len(), ast_func.span, execution_model);
    let mut ctx = LoweringContext::new(body, tc, is_release);
    ctx.use_compilation_ids(compilation_ids);

    // For an instantiated generic, `subs` maps each generic name to its concrete
    // type — those names no longer remain as unresolved placeholders after
    // substitution. Populate type_params with the original names anyway so that
    // any types not yet substituted (e.g. nested generics) are handled correctly.
    ctx.body.type_params = subs.keys().cloned().collect();
    (ctx.body.open_params, ctx.body.bound_params) = substitution_parameters(subs, tc);
    // Carry the substitution so that a type the type checker recorded against
    // the generic parameter — an operand of an operator, an intrinsic element
    // read — resolves to the instantiation's concrete type. Without it the
    // operator dispatch sees a parameter that names no class and compares the
    // operands' addresses.
    ctx.generic_subs = subs.clone();

    // _0: Return value (concrete type)
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_func.span));

    // Lower parameters and record out-param flags. `push_param` reads each
    // declared type at the instantiation.
    ctx.body.out_params = params.iter().map(|p| p.is_out).collect();
    for param in params.iter() {
        ctx.push_param(
            param.name.clone(),
            resolve_declared_annotation(tc, &param.typ),
            param.typ.span,
        );
    }
    stamp_residency_param_handles(&mut ctx, param_handles);
    ctx.residency_handles = param_handles
        .iter()
        .enumerate()
        .filter_map(|(idx, handle)| handle.map(|handle| (idx, handle)))
        .collect();
    assign_gpu_param_storage_classes(&mut ctx, params.len());

    if inject_allocator {
        inject_allocator_param(&mut ctx, name, ast_func.span);
    }

    emit_parameter_guards(&mut ctx, params)?;

    if let Some(body_box) = body_stmt {
        lower_as_return(&mut ctx, body_box, &ret_ty)?;
    }

    finalize_body(&mut ctx, ast_func.span)
}

/// Stamp the caller's device handle and `Gpu` residency onto each specialized
/// parameter local. Parameter `i` lives at local `i + 1` (`_0` is the return
/// slot). A `None` entry leaves the parameter host-resident.
fn stamp_residency_param_handles(
    ctx: &mut LoweringContext,
    param_handles: &[Option<crate::mir::body::DeviceHandleId>],
) {
    for (i, handle) in param_handles.iter().enumerate() {
        let Some(handle) = handle else { continue };
        let local = i + 1;
        if local < ctx.body.local_decls.len() {
            ctx.body.local_decls[local].device_handle = Some(*handle);
            ctx.body.local_decls[local].residency = crate::mir::body::BindingResidency::Gpu;
            // The parameter borrows the caller's device buffer; it must not be
            // released when the specialized body's scope exits.
            ctx.body.local_decls[local].device_handle_borrowed = true;
        }
    }
}

/// Lower a stdlib class method to a MIR Body.
///
/// Unlike [`lower_function`], this variant:
/// - Prepends an implicit `self` parameter (registered in `variable_map`)
/// - Registers the allocator in the function ABI (`body.arg_count`) but NOT in
///   the lowering context's `variable_map`
///
/// Keeping the allocator out of `variable_map` prevents the auto-injector from
/// appending it to calls to runtime C functions inside the method body. Those C
/// functions do not accept an allocator parameter.
///
/// # Arguments
///
/// * `ast_method` - The AST statement containing the method declaration
/// * `self_type` - The type of the implicit `self` parameter
/// * `tc` - The type checker, used to resolve types and look up definitions
/// * `is_release` - Whether this is a release build (strips debug names)
///
/// # Errors
///
/// Returns `LoweringError` if the statement is not a function declaration,
/// if expression lowering fails, or if the resulting MIR fails validation.
/// Enum/class-level generic names (e.g. `T`, `E` from `Result<T, E>`) declared
/// on the receiver's type definition. `collect_type_params` only catches
/// `TypeKind::Generic`, missing names that resolve to `Custom("T", None)` for
/// enum/class methods; reading them from the type definition closes that gap so
/// Perceus does not treat unresolved placeholders as concrete heap-managed types.
fn class_level_generic_names(self_type: &Type, tc: &TypeChecker) -> Vec<String> {
    let TypeKind::Custom(class_name, _) = &self_type.kind else {
        return Vec::new();
    };
    let Some(type_def) = tc.type_definitions().get(class_name.as_str()) else {
        return Vec::new();
    };
    let generics = match type_def {
        crate::type_checker::context::TypeDefinition::Enum(ed) => ed.generics.as_deref(),
        crate::type_checker::context::TypeDefinition::Class(cd) => cd.generics.as_deref(),
        _ => None,
    };
    generics
        .map(|gens| gens.iter().map(|g| g.name.clone()).collect())
        .unwrap_or_default()
}

pub fn lower_class_method(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_class_method_impl(
        ast_method,
        self_type,
        tc,
        is_release,
        &HashMap::new(),
        new_shared_compilation_ids(),
    )
}

/// [`lower_class_method`] that allocates GPU kernel names from the
/// compilation-wide `compilation_ids` instead of a private per-call one.
pub fn lower_class_method_with_compilation_ids(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_class_method_impl(
        ast_method,
        self_type,
        tc,
        is_release,
        &HashMap::new(),
        compilation_ids,
    )
}

/// Lower a class method with `self` typed as `self_type` and every generic
/// parameter its body names read through `subs`: the return type, parameter
/// types and `type_params` are rebuilt through it, so a scalar `T` pinned to a
/// concrete type is no longer an opaque managed placeholder.
///
/// A class's copy of a trait default it inherits passes what its `extends`
/// and `implements` clauses pin the trait's own parameters to: `class Impl
/// implements Op<int>` leaves no type argument on the receiver, yet fixes
/// `Op`'s `T`, and a bare `T` would be an opaque managed value released as
/// one. An instantiation's copy passes that instantiation's arguments as well,
/// while `self_type` carries the class's own — so a trait parameter sharing a
/// class parameter's name reads the trait's pin in the body without retyping
/// `self`.
pub fn lower_class_method_at_with_compilation_ids(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
    pinned: &HashMap<String, Type>,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_class_method_impl(
        ast_method,
        self_type,
        tc,
        is_release,
        pinned,
        compilation_ids,
    )
}

/// Build the `self` type for a monomorphized method: `Custom(class, Some(args))`
/// with one type argument per declared class generic, in declaration order, so
/// codegen's positional field-type substitution resolves a scalar `T` field to
/// its concrete width. A generic absent from `subs` (an unsubstituted
/// value-generic size) is emitted as a bare identifier placeholder that codegen
/// leaves untouched. Falls back to the bare class type when the class is
/// non-generic.
pub fn monomorphized_self_type(
    class_name: &str,
    tc: &TypeChecker,
    subs: &HashMap<String, Type>,
    span: crate::error::syntax::Span,
) -> Type {
    let generics = match tc.type_definitions().get(class_name) {
        Some(
            definition @ (crate::type_checker::context::TypeDefinition::Class(_)
            | crate::type_checker::context::TypeDefinition::Enum(_)),
        ) => definition.generics(),
        _ => None,
    };
    let Some(generics) = generics.filter(|g| !g.is_empty()) else {
        return Type::new(TypeKind::Custom(class_name.to_string(), None), span);
    };
    let args = generics
        .iter()
        .map(|g| match subs.get(&g.name) {
            Some(ty) => Expression {
                id: 0,
                span,
                node: ExpressionKind::Type(Box::new(ty.clone()), false),
            },
            None => Expression {
                id: 0,
                span,
                node: ExpressionKind::Identifier(g.name.clone(), None),
            },
        })
        .collect();
    Type::new(TypeKind::Custom(class_name.to_string(), Some(args)), span)
}

fn lower_class_method_impl(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
    subs: &HashMap<String, Type>,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    lower_class_method_body(ast_method, self_type, tc, is_release, subs, compilation_ids)
        .map_err(|error| in_declaring_file(error, ast_method, tc))
}

fn lower_class_method_body(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
    subs: &HashMap<String, Type>,
    compilation_ids: SharedCompilationIds,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    let StatementKind::FunctionDeclaration(decl) = &ast_method.node else {
        return Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration for class method".to_string(),
            ast_method.span,
        ));
    };
    let params = decl.explicit_params();
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let ret_ty = ret_type_expr.as_deref().map_or_else(
        || Type::new(TypeKind::Void, ast_method.span),
        |e| {
            let declared = apply_generic_sub(&resolve_declared_annotation(tc, e), subs);
            substitute_self_type(&declared, &self_type)
        },
    );

    let execution_model = resolve_execution_model(props);

    // For instance methods: arg_count = 1 (self) + explicit params.
    // For static methods: arg_count = explicit params (no self).
    // The allocator is counted below but not added to variable_map.
    let has_self = !props.is_static;
    let body = Body::new(
        params.len() + if has_self { 1 } else { 0 },
        ast_method.span,
        execution_model,
    );
    let mut ctx = LoweringContext::new(body, tc, is_release);
    ctx.use_compilation_ids(compilation_ids);
    // `Self` inside this body names the class being lowered. Recorded so that a
    // declared type spelling `Self` resolves to the class's real layout instead
    // of falling through codegen's unknown-type path.
    ctx.self_type = Some(self_type.clone());
    // Carry the instantiation substitution so an intrinsic element read typed
    // `T` (e.g. `self.items.element_at(0)`) resolves to the concrete
    // instantiation type instead of the pointer-width fallback.
    ctx.generic_subs = subs.clone();

    // The field-type table is built once from the class declaration, so a field
    // declared `value T` still reads as its type parameter. Perceus decides by
    // that type whether assigning into the field claims the value, so an
    // instantiation at a managed type would store what it never retained and the
    // caller would release it out from under the field.
    if let TypeKind::Custom(owner, _) = &self_type.kind {
        if let Some(field_types) = ctx.body.field_types.get_mut(owner) {
            for field_ty in field_types.iter_mut() {
                *field_ty = apply_generic_sub(field_ty, subs);
            }
        }
    }

    // Method-own generics plus class-level generics that appear in param/return
    // types (e.g. T in List<T>), minus any name pinned to a concrete type by
    // `subs`. Dropping substituted names lets Perceus treat a monomorphized
    // scalar `T` as concrete rather than an opaque managed placeholder. For the
    // non-monomorphized path `subs` is empty, so nothing is dropped.
    let mut type_params = collect_type_params(decl, tc);
    for name in class_level_generic_names(&self_type, tc) {
        type_params.insert(name);
    }
    type_params.retain(|name| !subs.contains_key(name));
    let (open_through_subs, bound_params) = substitution_parameters(subs, tc);
    ctx.body.open_params = type_params.union(&open_through_subs).cloned().collect();
    ctx.body.bound_params = bound_params;
    ctx.body.type_params = type_params;

    // _0: Return value
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_method.span));

    // _1: self parameter (instance methods only; the class instance, registered in variable_map).
    // Static methods skip this slot entirely.
    if has_self {
        ctx.push_param("self".to_string(), self_type.clone(), ast_method.span);
    }

    // Remaining explicit parameters (registered in variable_map).
    // For instance methods, ABI param 0 is `self` (never `out`); explicit params follow at 1..=N.
    // For static methods, explicit params start at 0.
    // The allocator is appended below as a non-out ABI param.
    let mut out_params = Vec::with_capacity(params.len() + 2);
    if has_self {
        out_params.push(false); // self is never `out`
    }
    for param in params.iter() {
        let param_ty =
            substitute_self_type(&resolve_declared_annotation(tc, &param.typ), &self_type);
        ctx.push_param(param.name.clone(), param_ty, param.typ.span);
        out_params.push(param.is_out);
    }

    // Inject allocator into the ABI for call-site compatibility.
    // We MUST register it in variable_map so that method-to-method calls can pass the allocator.
    let allocator_decl = LocalDecl::new(Type::new(TypeKind::Int, ast_method.span), ast_method.span);
    let alloc_local = ctx.body.new_local(allocator_decl);
    ctx.variable_map.insert("allocator".into(), alloc_local);
    ctx.body.allocator = Some(alloc_local);
    ctx.body.arg_count += 1;
    out_params.push(false);
    ctx.body.out_params = out_params;

    // Lower body
    if let Some(body_box) = body_stmt {
        lower_as_return(&mut ctx, body_box, &ret_ty)?;
    }

    finalize_body(&mut ctx, ast_method.span)
}

/// Resolve execution model from function properties.
pub(super) fn resolve_execution_model(
    props: &crate::ast::common::FunctionProperties,
) -> ExecutionModel {
    if props.is_gpu {
        ExecutionModel::GpuKernel
    } else if props.is_async {
        ExecutionModel::Async
    } else {
        ExecutionModel::Cpu
    }
}

/// Assign WGSL storage classes to the parameters of an explicit `gpu fn`: a
/// buffer-shaped parameter becomes a `GpuGlobal` `var<storage>` binding, and a
/// scalar with a GPU wire lane a `UniformBuffer` field of the kernel's scalar
/// inputs, which the launch fills from the call's scalar arguments. A no-op
/// for non-GPU functions; `forall_gpu` assigns its own kernels' classes.
fn assign_gpu_param_storage_classes(ctx: &mut LoweringContext, param_count: usize) {
    if ctx.body.execution_model != ExecutionModel::GpuKernel {
        return;
    }
    for param_idx in 1..=param_count {
        let kind = &ctx.body.local_decls[param_idx].ty.kind;
        if is_gpu_storage_param(kind) {
            ctx.body.local_decls[param_idx].storage_class = StorageClass::GpuGlobal;
        } else if crate::ast::gpu_wire::scalar_capture_wire(kind).is_some() {
            ctx.body.local_decls[param_idx].storage_class = StorageClass::UniformBuffer;
        }
    }
}

/// True for parameter types the GPU dispatcher marshals as a storage buffer
/// (host-side `MiriArray`-shaped). Mirrors the buffer-capture classification
/// used by the `forall` path.
fn is_gpu_storage_param(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Array(_, _) | TypeKind::List(_) => true,
        TypeKind::Custom(name, _) => matches!(
            crate::ast::types::BuiltinCollectionKind::from_name(name),
            Some(
                crate::ast::types::BuiltinCollectionKind::Array
                    | crate::ast::types::BuiltinCollectionKind::List
            )
        ),
        _ => false,
    }
}

/// Collect generic type parameter names from a function declaration.
///
/// Extracts names from:
/// - Explicit generic declarations in `decl.generics` (e.g. `fn foo<T, K>(...)`)
/// - `TypeKind::Generic` names found in parameter types (captures class-level
///   generics that appear in method signatures, e.g. `T` in `List<T>::push(item: T)`)
/// - `TypeKind::Generic` names found in the return type
///
/// The resulting set is stored in `Body::type_params` and used by `is_managed_type`
/// to distinguish unresolved generic placeholders from concrete user-defined types.
fn collect_type_params(
    decl: &crate::ast::statement::FunctionDeclarationData,
    tc: &TypeChecker,
) -> std::collections::HashSet<String> {
    let mut params = std::collections::HashSet::new();

    // Explicit generic declarations (e.g., `fn foo<T>(...)`)
    if let Some(gens) = &decl.generics {
        for gen_expr in gens {
            if let ExpressionKind::GenericType(name_expr, _, _) = &gen_expr.node {
                if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                    params.insert(name.clone());
                }
            }
        }
    }

    // Generic names appearing in parameter types (catches class-level generics)
    for param in &decl.params {
        collect_generic_names_from_type(
            &resolve_type(tc, &param.typ),
            tc.type_definitions(),
            &mut params,
        );
    }

    // Generic names appearing in the return type
    if let Some(ret_expr) = &decl.return_type {
        collect_generic_names_from_type(
            &resolve_type(tc, ret_expr),
            tc.type_definitions(),
            &mut params,
        );
    }

    params
}

/// The parameters a body lowered under `subs` leaves open, and the ones it
/// binds.
///
/// A parameter substituted by a concrete type is bound. One substituted by a
/// type still naming a parameter — a callee reached from a shared body at that
/// body's own open parameter (`getN<T>` called inside `Box<T>`) — is not: the
/// body is shared by every instantiation, and the parameters that type names
/// are the ones it leaves open.
fn substitution_parameters(
    subs: &HashMap<String, Type>,
    tc: &TypeChecker,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    let definitions = tc.type_definitions();
    let mut open = std::collections::HashSet::new();
    let mut bound = std::collections::HashSet::new();
    for (name, ty) in subs {
        let still_open = crate::mir::instantiation::shape::open_parameter_names(ty, definitions);
        if still_open.is_empty() {
            bound.insert(name.clone());
        } else {
            open.extend(still_open);
        }
    }
    (open, bound)
}

/// Recursively collect the type parameter names a resolved type reads: each
/// `TypeKind::Generic`, and each type argument written as a bare name the type
/// table does not define (`T` in `List<T>` before normalization), inside every
/// type that can carry one — an optional, a collection, a tuple, a result, a
/// future, a function's parameters and return.
fn collect_generic_names_from_type(
    ty: &Type,
    definitions: &HashMap<String, crate::type_checker::context::TypeDefinition>,
    params: &mut std::collections::HashSet<String>,
) {
    // A value argument (`3`, `Size + 1`) names no type parameter a body holds
    // a value of, so only a type and a bare name are read.
    let mut collect_argument = |arg: &Expression| {
        if let ExpressionKind::Type(inner, _) = &arg.node {
            collect_generic_names_from_type(inner, definitions, params);
        } else if let ExpressionKind::Identifier(name, None) = &arg.node {
            if !definitions.contains_key(name) {
                params.insert(name.clone());
            }
        }
    };
    match &ty.kind {
        TypeKind::Generic(name, _, _) => {
            params.insert(name.clone());
        }
        TypeKind::Option(inner) | TypeKind::Linear(inner) | TypeKind::Meta(inner) => {
            collect_generic_names_from_type(inner, definitions, params)
        }
        // Canonical collection variants may appear when resolve_type reads a raw
        // type expression from the parser before normalization.
        TypeKind::List(elem) | TypeKind::Set(elem) | TypeKind::Future(elem) => {
            collect_argument(elem)
        }
        TypeKind::Array(elem, _) => collect_argument(elem),
        TypeKind::Map(first, second) | TypeKind::Result(first, second) => {
            collect_argument(first);
            collect_argument(second);
        }
        TypeKind::Tuple(elements) | TypeKind::OneOf(elements) => {
            elements.iter().for_each(collect_argument)
        }
        TypeKind::Custom(_, Some(args)) => args.iter().for_each(collect_argument),
        TypeKind::Function(function) => {
            function
                .params
                .iter()
                .for_each(|param| collect_argument(&param.typ));
            if let Some(return_type) = &function.return_type {
                collect_argument(return_type);
            }
        }
        // A named type without arguments and every type built from nothing
        // read no parameter.
        TypeKind::Custom(_, None)
        | TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => {}
    }
}

/// Bind the allocator every call made in this body passes on to a local
/// holding zero, for a body whose own signature carries none: the entry point,
/// and a body the runtime calls with a fixed signature. Initialized so no call
/// reads an undefined value.
pub(crate) fn bind_null_allocator(ctx: &mut LoweringContext, span: crate::error::syntax::Span) {
    let allocator_type = Type::new(TypeKind::Int, span);
    let alloc_local = ctx.push_local("allocator".to_string(), allocator_type.clone(), span);
    ctx.body.allocator = Some(alloc_local);
    let null_allocator = Operand::Constant(Box::new(Constant {
        span,
        ty: allocator_type,
        literal: crate::ast::literal::Literal::Integer(crate::ast::literal::IntegerLiteral::I32(0)),
    }));
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(alloc_local), Rvalue::Use(null_allocator)),
        span,
    });
}

/// Inject an allocator parameter into the lowering context.
///
/// For `main`, creates a local variable initialized to 0 (cannot inject a parameter
/// as it would break the entry point signature). For all other functions, appends
/// an additional parameter to the function signature.
fn inject_allocator_param(
    ctx: &mut LoweringContext,
    function_name: &str,
    span: crate::error::syntax::Span,
) {
    let allocator_type = Type::new(TypeKind::Int, span);

    if function_name == "main" {
        // For main, create a local variable instead of a parameter to preserve
        // the entry point ABI.
        bind_null_allocator(ctx, span);
    } else {
        let alloc_local = ctx.push_param("allocator".to_string(), allocator_type, span);
        ctx.body.allocator = Some(alloc_local);
        ctx.body.arg_count += 1;
    }
}

/// Emit guard checks for parameters that have guard conditions.
///
/// For each parameter with a guard (e.g., `n > 0`), emits a comparison followed
/// by a conditional branch to an unreachable block on failure.
fn emit_parameter_guards(
    ctx: &mut LoweringContext,
    params: &[crate::ast::common::Parameter],
) -> Result<(), LoweringError> {
    for param in params {
        emit_param_guard(ctx, param)?;
    }
    Ok(())
}

/// Emit the comparison + fail-on-false branch for a single guarded parameter.
/// Parameters without a (supported) guard are left untouched.
fn emit_param_guard(
    ctx: &mut LoweringContext,
    param: &crate::ast::common::Parameter,
) -> Result<(), LoweringError> {
    let Some(guard) = &param.guard else {
        return Ok(());
    };
    let Some(&param_local) = ctx.variable_map.get(param.name.as_str()) else {
        return Ok(());
    };
    let ExpressionKind::Guard(guard_op, guard_value) = &guard.node else {
        return Ok(());
    };

    let arg_watermark = ctx.body.local_decls.len();
    let guard_val = lower_expression(ctx, guard_value, None)?;
    let Some(bin_op) = guard_op_to_binop(guard_op) else {
        return Ok(());
    };

    let check_result = ctx.push_temp(Type::new(TypeKind::Boolean, guard.span), guard.span);
    let lowered_through_trait = emit_guard_trait_comparison(
        ctx,
        GuardComparison {
            param_local,
            guard_val: guard_val.clone(),
            check_result,
            arg_watermark,
        },
        guard_op,
        guard,
    )?;
    if !lowered_through_trait {
        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(
                Place::new(check_result),
                Rvalue::BinaryOp(
                    bin_op,
                    Box::new(Operand::Copy(Place::new(param_local))),
                    Box::new(guard_val),
                ),
            ),
            span: guard.span,
        });
    }
    emit_guard_fail_branch(ctx, check_result, guard.span);
    Ok(())
}

/// The pieces a guard comparison is built from.
struct GuardComparison {
    param_local: crate::mir::Local,
    guard_val: Operand,
    check_result: crate::mir::Local,
    arg_watermark: usize,
}

/// Emit the guard's comparison as a call to the operator trait method the
/// parameter's type defines, returning whether it did.
///
/// A guard is the same operator the author could have written in the body, so
/// it has to answer the same way. Without this a `String` parameter guard would
/// compare the two operands' addresses.
fn emit_guard_trait_comparison(
    ctx: &mut LoweringContext,
    comparison: GuardComparison,
    guard_op: &crate::ast::operator::GuardOp,
    guard: &Expression,
) -> Result<bool, LoweringError> {
    let Some(binary_op) = guard_op_to_binary_op(guard_op) else {
        return Ok(false);
    };
    let param_ty = ctx.body.local_decls[comparison.param_local.0].ty.clone();
    let operands = crate::mir::lowering::expression::binary_expr::OperatorOperands {
        lhs_op: Operand::Copy(Place::new(comparison.param_local)),
        rhs_op: comparison.guard_val,
    };
    let lowered = crate::mir::lowering::expression::binary_expr::try_lower_operator_trait_call(
        ctx,
        &param_ty,
        &binary_op,
        operands,
        guard.span,
        Some(Place::new(comparison.check_result)),
        comparison.arg_watermark,
    )?;
    Ok(lowered.is_some())
}

/// Map a guard operator to the binary operator it spells (None if the guard is
/// not a comparison between two values).
fn guard_op_to_binary_op(
    op: &crate::ast::operator::GuardOp,
) -> Option<crate::ast::operator::BinaryOp> {
    use crate::ast::operator::{BinaryOp, GuardOp};
    match op {
        GuardOp::GreaterThan => Some(BinaryOp::GreaterThan),
        GuardOp::GreaterThanEqual => Some(BinaryOp::GreaterThanEqual),
        GuardOp::LessThan => Some(BinaryOp::LessThan),
        GuardOp::LessThanEqual => Some(BinaryOp::LessThanEqual),
        GuardOp::NotEqual => Some(BinaryOp::NotEqual),
        GuardOp::Not | GuardOp::In | GuardOp::NotIn => None,
    }
}

/// Map a guard operator to its MIR comparison op (None if unsupported).
fn guard_op_to_binop(op: &crate::ast::operator::GuardOp) -> Option<BinOp> {
    match op {
        crate::ast::operator::GuardOp::GreaterThan => Some(BinOp::Gt),
        crate::ast::operator::GuardOp::GreaterThanEqual => Some(BinOp::Ge),
        crate::ast::operator::GuardOp::LessThan => Some(BinOp::Lt),
        crate::ast::operator::GuardOp::LessThanEqual => Some(BinOp::Le),
        crate::ast::operator::GuardOp::NotEqual => Some(BinOp::Ne),
        _ => None,
    }
}

/// Branch on `check_result`: continue when true, else jump to an unreachable
/// fail block. Leaves the current block at the continue path.
fn emit_guard_fail_branch(
    ctx: &mut LoweringContext,
    check_result: crate::mir::Local,
    span: crate::error::syntax::Span,
) {
    let continue_bb = ctx.new_basic_block();
    let fail_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(check_result)),
            targets: vec![(Discriminant::bool_true(), continue_bb)],
            otherwise: fail_bb,
        },
        span,
    ));
    ctx.set_current_block(fail_bb);
    ctx.set_terminator(Terminator::new(TerminatorKind::Unreachable, span));
    ctx.set_current_block(continue_bb);
}

/// Finalize the lowering context: pop root scope, ensure termination, and validate.
///
/// This shared logic is used by both [`lower_function`] and [`lower_class_method`]
/// to avoid duplicating the post-lowering finalization sequence.
///
/// # Errors
///
/// Returns `LoweringError` if the MIR body fails validation.
fn finalize_body(
    ctx: &mut LoweringContext,
    span: crate::error::syntax::Span,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    // Pop root scope variables if falling through
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.pop_scope(span);
    }

    // Ensure the last block has a terminator
    let last_block_idx = ctx.current_block.0;
    if ctx.body.basic_blocks[last_block_idx].terminator.is_none() {
        ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    }

    // Validate the body
    if let Err(msg) = ctx.body.validate() {
        return Err(LoweringError::internal(
            DiagnosticCode::MirValidationFailed,
            format!("MIR body failed validation: {}", msg),
            span,
        ));
    }

    ctx.record_local_class_instantiations();

    let body = std::mem::replace(&mut ctx.body, Body::new(0, span, ExecutionModel::Cpu));
    let lambda_bodies = std::mem::take(&mut ctx.lambda_bodies);
    Ok((body, lambda_bodies))
}

#[cfg(test)]
mod class_generic_substitution_tests {
    use super::*;
    use crate::ast::types::TypeDeclarationKind;
    use crate::ast::IdNode;
    use crate::error::syntax::Span;

    fn generic(name: &str) -> GenericDefinition {
        GenericDefinition {
            name: name.to_string(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }
    }

    fn generic_field(name: &str) -> Type {
        Type::new(
            TypeKind::Generic(name.to_string(), None, TypeDeclarationKind::None),
            Span::new(0, 0),
        )
    }

    /// Build a resolved `Custom(class, Some(args))` whose argument expressions
    /// each wrap a concrete `Type` — the shape type resolution produces.
    fn instantiation(tc: &TypeChecker, class: &str, args: Vec<Type>) -> Type {
        let arg_exprs = args
            .into_iter()
            .map(|ty| tc.create_type_expression(ty))
            .collect();
        Type::new(
            TypeKind::Custom(class.to_string(), Some(arg_exprs)),
            Span::new(0, 0),
        )
    }

    #[test]
    fn substitution_map_pairs_generics_with_resolved_args() {
        let tc = TypeChecker::new();
        let defs = [generic("K"), generic("V")];
        let ty = instantiation(
            &tc,
            "Pair",
            vec![
                Type::new(TypeKind::Int, Span::new(0, 0)),
                Type::new(TypeKind::Float, Span::new(0, 0)),
            ],
        );
        let subs = build_class_generic_substitution(&tc, &defs, &ty);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs["K"].kind, TypeKind::Int);
        assert_eq!(subs["V"].kind, TypeKind::Float);
    }

    #[test]
    fn substitutes_generic_field_type_to_concrete() {
        let tc = TypeChecker::new();
        let defs = [generic("T")];
        let ty = instantiation(
            &tc,
            "Box",
            vec![Type::new(TypeKind::Float, Span::new(0, 0))],
        );
        let substituted = substitute_class_generics(&tc, &defs, &ty, &generic_field("T"));
        assert_eq!(substituted.kind, TypeKind::Float);
    }

    #[test]
    fn substitutes_bare_identifier_generic_field_to_concrete() {
        // A generic param written as a plain identifier resolves to `Custom(name, None)`.
        let tc = TypeChecker::new();
        let defs = [generic("T")];
        let ty = instantiation(
            &tc,
            "Box",
            vec![Type::new(TypeKind::Float, Span::new(0, 0))],
        );
        let bare = Type::new(TypeKind::Custom("T".to_string(), None), Span::new(0, 0));
        let substituted = substitute_class_generics(&tc, &defs, &ty, &bare);
        assert_eq!(substituted.kind, TypeKind::Float);
    }

    #[test]
    fn leaves_unrelated_field_type_unchanged() {
        let tc = TypeChecker::new();
        let defs = [generic("T")];
        let ty = instantiation(
            &tc,
            "Box",
            vec![Type::new(TypeKind::Float, Span::new(0, 0))],
        );
        let unrelated = Type::new(TypeKind::String, Span::new(0, 0));
        let substituted = substitute_class_generics(&tc, &defs, &ty, &unrelated);
        assert_eq!(substituted.kind, TypeKind::String);
    }

    #[test]
    fn rebuilt_type_params_exclude_substituted_names() {
        let tc = TypeChecker::new();
        let defs = [generic("T")];
        let ty = instantiation(
            &tc,
            "Box",
            vec![Type::new(TypeKind::Float, Span::new(0, 0))],
        );
        let subs = build_class_generic_substitution(&tc, &defs, &ty);
        assert_eq!(
            rebuild_class_type_params(&defs, &subs),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rebuilt_type_params_keep_unsubstituted_names() {
        // A value-generic (size) slot is not a type expression, so it stays
        // unmapped and its name survives the rebuild.
        let tc = TypeChecker::new();
        let defs = [generic("T"), generic("Size")];
        let size_expr = IdNode::new(
            0,
            ExpressionKind::Literal(crate::ast::literal::Literal::None),
            Span::new(0, 0),
        );
        let ty = Type::new(
            TypeKind::Custom(
                "Buffer".to_string(),
                Some(vec![
                    tc.create_type_expression(Type::new(TypeKind::Float, Span::new(0, 0))),
                    size_expr,
                ]),
            ),
            Span::new(0, 0),
        );
        let subs = build_class_generic_substitution(&tc, &defs, &ty);
        assert!(subs.contains_key("T"));
        assert!(!subs.contains_key("Size"));
        assert_eq!(
            rebuild_class_type_params(&defs, &subs),
            vec!["Size".to_string()]
        );
    }

    #[test]
    fn non_generic_instantiation_yields_empty_map() {
        let tc = TypeChecker::new();
        let defs = [generic("T")];
        let plain = Type::new(TypeKind::Custom("Box".to_string(), None), Span::new(0, 0));
        assert!(build_class_generic_substitution(&tc, &defs, &plain).is_empty());
    }
}
