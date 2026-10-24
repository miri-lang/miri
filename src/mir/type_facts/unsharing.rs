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
/// copies it first. A vector is never shared either: a collection holds it
/// inline, as its components' bytes, and a copy built for it would be stored
/// by value and never released.
pub fn unsharing(body: &Body, ty: &Type) -> Option<Unsharing> {
    if let Some(kind) = ty.kind.as_builtin_collection() {
        return Some(Unsharing::Collection(kind));
    }
    if crate::ast::types::vec_type_dim(&ty.kind).is_some() || body.value_layout(&ty.kind).is_some()
    {
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
