// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The type algebra of generic instantiation: what a type argument written in
//! the source denotes, how a substitution of generic parameters rewrites a
//! type, what a member declared in a generic owner's parameters is at one
//! instantiation, and which type arguments get a body compiled for them.
//!
//! MIR lowering and code generation both ask these questions about the same
//! instantiation. Each rule lives here once, so the type a field is stored at
//! when lowering decides how to release it and the type codegen lays it out
//! and drops it at cannot drift apart.

pub(crate) mod inherited;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::type_checker::context::TypeDefinition;
use std::collections::HashMap;

/// Apply a generic substitution mapping to an Expression that contains a type,
/// replacing generic parameters in the wrapped type.
fn substitute_in_type_expr(expr: &Expression, subs: &HashMap<String, Type>) -> Expression {
    match &expr.node {
        ExpressionKind::Type(ty, is_ref) => {
            let new_ty = apply_generic_sub(ty, subs);
            Expression {
                id: expr.id,
                span: expr.span,
                node: ExpressionKind::Type(Box::new(new_ty), *is_ref),
            }
        }
        // A type argument written as a bare name (`List<T>`) parses as an
        // identifier rather than a resolved type, so it needs the same
        // substitution: without it the argument keeps naming the generic
        // parameter and the instantiation is never recognised as concrete.
        ExpressionKind::Identifier(name, _) => match subs.get(name.as_str()) {
            Some(concrete) => Expression {
                id: expr.id,
                span: expr.span,
                node: ExpressionKind::Type(Box::new(concrete.clone()), false),
            },
            None => expr.clone(),
        },
        // A value argument computed from the parameters (`Size + 1`) names one
        // instantiation once they are bound; folded, it is spelled like any
        // other value argument. One that does not fold stays as written, and
        // constructor lowering refuses the instance it would build.
        ExpressionKind::Binary(..) | ExpressionKind::Unary(..) => {
            crate::type_checker::generics::fold_value_generic_arithmetic(expr, subs)
                .unwrap_or_else(|| expr.clone())
        }
        _ => expr.clone(),
    }
}

/// The type a member declared as `field_ty` has inside an instance whose type
/// arguments are `args`, for a type declaring the parameters `params` — a class
/// or struct field, or an enum variant's payload.
///
/// Such a member is declared in the owner's own parameters (`value T`,
/// `items List<T>`), which name nothing concrete. Substituting the whole member
/// type, not only a bare parameter, is what reaches a type nested inside it —
/// an element type inside a collection field, or the argument of another
/// generic type the member is declared at. An argument that is a value rather
/// than a type (the size of a value generic) leaves its parameter
/// unsubstituted, and a nullable argument (`T` at `int?`) substitutes as the
/// option it denotes.
// TODO: a class declaring both a value parameter and a type parameter — a
// `Buf<T, Size>` holding `Array<T, Size>` alongside a bare `T` — leaks the bare
// field's value at a managed instantiation, while either field alone is
// balanced. Whether the value argument being dropped here misaligns the pairing
// for the parameters that follow it is unproven; the codegen drop thunk zips the
// same parameters against resolved types of its own and must agree.
pub(crate) fn instantiated_member_type<'a>(
    params: impl IntoIterator<Item = &'a str>,
    args: &[Expression],
    field_ty: &Type,
) -> Type {
    let subs: HashMap<String, Type> = params
        .into_iter()
        .zip(args)
        .filter_map(|(param, arg)| Some((param.to_string(), type_argument(arg)?)))
        .collect();
    apply_generic_sub(field_ty, &subs)
}

/// The type a type argument denotes, or `None` when the argument is a value
/// rather than a type (the size of a value generic).
///
/// A nullable argument (`T` at `int?`) denotes the option it stands for, so it
/// is folded into `Option` here rather than by every caller.
pub(crate) fn type_argument(arg: &Expression) -> Option<Type> {
    let ExpressionKind::Type(arg_ty, is_nullable) = &arg.node else {
        return None;
    };
    Some(if *is_nullable {
        Type::new(TypeKind::Option(arg_ty.clone()), arg_ty.span)
    } else {
        (**arg_ty).clone()
    })
}

/// Every argument of `args` as the type it denotes (see [`type_argument`]), or
/// `None` when any of them is a value rather than a type.
pub(crate) fn type_arguments(args: &[Expression]) -> Option<Vec<Type>> {
    args.iter().map(type_argument).collect()
}

/// One argument of a generic-class reference as the instantiation registry
/// records it and every per-instantiation symbol is mangled from: the type
/// [`type_argument`] reads, or the marker standing for a value generic. `None`
/// when the argument is neither.
///
/// A static call, a vtable and a drop thunk all name an instantiation from
/// what this answers, so they cannot spell one instantiation two ways.
pub(crate) fn instantiation_argument(arg: &Expression) -> Option<Type> {
    type_argument(arg).or_else(|| crate::type_checker::generics::value_generic_slot(arg))
}

/// The type arguments `arg_exprs` spell for a generic class of `arity`
/// parameters, or `None` when they name no per-instantiation body: an argument
/// that is neither a type nor a value, a count that disagrees, or an argument
/// [`is_monomorphized_instantiation`] rejects.
pub(crate) fn monomorphized_arguments(
    arg_exprs: &[Expression],
    arity: usize,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> Option<Vec<Type>> {
    let args: Vec<Type> = arg_exprs
        .iter()
        .map(instantiation_argument)
        .collect::<Option<_>>()?;
    is_monomorphized_instantiation(&args, arity, type_definitions).then_some(args)
}

/// Whether `args` fill a generic class's `arity` parameters with arguments a
/// per-instantiation body is compiled at.
pub(crate) fn is_monomorphized_instantiation(
    args: &[Type],
    arity: usize,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    args.len() == arity
        && args
            .iter()
            .all(|arg| is_monomorphizable_type_argument(&arg.kind, type_definitions))
}

/// Apply a generic substitution mapping to a `Type`, replacing generic parameters
/// with their concrete counterparts. Exhaustively handles all `TypeKind` variants
/// that can contain nested types: recursively substitutes in `Option<T>`, `List<T>`,
/// `Custom<T>`, `Tuple`, `Result<T, E>`, `Function`, `Meta<T>`, `Linear<T>`,
/// `Generic` bounds, and all other type constructors.
///
/// Handles two representations that appear in `resolve_type` output:
/// - `TypeKind::Generic("T", ...)` - explicit generic placeholder
/// - `TypeKind::Custom("T", None)` - generic param written as a plain identifier
pub(crate) fn apply_generic_sub(ty: &Type, subs: &HashMap<String, Type>) -> Type {
    if subs.is_empty() {
        return ty.clone();
    }

    match &ty.kind {
        // Direct generic: replace with concrete type if in map
        TypeKind::Generic(name, bound, decl_kind) => {
            if let Some(concrete) = subs.get(name) {
                concrete.clone()
            } else {
                // Bound itself might contain generics
                let new_bound = bound.as_ref().map(|b| Box::new(apply_generic_sub(b, subs)));
                Type::new(
                    TypeKind::Generic(name.clone(), new_bound, *decl_kind),
                    ty.span,
                )
            }
        }
        // Custom type with no args: replace if it's a generic param name
        TypeKind::Custom(name, None) => {
            if let Some(concrete) = subs.get(name.as_str()) {
                concrete.clone()
            } else {
                ty.clone()
            }
        }
        // Custom type with args: substitute in each arg expression
        TypeKind::Custom(name, Some(args)) => {
            let new_args = args
                .iter()
                .map(|arg| substitute_in_type_expr(arg, subs))
                .collect();
            Type::new(TypeKind::Custom(name.clone(), Some(new_args)), ty.span)
        }
        // Type wrappers that directly contain Type (not Expression)
        TypeKind::Option(inner) => {
            let new_inner = apply_generic_sub(inner, subs);
            Type::new(TypeKind::Option(Box::new(new_inner)), ty.span)
        }
        TypeKind::Meta(inner) => {
            let new_inner = apply_generic_sub(inner, subs);
            Type::new(TypeKind::Meta(Box::new(new_inner)), ty.span)
        }
        TypeKind::Linear(inner) => {
            let new_inner = apply_generic_sub(inner, subs);
            Type::new(TypeKind::Linear(Box::new(new_inner)), ty.span)
        }
        // Type constructors that use Expression (parser-only or container variants)
        TypeKind::List(expr) => {
            let new_expr = substitute_in_type_expr(expr, subs);
            Type::new(TypeKind::List(Box::new(new_expr)), ty.span)
        }
        TypeKind::Array(elem_expr, size_expr) => {
            let new_elem = substitute_in_type_expr(elem_expr, subs);
            Type::new(
                TypeKind::Array(Box::new(new_elem), size_expr.clone()),
                ty.span,
            )
        }
        TypeKind::Map(key_expr, val_expr) => {
            let new_key = substitute_in_type_expr(key_expr, subs);
            let new_val = substitute_in_type_expr(val_expr, subs);
            Type::new(TypeKind::Map(Box::new(new_key), Box::new(new_val)), ty.span)
        }
        TypeKind::Set(elem_expr) => {
            let new_elem = substitute_in_type_expr(elem_expr, subs);
            Type::new(TypeKind::Set(Box::new(new_elem)), ty.span)
        }
        TypeKind::Tuple(exprs) => {
            let new_exprs = exprs
                .iter()
                .map(|expr| substitute_in_type_expr(expr, subs))
                .collect();
            Type::new(TypeKind::Tuple(new_exprs), ty.span)
        }
        TypeKind::Result(ok_expr, err_expr) => {
            let new_ok = substitute_in_type_expr(ok_expr, subs);
            let new_err = substitute_in_type_expr(err_expr, subs);
            Type::new(
                TypeKind::Result(Box::new(new_ok), Box::new(new_err)),
                ty.span,
            )
        }
        TypeKind::Future(expr) => {
            let new_expr = substitute_in_type_expr(expr, subs);
            Type::new(TypeKind::Future(Box::new(new_expr)), ty.span)
        }
        TypeKind::Function(fdata) => {
            use crate::ast::common::Parameter as AstParameter;
            let new_generics = fdata.generics.as_ref().map(|gens| {
                gens.iter()
                    .map(|gen| substitute_in_type_expr(gen, subs))
                    .collect()
            });
            let new_params = fdata
                .params
                .iter()
                .map(|p| AstParameter {
                    typ: Box::new(substitute_in_type_expr(&p.typ, subs)),
                    ..p.clone()
                })
                .collect();
            let new_return_type = fdata
                .return_type
                .as_ref()
                .map(|rt| Box::new(substitute_in_type_expr(rt, subs)));
            let new_fdata = crate::ast::types::FunctionTypeData {
                generics: new_generics,
                params: new_params,
                return_type: new_return_type,
            };
            Type::new(TypeKind::Function(Box::new(new_fdata)), ty.span)
        }
        // Leaf types that cannot contain generics: clone as-is
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
        | TypeKind::Error => ty.clone(),
    }
}

/// True for a generic-class type argument that gets a per-instantiation body.
///
/// Adds `String` to the scalar set. A managed type argument needs the
/// instantiated body for a different reason than a scalar does: not to pin a
/// store width, but so reference counting sees the concrete type. In a body
/// that still spells the argument `T`, an unresolved type parameter counts as
/// unmanaged, so a value stored into a field is never retained and the holder
/// releases a reference it never took.
///
/// A named type is admitted only when the type table defines it. The same
/// spelling carries a generic parameter still awaiting substitution (`T` reaches
/// here as `Custom("T", None)`), and monomorphizing at a placeholder would name
/// a body no call site can reach.
///
/// A type built out of others — an instantiated generic class, a nested
/// collection, an optional, a tuple — is admitted on its spelling alone. Its
/// components are what the body addresses, and the mangler now names each of
/// them, so two such arguments that differ anywhere get two symbols.
pub(crate) fn is_monomorphizable_type_argument(
    kind: &TypeKind,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    if !has_a_monomorphized_spelling(kind) {
        return false;
    }
    let TypeKind::Custom(name, None) = kind else {
        return true;
    };
    matches!(
        type_definitions.get(name),
        Some(TypeDefinition::Class(_) | TypeDefinition::Struct(_) | TypeDefinition::Enum(_))
    )
}

/// Whether the symbol mangler has a spelling for `kind` at all.
///
/// A necessary condition for [`is_monomorphizable_type_argument`], and the part
/// of it that needs no type table: an unresolved generic parameter and a
/// closure type declaring type parameters of its own have no token, and
/// neither does anything built out of one.
/// Whether a name that *has* a spelling also denotes a type worth
/// monomorphizing is the table's answer, not this one's.
pub(crate) fn has_a_monomorphized_spelling(kind: &TypeKind) -> bool {
    crate::mir::symbol::token::kind_has_a_mangled_token(kind)
}
