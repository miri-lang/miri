// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which values a write copies first when another name still holds them.
//!
//! A struct, a tuple and a `List`, `Map`, `Set` or `Array` are values that a
//! second binding, an argument or a capture shares by reference until either
//! writes; the write first makes the value its own. Lowering asks how each
//! value on a written path is made its own, and the verifier asks which
//! parameters hold such a value, from this one rule.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::Body;
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::utils::ResourceTypes;

/// How a value on a write's path is made its own before the write.
pub enum Unsharing {
    /// A struct held as a reference-counted allocation, with this many
    /// fields: rebuilt from its fields.
    Struct(usize),
    /// A tuple held as a reference-counted allocation, with this many
    /// elements: rebuilt from its elements.
    Tuple(usize),
    /// A built-in collection: cloned by the runtime.
    Collection(BuiltinCollectionKind),
}

/// How a value of type `ty` is made its own before a write, or `None` when a
/// value of that type is never shared or is shared by design.
///
/// A value held as its bytes is copied whenever a binding, a field, an element
/// or a closure takes it, so no second holder of it ever exists. Any other
/// struct is a reference-counted allocation that a copy shares, and a write
/// copies it first.
pub fn unsharing(body: &Body, ty: &Type) -> Option<Unsharing> {
    if let Some(kind) = ty.kind.as_builtin_collection() {
        return Some(Unsharing::Collection(kind));
    }
    if body.value_layout(&ty.kind).is_some() {
        return None;
    }
    if let TypeKind::Tuple(elements) = &ty.kind {
        return Some(Unsharing::Tuple(elements.len()));
    }
    let TypeKind::Custom(name, _) = &ty.kind else {
        return None;
    };
    let Some(TypeDefinition::Struct(definition)) =
        body.rc_classifier.definitions().get(name.as_str())
    else {
        return None;
    };
    Some(Unsharing::Struct(definition.fields.len()))
}

/// Whether a value of type `ty`, held in `body`, is shared between the names
/// holding it until one writes it, and copied by that write.
pub fn is_copied_on_write(body: &Body, ty: &Type) -> bool {
    unsharing(body, ty).is_some()
}

/// Whether a value of type `ty`, held in `body`, is a class instance that a
/// second binding shares and that a write through either name changes for
/// both: one that is no resource (`resources` says which types are) and
/// that a write does not copy. A resource has one owner, moved on
/// assignment, and a collection is copied on write, so neither is.
///
/// Class instances are shared on assignment today, so a slot inside one may
/// be reached through another binding while a call writes it.
pub fn is_shared_on_assignment(body: &Body, ty: &Type, resources: &ResourceTypes) -> bool {
    let TypeKind::Custom(name, _) = &ty.kind else {
        return false;
    };
    let definition = body.rc_classifier.definitions().get(name.as_str());
    matches!(definition, Some(TypeDefinition::Class(_)))
        && body.is_counted(&ty.kind)
        && !is_copied_on_write(body, ty)
        && !resources.holds(&ty.kind)
}
