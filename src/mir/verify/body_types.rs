// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The types a lowered body carries, and a walk over every type nested in
//! one, for the verifiers that refuse a spelling no later stage reads.
//!
//! The walk is the verifiers' own: it shares nothing with the type
//! checker's substitution or the lowering's canonical form, so a spelling
//! they missed is still found.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::type_arguments::{TypeArgKind, TypeArgument};
use crate::ast::types::{Type, TypeKind};
use crate::mir::{AggregateKind, Body, Operand, Rvalue, StatementKind, TerminatorKind};

/// The types `body`'s statements build values at, the constants it assigns,
/// and the types its calls pass arguments at: a virtual call's parameter
/// types, and the type of each constant a direct call passes or calls,
/// the callee's own signature included.
pub(super) fn built_types(body: &Body) -> impl Iterator<Item = &Type> {
    let statements = assigned_rvalues(body).filter_map(rvalue_type);
    let terminators = body
        .basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref());
    let virtual_calls = terminators
        .clone()
        .filter_map(|terminator| {
            let TerminatorKind::VirtualCall { param_types, .. } = &terminator.kind else {
                return None;
            };
            Some(param_types.iter().flatten())
        })
        .flatten();
    let direct_calls = terminators
        .filter_map(|terminator| {
            let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
                return None;
            };
            Some(std::iter::once(func).chain(args).filter_map(constant_type))
        })
        .flatten();
    statements.chain(virtual_calls).chain(direct_calls)
}

/// The type of `operand` when it is a constant.
fn constant_type(operand: &Operand) -> Option<&Type> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    Some(&constant.ty)
}

/// Every rvalue `body` assigns.
pub(super) fn assigned_rvalues(body: &Body) -> impl Iterator<Item = &Rvalue> {
    body.basic_blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| {
            let StatementKind::Assign(_, rvalue) = &statement.kind else {
                return None;
            };
            Some(rvalue)
        })
}

/// The type `rvalue` builds a value at, when it names one.
fn rvalue_type(rvalue: &Rvalue) -> Option<&Type> {
    if let Rvalue::Cast(_, ty) = rvalue {
        return Some(ty);
    }
    if let Rvalue::Use(operand) = rvalue {
        return constant_type(operand);
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

/// Whether `kind`, or any type nested in it, is one `is_spelled` accepts.
pub(super) fn mentions(kind: &TypeKind, is_spelled: &impl Fn(&TypeKind) -> bool) -> bool {
    if is_spelled(kind) {
        return true;
    }
    let argument = |argument: &TypeArgument| argument_mentions(argument, is_spelled);
    match kind {
        TypeKind::Generic(_, bounds) => bounds
            .iter()
            .any(|bound| mentions(&bound.ty.kind, is_spelled)),
        TypeKind::Custom(_, None) => false,
        TypeKind::Custom(_, Some(arguments))
        | TypeKind::Tuple(arguments)
        | TypeKind::OneOf(arguments) => arguments.iter().any(argument),
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
            argument(element)
        }
        TypeKind::Array(first, second)
        | TypeKind::Map(first, second)
        | TypeKind::Result(first, second) => argument(first) || argument(second),
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            mentions(&inner.kind, is_spelled)
        }
        TypeKind::Function(signature) => {
            let written = |expr: &Expression| written_mentions(expr, is_spelled);
            signature.params.iter().any(|param| written(&param.typ))
                || signature.return_type.as_deref().is_some_and(written)
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

fn argument_mentions(argument: &TypeArgument, is_spelled: &impl Fn(&TypeKind) -> bool) -> bool {
    match &argument.node {
        TypeArgKind::Type(ty, _) => mentions(&ty.kind, is_spelled),
        TypeArgKind::Value(_) => false,
    }
}

fn written_mentions(expr: &Expression, is_spelled: &impl Fn(&TypeKind) -> bool) -> bool {
    let ExpressionKind::Type(ty, _) = &expr.node else {
        return false;
    };
    mentions(&ty.kind, is_spelled)
}
