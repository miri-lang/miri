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
//! The walk over a type is the verifiers' own, cruder than the type
//! checker's: it looks for the keyword's spelling anywhere a type can hold
//! another.

use super::body_types::{built_types, mentions};
use super::{local_display_name, VerificationViolation};
use crate::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use crate::mir::{Body, Local};

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
    mentions(kind, &|kind: &TypeKind| {
        matches!(
            kind,
            TypeKind::Generic(name, _) | TypeKind::Custom(name, None) if name == SELF_TYPE_NAME
        )
    })
}
