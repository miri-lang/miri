// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether the values of a type are shared by reference.
//!
//! An instance of a class is one object every binding holding it names, so a
//! write through one binding is seen through all of them. A class whose chain
//! of `extends` reaches a built-in collection copies on write, and so does
//! `String`, which appends in place only when nothing else holds it: both are
//! values, as structs, enums, tuples, optionals, scalars and the built-in
//! collections are. A trait-typed value is an instance of one of the classes
//! implementing the trait, directly, through a trait extending it, or through
//! a class it extends. A type parameter, or a name the table does not hold,
//! may stand for either.
//!
//! A type is read by the name it carries, which is its identity: a type the
//! checker records names a module-private type by the identity that type was
//! registered under, so two same-named types never meet here.

use std::collections::HashMap;

use crate::ast::types::{BuiltinCollectionKind, TypeKind, STRING_TYPE_NAME};
use crate::type_checker::context::{class_ancestry, class_implements_trait, TypeDefinition};

/// How the values of a type are shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sharing {
    /// Every value is a reference to an object other bindings may hold.
    Reference,
    /// Every value is a value: a write never reaches another binding's.
    Value,
    /// A value may be either: a type parameter, a trait implemented by both
    /// kinds of class or by none, or a name the table does not hold.
    Either,
}

/// How the values of the type `kind` are shared, its definitions read from
/// `definitions`.
pub fn sharing(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> Sharing {
    if matches!(kind, TypeKind::Generic(..)) {
        return Sharing::Either;
    }
    let TypeKind::Custom(name, _) = kind else {
        return Sharing::Value;
    };
    if kind.as_builtin_collection().is_some() {
        return Sharing::Value;
    }
    match definitions.get(name) {
        Some(TypeDefinition::Class(_)) => class_sharing(name, definitions),
        Some(TypeDefinition::Trait(_)) => trait_sharing(name, definitions),
        Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_) | TypeDefinition::Alias(_)) => {
            Sharing::Value
        }
        Some(TypeDefinition::Generic(_)) | None => Sharing::Either,
    }
}

/// Whether the type named `name` copies on write: a built-in collection or
/// `String`.
fn copies_on_write(name: &str) -> bool {
    BuiltinCollectionKind::from_name(name).is_some() || name == STRING_TYPE_NAME
}

/// A reference unless the class `name`, or a class it extends, copies on
/// write. A base that is not itself a class (a collection or `String` the
/// table does not hold as one) is read by its name.
fn class_sharing(name: &str, definitions: &HashMap<String, TypeDefinition>) -> Sharing {
    let copies = class_ancestry(name, definitions).any(|(class, definition)| {
        copies_on_write(class)
            || definition
                .base_class
                .as_deref()
                .is_some_and(copies_on_write)
    });
    if copies {
        Sharing::Value
    } else {
        Sharing::Reference
    }
}

/// What every class implementing the trait `name` agrees on; `Either` when
/// they disagree or no class implements it.
fn trait_sharing(name: &str, definitions: &HashMap<String, TypeDefinition>) -> Sharing {
    let mut implementers = definitions.iter().filter_map(|(class, definition)| {
        let TypeDefinition::Class(_) = definition else {
            return None;
        };
        class_implements_trait(class, name, definitions).then(|| class_sharing(class, definitions))
    });
    let Some(first) = implementers.next() else {
        return Sharing::Either;
    };
    if implementers.all(|sharing| sharing == first) {
        first
    } else {
        Sharing::Either
    }
}
