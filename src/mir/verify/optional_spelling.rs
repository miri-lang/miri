// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that no lowered body spells the optional by its generic name.
//!
//! `T?` and `Option<T>` are one type, and the type checker hands every later
//! stage the first spelling alone. The layout authority, reference counting
//! and the release plans read an optional only as `T?`: a value still
//! spelled `Option<T>` would be laid out as a generic enum, a counted block
//! with a tag, while every other holder of it reads the optional's own
//! layout, a nullable word or a tagged payload. A body carrying the generic
//! spelling is therefore refused, on every build, rather than compiled with
//! two layouts for one value.
//!
//! The rule shares only its specification with the type checker that
//! normalises the spelling: the identity of the enum marked `@optional`,
//! handed to it by the caller.

use super::body_types::{assigned_rvalues, built_types, mentions};
use super::{local_display_name, VerificationViolation};
use crate::ast::types::{Type, TypeKind};
use crate::mir::{AggregateKind, Body, Local, Rvalue};

/// Every type `body` declares or builds at that spells the optional as
/// `Option<T>`: each local's, its signature included, the instance it is
/// lowered for, each value it constructs or casts, each optional and closure
/// it builds, and each parameter type a virtual call it makes passes; and
/// each variant of the optional it builds as an enum value.
///
/// `optional` is the identity of the enum marked `@optional`.
pub fn verify_optional_spelling(body: &Body, optional: &str) -> Vec<VerificationViolation> {
    let mut found: Vec<VerificationViolation> = body
        .local_decls
        .iter()
        .enumerate()
        .filter(|(_, decl)| spells_generic_optional(&decl.ty.kind, optional))
        .map(|(index, decl)| violation(body, Local(index), &decl.ty, "is declared at"))
        .collect();
    let built = (body.generics.instance.iter())
        .chain(built_types(body))
        .chain(assigned_rvalues(body).filter_map(built_optional_or_closure));
    found.extend(
        built
            .filter(|ty| spells_generic_optional(&ty.kind, optional))
            .map(|ty| violation(body, Local(0), ty, "builds a value at")),
    );
    found.extend(
        assigned_rvalues(body)
            .filter_map(|rvalue| optional_variant_built_as_enum(rvalue, optional))
            .map(|variant| variant_violation(body, optional, variant)),
    );
    found
}

/// The variant `rvalue` builds as an enum value when the enum is the
/// optional: its variants are built as `Some(x)` and `None`, in the
/// optional's own layout, never as a generic enum's tagged block.
fn optional_variant_built_as_enum<'r>(rvalue: &'r Rvalue, optional: &str) -> Option<&'r str> {
    let Rvalue::Aggregate(AggregateKind::Enum(enum_name, variant), _) = rvalue else {
        return None;
    };
    (&**enum_name == optional).then_some(&**variant)
}

fn variant_violation(body: &Body, optional: &str, variant: &str) -> VerificationViolation {
    VerificationViolation {
        local: Local(0),
        local_name: local_display_name(body, Local(0)),
        message: format!(
            "`{}` builds the variant `{optional}.{variant}` as an enum value: the \
             optional's variants are built in the optional's own layout",
            Local(0)
        ),
    }
}

/// The type of the optional or the closure `rvalue` builds, when it builds
/// one: a closure's type is the signature a call through it is passed by.
fn built_optional_or_closure(rvalue: &Rvalue) -> Option<&Type> {
    let Rvalue::Aggregate(AggregateKind::Option(ty) | AggregateKind::Closure(_, ty), _) = rvalue
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
            "`{local}` {what} `{}`, which spells the optional by its generic name: the type \
             checker hands lowering every optional as `T?`",
            ty.kind
        ),
    }
}

/// Whether `kind` names the optional by its generic name anywhere in it.
fn spells_generic_optional(kind: &TypeKind, optional: &str) -> bool {
    mentions(
        kind,
        &|kind: &TypeKind| matches!(kind, TypeKind::Custom(name, _) if name == optional),
    )
}
