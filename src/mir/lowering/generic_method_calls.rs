// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Calls to a method declaring type parameters of its own, `fn m<T>(x T) T`.
//!
//! Such a method has no body shared by every type argument and no vtable
//! slot: each call names the copy compiled for the receiver's owner at its
//! type arguments and for the method's own at the call's, and records it so
//! the pipeline lowers that copy. The method's arguments are read only from
//! the call's recorded mapping, with the calling body's substitution applied.

use super::dispatch::refuse_unnameable_call_mapping;
use super::method_dispatch::{instantiated_callee, owner_substitution};
use super::requests::GenericMethodCall;
use super::{Lowered, LoweringContext, SharedCompilationIds};
use crate::ast::statement::Statement;
use crate::ast::Type;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::Symbol;
use crate::type_checker::context::{GenericDefinition, TypeDefinition};
use crate::type_checker::TypeChecker;

/// A call to a generic method, its receiver and target resolved.
pub(super) struct GenericMethodSite<'a> {
    /// The class or enum the receiver is, at the arguments it is reached at;
    /// `None` when its arguments have no monomorphized spelling.
    pub receiver: Option<(String, Vec<Type>)>,
    /// The type the receiver dispatches on.
    pub class_name: &'a str,
    /// The type whose body a static call on the receiver names.
    pub defining_class: &'a str,
    pub method_name: &'a str,
    /// The type parameters the method declares where its body is written.
    pub parameters: &'a [GenericDefinition],
    /// Whether the method is static, and so reads none of its owner's
    /// parameters.
    pub is_static: bool,
    pub call_expr_id: usize,
    pub span: Span,
}

/// The symbol of the copy of a generic method `site` calls, recorded on the
/// body so the pipeline lowers it.
///
/// A call with no recorded type arguments is an internal error, and so is
/// one on a receiver of a trait type. The type checker refuses both ways a
/// trait type could reach a receiver here (`MER_TYP_087`): a call through a
/// value of the trait type, and a site binding a type parameter a body calls
/// a generic method through to the trait type. A body still open at a
/// bounded parameter is compiled per type, never shared. A call pinning a
/// parameter to a type with no name to compile a copy at is refused.
pub(super) fn generic_method_symbol(
    ctx: &mut LoweringContext,
    site: &GenericMethodSite,
) -> Result<Symbol, LoweringError> {
    if is_trait(ctx, site.class_name) {
        return Err(generic_method_through_trait(site));
    }
    let Some(type_args) = ctx.instantiated_call_mapping(site.call_expr_id) else {
        return Err(uninstantiated_generic_method_call(site));
    };
    let type_args = bound_by_position(site.method_name, site.parameters, type_args, site.span)?;
    refuse_unnameable_call_mapping(ctx, site.method_name, &type_args, site.span)?;
    let (owner, owner_args) = generic_method_owner(ctx, site)?;
    let symbol = Symbol::method(
        &owner,
        &owner_args,
        site.method_name,
        type_args.iter().map(|(_, ty)| ty),
    );
    ctx.requests.generic_method_calls.push(GenericMethodCall {
        symbol: symbol.clone(),
        owner,
        owner_args,
        method: site.method_name.to_string(),
        type_args,
    });
    Ok(symbol)
}

/// The type a generic method's copy is compiled under, with its arguments:
/// the class a generic receiver reaches the method's body at — its own, or
/// the ancestor declaring it, at what the `extends` chain maps the receiver's
/// arguments to — else the type declaring the method, when its body reads
/// none of that type's parameters: the type declares none, or the method is
/// static. A body that would read its owner's parameters at no arguments is
/// an internal error.
fn generic_method_owner(
    ctx: &LoweringContext,
    site: &GenericMethodSite,
) -> Result<(String, Vec<Type>), LoweringError> {
    let definitions = ctx.type_checker.type_definitions();
    let reached = site
        .receiver
        .as_ref()
        .and_then(|(name, args)| instantiated_callee(definitions, name, args, site.method_name))
        .map(|callee| (callee.owner, callee.owner_args))
        .or_else(|| default_compiled_into_receiver(ctx, site));
    if let Some(reached) = reached {
        return Ok(reached);
    }
    let declares_parameters = definitions
        .get(site.defining_class)
        .and_then(TypeDefinition::generics)
        .is_some_and(|generics| !generics.is_empty());
    if declares_parameters && !site.is_static {
        return Err(generic_method_owner_unresolved(site));
    }
    Ok((site.defining_class.to_string(), Vec::new()))
}

/// The receiver's class and arguments when the method is a trait's default
/// the class inherits: the default is compiled into the class that runs it,
/// an abstract one included, as a concrete class's is. `None` otherwise.
fn default_compiled_into_receiver(
    ctx: &LoweringContext,
    site: &GenericMethodSite,
) -> Option<(String, Vec<Type>)> {
    let (name, args) = site.receiver.as_ref()?;
    is_trait(ctx, site.defining_class).then(|| (name.clone(), args.clone()))
}

/// A call's type arguments keyed by `parameters`, the type parameters of the
/// method whose body runs, matched by position. A call through a type
/// parameter bounded by a trait binds the trait method's parameters, which
/// the implementing class may name otherwise: `fn pick<U>` implements
/// `fn pick<T>`. A mapping of another length is an internal error: the type
/// checker binds every parameter a call's method declares.
pub(super) fn bound_by_position(
    method: &str,
    parameters: &[GenericDefinition],
    type_args: Vec<(String, Type)>,
    span: Span,
) -> Result<Vec<(String, Type)>, LoweringError> {
    if parameters.len() != type_args.len() {
        return Err(LoweringError::internal(
            DiagnosticCode::MirInvalidInstantiationArgument,
            format!(
                "the call to the generic method `{method}` binds {} type arguments, but the \
                 body it runs declares {} type parameters",
                type_args.len(),
                parameters.len()
            ),
            span,
        ));
    }
    Ok(parameters
        .iter()
        .zip(type_args)
        .map(|(parameter, (_, ty))| (parameter.name.clone(), ty))
        .collect())
}

/// Whether `name` registers a trait.
fn is_trait(ctx: &LoweringContext, name: &str) -> bool {
    matches!(
        ctx.type_checker.type_definitions().get(name),
        Some(TypeDefinition::Trait(_))
    )
}

/// The internal error for a generic method call that reached lowering on a
/// receiver of a trait type, which only a vtable could answer.
fn generic_method_through_trait(site: &GenericMethodSite) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!(
            "the call to the generic method `{}` reached lowering on a receiver of the \
             trait `{}`, which no vtable slot answers",
            site.method_name, site.class_name
        ),
        site.span,
    )
}

/// The internal error for a call to a generic method of a type declaring
/// parameters, on a receiver whose arguments name no instantiation of it.
fn generic_method_owner_unresolved(site: &GenericMethodSite) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!(
            "the call to the generic method `{}` of `{}` reached lowering with no type \
             arguments for `{}` to compile its copy at",
            site.method_name, site.defining_class, site.defining_class
        ),
        site.span,
    )
}

/// The internal error for a generic method call that reached lowering with
/// no instantiation recorded for it.
fn uninstantiated_generic_method_call(site: &GenericMethodSite) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!(
            "the call to the generic method `{}` reached lowering with no instantiation \
             recorded for it",
            site.method_name
        ),
        site.span,
    )
}

/// Lower the copy of a generic method `call` names from `method`, the
/// statement whose body it runs: `self` typed as the owner at its arguments,
/// the owner's parameters read at those arguments — a trait default's at
/// what the owner's clauses pin them to — and the method's own at what the
/// call binds.
pub fn lower_generic_method_copy(
    method: &Statement,
    call: &GenericMethodCall,
    tc: &TypeChecker,
    is_release: bool,
    compilation_ids: SharedCompilationIds,
) -> Result<Lowered, LoweringError> {
    let owner_subs = owner_substitution(tc.type_definitions(), &call.owner, &call.owner_args);
    let self_type = super::monomorphized_self_type(&call.owner, tc, &owner_subs, method.span);
    let mut subs = super::dispatch_symbols::instantiation_substitution(
        tc,
        &call.owner,
        &call.method,
        &owner_subs,
    )
    .into_owned();
    subs.extend(call.type_args.iter().cloned());
    super::lower_class_method_at_with_compilation_ids(
        method,
        self_type,
        tc,
        is_release,
        &subs,
        compilation_ids,
    )
}
