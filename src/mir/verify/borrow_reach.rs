// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a borrowed local's source is reached, and whether a type is a
//! reference by design, worked out for the borrow check on its own.
//!
//! The borrow inference answers both questions too. The check does not ask
//! it: a defect shared by the two would pass both, and these answers decide
//! whether a borrow outlives the value it reads. They are derived here from
//! the type definitions and the runtime's copy-on-write table alone.

use std::collections::{HashMap, HashSet};

use crate::ast::types::{BuiltinCollectionKind, TypeKind, STRING_TYPE_NAME};
use crate::mir::place_type::PlaceTypes;
use crate::mir::{Body, Place, PlaceElem};
use crate::runtime_fns::cow_fn;
use crate::type_checker::context::TypeDefinition;

/// How a place is reached from its root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reach {
    /// Every step stays inside the root's own value.
    InValue,
    /// A step goes through a field of a class instance, which every holder
    /// of the instance reaches and may replace.
    ThroughField,
}

/// What a value of some type is, as a container a step leaves.
enum Container {
    /// Its fields are part of it: an optional, a tuple, a struct, an enum.
    FieldsByValue,
    /// Its elements are part of it: a list or an array.
    ElementsByValue,
    /// An instance of a class of the program, other than a collection.
    Instance,
    /// Anything else: a step out of it is not followed.
    Opaque,
}

/// How `place` is reached from its root; `None` for a place with no step,
/// or with a step this check does not follow.
pub(super) fn reach_of(body: &Body, place: &Place) -> Option<Reach> {
    if place.projection.is_empty() {
        return None;
    }
    let types = PlaceTypes::of(body);
    let definitions = body.rc_classifier.definitions();
    let mut reach = Reach::InValue;
    for depth in 0..place.projection.len() {
        let container = Place {
            local: place.local,
            projection: place.projection[..depth].to_vec(),
        };
        let kind = types.declared(&container)?.kind;
        match (&place.projection[depth], container_of(&kind, definitions)) {
            (PlaceElem::Field(_), Container::FieldsByValue)
            | (PlaceElem::Index(_, _), Container::ElementsByValue) => {}
            (PlaceElem::Field(_), Container::Instance) => reach = Reach::ThroughField,
            (PlaceElem::Field(_) | PlaceElem::Index(_, _), _) | (PlaceElem::Deref, _) => {
                return None
            }
        }
    }
    Some(reach)
}

fn container_of(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> Container {
    if let Some(collection) = kind.as_builtin_collection() {
        return match collection {
            BuiltinCollectionKind::List | BuiltinCollectionKind::Array => {
                Container::ElementsByValue
            }
            BuiltinCollectionKind::Map | BuiltinCollectionKind::Set => Container::Opaque,
        };
    }
    if matches!(kind, TypeKind::Option(_) | TypeKind::Tuple(_)) {
        return Container::FieldsByValue;
    }
    let TypeKind::Custom(name, _) = kind else {
        return Container::Opaque;
    };
    match definitions.get(name) {
        Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_)) => Container::FieldsByValue,
        Some(TypeDefinition::Class(_)) => Container::Instance,
        Some(TypeDefinition::Trait(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_))
        | None => Container::Opaque,
    }
}

/// Whether a value of `kind` is a reference by design, never copied on
/// write: an instance of a class no ancestor of which is a collection or a
/// string, or a value of a trait implemented, and only, by such classes.
pub(super) fn is_reference_by_design(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    let TypeKind::Custom(name, _) = kind else {
        return false;
    };
    match definitions.get(name) {
        Some(TypeDefinition::Class(_)) => is_reference_class(name, definitions),
        Some(TypeDefinition::Trait(_)) => {
            let implementers = implementers_of(name, definitions);
            !implementers.is_empty()
                && (implementers.iter()).all(|class| is_reference_class(class, definitions))
        }
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

/// The class `name` and every class it extends, nearest first, each once.
fn ancestry<'d>(name: &'d str, definitions: &'d HashMap<String, TypeDefinition>) -> Vec<&'d str> {
    let mut chain = vec![name];
    while let Some(Some(TypeDefinition::Class(class))) = chain.last().map(|c| definitions.get(*c)) {
        match class.base_class.as_deref() {
            Some(base) if !chain.contains(&base) => chain.push(base),
            Some(_) | None => break,
        }
    }
    chain
}

/// Whether no class in `name`'s ancestry copies on write.
fn is_reference_class(name: &str, definitions: &HashMap<String, TypeDefinition>) -> bool {
    ancestry(name, definitions).into_iter().all(|class| {
        let collection_copies = BuiltinCollectionKind::from_name(class)
            .and_then(cow_fn)
            .is_some();
        !collection_copies && class != STRING_TYPE_NAME
    })
}

/// Every class whose ancestry implements the trait `name` or a trait
/// extending it.
fn implementers_of<'d>(
    name: &str,
    definitions: &'d HashMap<String, TypeDefinition>,
) -> Vec<&'d str> {
    let extends_target = |written: &str| {
        let mut pending = vec![written];
        let mut seen = HashSet::new();
        while let Some(current) = pending.pop() {
            if current == name {
                return true;
            }
            if seen.insert(current) {
                if let Some(TypeDefinition::Trait(definition)) = definitions.get(current) {
                    pending.extend(definition.parent_traits.iter().map(String::as_str));
                }
            }
        }
        false
    };
    let mut classes: Vec<&str> = (definitions.iter())
        .filter(|(_, definition)| matches!(definition, TypeDefinition::Class(_)))
        .map(|(class, _)| class.as_str())
        .filter(|class| {
            ancestry(class, definitions).into_iter().any(|ancestor| {
                matches!(definitions.get(ancestor), Some(TypeDefinition::Class(definition))
                    if definition.traits.iter().any(|t| extends_target(t)))
            })
        })
        .collect();
    classes.sort_unstable();
    classes
}
