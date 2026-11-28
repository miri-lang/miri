// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a borrowed local's source is reached, whether a type is a reference
//! by design, and whether every instance on the way has one owner, worked
//! out for the borrow check on its own.
//!
//! The borrow inference answers these questions too. The check does not ask
//! it: a defect shared by the two would pass both, and these answers decide
//! whether a borrow outlives the value it reads. They are derived here from
//! the type definitions, the runtime's copy-on-write table and the type
//! checker's table of resources alone.

use std::collections::{HashMap, HashSet};

use crate::ast::types::{BuiltinCollectionKind, TypeKind, STRING_TYPE_NAME};
use crate::mir::place_type::PlaceTypes;
use crate::mir::{Body, Place, PlaceElem};
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

/// How `place` is reached from its root, the root itself being inside its
/// own value; `None` for a place with a step this check does not follow.
pub(super) fn reach_of(body: &Body, place: &Place) -> Option<Reach> {
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
            // A tuple's element picked by a run-time index is one of its
            // fields.
            (PlaceElem::Index(_, _), Container::FieldsByValue)
                if matches!(kind, TypeKind::Tuple(_)) => {}
            (PlaceElem::Field(_), Container::Instance) => reach = Reach::ThroughField,
            (PlaceElem::Field(_) | PlaceElem::Index(_, _), _) | (PlaceElem::Deref, _) => {
                return None
            }
        }
    }
    Some(reach)
}

/// Whether `written` and `read`, rooted at one local, part at a field of a
/// value that keeps each field in a slot of its own — a tuple, a struct or an
/// instance of a class — so a write into `written` leaves `read` as it was.
/// An enum's payload fields, and an optional's, may share a slot between
/// variants; a place inside the other, or a step this check cannot type,
/// parts at no field.
pub(super) fn parts_at_own_field(body: &Body, written: &Place, read: &Place) -> bool {
    if written.local != read.local {
        return false;
    }
    let common = (written.projection.iter())
        .zip(&read.projection)
        .position(|(w, r)| w != r);
    let Some(depth) = common else {
        return false;
    };
    let (PlaceElem::Field(_), PlaceElem::Field(_)) =
        (&written.projection[depth], &read.projection[depth])
    else {
        return false;
    };
    let container = Place {
        local: written.local,
        projection: written.projection[..depth].to_vec(),
    };
    let definitions = body.rc_classifier.definitions();
    PlaceTypes::of(body)
        .declared(&container)
        .is_some_and(|ty| keeps_fields_apart(&ty.kind, definitions))
}

/// Whether every step `place` takes through a field of a class instance, on
/// its way from its root, leaves an instance of a type every instance of
/// which is a resource, so no name but the root's chain holds it; a step
/// this check cannot type is not.
pub(super) fn steps_only_through_resources(body: &Body, place: &Place) -> bool {
    let types = PlaceTypes::of(body);
    let definitions = body.rc_classifier.definitions();
    (0..place.projection.len()).all(|depth| {
        let container = Place {
            local: place.local,
            projection: place.projection[..depth].to_vec(),
        };
        let Some(ty) = types.declared(&container) else {
            return false;
        };
        match (
            &place.projection[depth],
            container_of(&ty.kind, definitions),
        ) {
            (PlaceElem::Field(_), Container::Instance) => {
                body.resource_types.holds_at_every_instance(&ty.kind)
            }
            (PlaceElem::Field(_) | PlaceElem::Index(_, _), Container::FieldsByValue)
            | (PlaceElem::Index(_, _), Container::ElementsByValue) => true,
            (PlaceElem::Field(_), Container::ElementsByValue)
            | (PlaceElem::Index(_, _), Container::Instance)
            | (PlaceElem::Field(_) | PlaceElem::Index(_, _), Container::Opaque)
            | (PlaceElem::Deref, _) => false,
        }
    })
}

/// Whether a value of `kind` keeps each field in a slot of its own.
fn keeps_fields_apart(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> bool {
    let is_struct =
        |name: &String| matches!(definitions.get(name), Some(TypeDefinition::Struct(_)));
    match container_of(kind, definitions) {
        Container::Instance => true,
        Container::FieldsByValue => {
            matches!(kind, TypeKind::Tuple(_))
                || matches!(kind, TypeKind::Custom(name, _) if is_struct(name))
        }
        Container::ElementsByValue | Container::Opaque => false,
    }
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
        let collection_copies = BuiltinCollectionKind::from_name(class).is_some();
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
