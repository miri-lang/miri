// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every type a body spells names only what is bound in it.
//!
//! A body is lowered at its instantiation's arguments, so every type it
//! declares a local at, builds an aggregate at, casts to or writes a constant
//! at names a type the program defines, a built-in collection, `Self`, or a
//! parameter the body leaves open — a shared generic body leaves its own
//! parameters open, and is checked against them. A name that is none of these
//! is a parameter of some other declaration that missed its substitution:
//! `U?` inside `main`, from a base class's field read at the child's
//! parameters. Such a type has no layout anyone can agree on, so it is refused
//! here rather than laid out by guess.
//!
//! One spelling is not a missed substitution: an enum argument inference never
//! settled. `Result.Ok(1.5)` with nothing fixing the error type is written
//! `Result<float, E>`, the argument left at the enum's own parameter name in
//! its own position; no value of that variant exists. That position is bound
//! where it is written (see [`PlaceTypes::binds_every_name`]), and so is the
//! local a match arm binds out of such a value at bare `E` — that arm never
//! runs. The name is bound nowhere else.

use super::{local_display_name, VerificationViolation};
use crate::ast::expression::ExpressionKind;
use crate::ast::types::{Type, TypeKind};
use crate::mir::place_type::PlaceTypes;
use crate::mir::rvalue::{AggregateKind, Rvalue};
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::{Body, Local, Operand};
use crate::type_checker::context::TypeDefinition;
use std::collections::{HashMap, HashSet};

/// Every type `body` spells that names a parameter the body neither leaves
/// open nor the program defines.
pub fn verify_bound_names(body: &Body) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let slots = PlaceTypes::of(body);
    let unsettled = unsettled_bindings(body, &slots);
    let unbound = |ty: &Type| !slots.binds_every_name(ty);
    let mut found: Vec<VerificationViolation> = body
        .local_decls
        .iter()
        .enumerate()
        .filter(|(index, decl)| unbound(&decl.ty) && !unsettled.contains(&Local(*index)))
        .map(|(index, decl)| violation(body, Local(index), "is declared at", &decl.ty))
        .collect();
    let statements = body.basic_blocks.iter().flat_map(|block| &block.statements);
    for (dest, rvalue) in statements.filter_map(assignment) {
        found.extend(
            spelled_types(rvalue)
                .into_iter()
                .filter(|ty| unbound(ty))
                .map(|ty| violation(body, dest, "is written from a value spelled", ty)),
        );
    }
    found
}

/// The local `statement` writes and the value it writes, if it assigns one.
fn assignment(statement: &Statement) -> Option<(Local, &Rvalue)> {
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
        &statement.kind
    else {
        return None;
    };
    Some((dest.local, rvalue))
}

/// The locals a match arm binds out of an enum value whose arguments
/// inference left unsettled, each declared at a type only those arguments
/// leave unbound.
fn unsettled_bindings(body: &Body, slots: &PlaceTypes) -> HashSet<Local> {
    let statements = body.basic_blocks.iter().flat_map(|block| &block.statements);
    statements
        .filter_map(assignment)
        .filter_map(|(dest, rvalue)| {
            let Rvalue::Use(Operand::Copy(source) | Operand::Move(source)) = rvalue else {
                return None;
            };
            let source_ty = &body.local_decls.get(source.local.0)?.ty;
            let unsettled = own_parameter_arguments(&source_ty.kind, slots.definitions);
            if source.projection.is_empty() || unsettled.is_empty() {
                return None;
            }
            let open: HashSet<String> =
                slots.open_params.iter().cloned().chain(unsettled).collect();
            let widened = PlaceTypes {
                open_params: &open,
                ..PlaceTypes::of(body)
            };
            widened
                .binds_every_name(&body.local_decls.get(dest.0)?.ty)
                .then_some(dest)
        })
        .collect()
}

/// The arguments of the enum application `kind` that are spelled as the
/// enum's own parameter at their position.
fn own_parameter_arguments(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<String> {
    let TypeKind::Custom(name, Some(args)) = kind else {
        return Vec::new();
    };
    let Some(TypeDefinition::Enum(definition)) = definitions.get(name) else {
        return Vec::new();
    };
    let params = definition
        .generics
        .iter()
        .flatten()
        .map(|g| g.name.as_str());
    params
        .zip(args)
        .filter(|(param, arg)| bare_name(arg) == Some(*param))
        .map(|(param, _)| param.to_string())
        .collect()
}

/// The name a bare written type argument spells, parameter or type.
fn bare_name(arg: &crate::ast::expression::Expression) -> Option<&str> {
    let ExpressionKind::Type(ty, _) = &arg.node else {
        return None;
    };
    let (TypeKind::Custom(name, None) | TypeKind::Generic(name, None, _)) = &ty.kind else {
        return None;
    };
    Some(name)
}

/// The types `rvalue` spells itself: an aggregate's, a cast's target, a
/// constant's.
fn spelled_types(rvalue: &Rvalue) -> Vec<&Type> {
    let mut types = Vec::new();
    match rvalue {
        Rvalue::Aggregate(kind, operands) => {
            match kind {
                AggregateKind::Option(ty)
                | AggregateKind::Tuple(ty)
                | AggregateKind::Struct(ty)
                | AggregateKind::Class(ty)
                | AggregateKind::Closure(_, ty) => types.push(ty),
                AggregateKind::Enum(_, _)
                | AggregateKind::Array
                | AggregateKind::List
                | AggregateKind::Set
                | AggregateKind::Map
                | AggregateKind::FormattedString => {}
            }
            types.extend(operands.iter().filter_map(constant_type));
        }
        Rvalue::Cast(operand, ty) => {
            types.push(ty);
            types.extend(constant_type(operand));
        }
        Rvalue::Use(operand) => types.extend(constant_type(operand)),
        Rvalue::BinaryOp(_, lhs, rhs) => {
            types.extend(constant_type(lhs));
            types.extend(constant_type(rhs));
        }
        Rvalue::UnaryOp(_, operand) => types.extend(constant_type(operand)),
        Rvalue::Ref(_)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => {}
    }
    types
}

/// The type a constant operand is written at.
fn constant_type(operand: &Operand) -> Option<&Type> {
    match operand {
        Operand::Constant(constant) => Some(&constant.ty),
        Operand::Copy(_) | Operand::Move(_) | Operand::Function(_) => None,
    }
}

fn violation(body: &Body, local: Local, how: &str, ty: &Type) -> VerificationViolation {
    VerificationViolation {
        local,
        local_name: local_display_name(body, local),
        message: format!(
            "`{local}` {how} `{}`, which names a parameter this body neither leaves open nor \
             the program defines",
            ty.kind
        ),
    }
}
