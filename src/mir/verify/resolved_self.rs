// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that no lowered body still spells `Self`.
//!
//! A trait default is checked once, with `Self` a parameter of the trait, and
//! lowered once per class implementing it, every type it declares read at
//! that class. A local left at `Self` names no layout: a field read off it
//! would land at the wrong offset, and reference counting would take it for
//! an uncounted parameter, never retaining or releasing what it holds. A
//! lowering path that missed the substitution is therefore refused, on every
//! build, rather than compiled by guess.
//!
//! The walk over a type is its own, cruder than the type checker's: it looks
//! for the keyword's spelling anywhere a type can hold another.

use super::{local_display_name, VerificationViolation};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::type_arguments::{TypeArgKind, TypeArgument};
use crate::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use crate::mir::{AggregateKind, Body, Local, Rvalue, StatementKind, TerminatorKind};

/// Every type `body` declares or builds at that still spells `Self`: each
/// local's — its return slot and parameters included, so its signature too —
/// the instance it is lowered for, each value it constructs or casts, and
/// each parameter type a virtual call it makes passes.
pub fn verify_resolved_self(body: &Body) -> Vec<VerificationViolation> {
    let mut found: Vec<VerificationViolation> = body
        .local_decls
        .iter()
        .enumerate()
        .filter(|(_, decl)| spells_self(&decl.ty.kind))
        .map(|(index, decl)| violation(body, Local(index), &decl.ty, "is declared at"))
        .collect();
    let built = body.generics.instance.iter().chain(built_types(body));
    found.extend(
        built
            .filter(|ty| spells_self(&ty.kind))
            .map(|ty| violation(body, Local(0), ty, "builds a value at")),
    );
    found
}

/// The types `body`'s statements build values at and its virtual calls
/// pass arguments at.
fn built_types(body: &Body) -> impl Iterator<Item = &Type> {
    let statements = body
        .basic_blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| {
            let StatementKind::Assign(_, rvalue) = &statement.kind else {
                return None;
            };
            rvalue_type(rvalue)
        });
    let virtual_calls = body
        .basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| {
            let TerminatorKind::VirtualCall { param_types, .. } = &terminator.kind else {
                return None;
            };
            Some(param_types.iter().flatten())
        })
        .flatten();
    statements.chain(virtual_calls)
}

/// The type `rvalue` builds a value at, when it names one.
fn rvalue_type(rvalue: &Rvalue) -> Option<&Type> {
    if let Rvalue::Cast(_, ty) = rvalue {
        return Some(ty);
    }
    let Rvalue::Aggregate(
        AggregateKind::Tuple(ty) | AggregateKind::Struct(ty) | AggregateKind::Class(ty),
        _,
    ) = rvalue
    else {
        return None;
    };
    Some(ty)
}

fn violation(body: &Body, local: Local, ty: &Type, what: &str) -> VerificationViolation {
    VerificationViolation {
        local,
        local_name: local_display_name(body, local),
        message: format!(
            "`{local}` {what} `{}`, which still names `Self`: the body was lowered without \
             reading `Self` as the class it runs for",
            ty.kind
        ),
    }
}

/// Whether `kind` spells `Self`, as a parameter or as a bare name, anywhere
/// in it.
fn spells_self(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Generic(name, bounds) => {
            name == SELF_TYPE_NAME || bounds.iter().any(|bound| spells_self(&bound.ty.kind))
        }
        TypeKind::Custom(name, None) => name == SELF_TYPE_NAME,
        TypeKind::Custom(_, Some(arguments))
        | TypeKind::Tuple(arguments)
        | TypeKind::OneOf(arguments) => arguments.iter().any(argument_spells_self),
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
            argument_spells_self(element)
        }
        TypeKind::Array(first, second)
        | TypeKind::Map(first, second)
        | TypeKind::Result(first, second) => {
            argument_spells_self(first) || argument_spells_self(second)
        }
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            spells_self(&inner.kind)
        }
        TypeKind::Function(signature) => {
            signature
                .params
                .iter()
                .any(|param| written_spells_self(&param.typ))
                || signature
                    .return_type
                    .as_deref()
                    .is_some_and(written_spells_self)
        }
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
        | TypeKind::Error => false,
    }
}

fn argument_spells_self(argument: &TypeArgument) -> bool {
    match &argument.node {
        TypeArgKind::Type(ty, _) => spells_self(&ty.kind),
        TypeArgKind::Value(_) => false,
    }
}

fn written_spells_self(expr: &Expression) -> bool {
    let ExpressionKind::Type(ty, _) = &expr.node else {
        return false;
    };
    spells_self(&ty.kind)
}
