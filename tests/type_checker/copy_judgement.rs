// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether a collection refuses to copy an element, and which element types
//! a copy of a collection copies.

use std::collections::HashMap;

use crate::type_checker::utils::type_checker_result;
use miri::ast::factory::make_type;
use miri::ast::type_arguments::type_arguments;
use miri::ast::types::{BuiltinCollectionKind, TypeKind};
use miri::runtime_effects::ElementRole;
use miri::type_checker::context::TypeDefinition;
use miri::type_checker::copy_judgement::{
    copied_element_kinds, element_copy, refuses_copy_of, CopiedCollection, ElementCopy, RefusedCopy,
};
use miri::type_checker::utils::{Instantiations, ResourceTypes};

const PROGRAM: &str = r#"use system.memory

class Hooked
    var id int
    fn drop(self)
        return

class Copied implements Cloneable
    var id int
    fn clone() Copied
        return Copied(id: self.id)
    fn drop(self)
        return

class Plain
    var id int

enum Tagged
    Tag(int)

    fn drop(self)
        return

fn main()
    return
"#;

fn named(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

/// Whether a copy of `kind` stored as `role` is refused over `PROGRAM`.
fn refuses(kind: &TypeKind, role: ElementRole) -> bool {
    let result = type_checker_result(PROGRAM);
    let definitions: &HashMap<String, TypeDefinition> = result.type_checker.type_definitions();
    let resources = ResourceTypes::of(definitions, &Instantiations::new());
    refuses_copy_of(kind, role, definitions, &resources)
}

#[test]
fn a_resource_with_no_clone_is_refused_in_every_role() {
    for role in [ElementRole::Element, ElementRole::Value, ElementRole::Key] {
        assert!(refuses(&named("Hooked"), role), "{role:?}");
    }
}

#[test]
fn a_cloneable_resource_is_copied_but_never_as_a_key() {
    assert!(!refuses(&named("Copied"), ElementRole::Element));
    assert!(!refuses(&named("Copied"), ElementRole::Value));
    assert!(refuses(&named("Copied"), ElementRole::Key));
}

#[test]
fn a_plain_value_is_never_refused() {
    for kind in [named("Plain"), TypeKind::String, TypeKind::Int] {
        assert!(!refuses(&kind, ElementRole::Key), "{kind:?}");
        assert!(!refuses(&kind, ElementRole::Element), "{kind:?}");
    }
}

#[test]
fn a_collection_copies_each_type_it_is_an_instance_at() {
    let result = type_checker_result(PROGRAM);
    let definitions = result.type_checker.type_definitions();
    let arguments = type_arguments([make_type(TypeKind::Int), make_type(named("Hooked"))]);
    let map = TypeKind::Custom("Map".to_string(), Some(arguments));
    assert_eq!(
        copied_element_kinds(&map, definitions),
        vec![
            (TypeKind::Int, ElementRole::Key),
            (named("Hooked"), ElementRole::Value)
        ]
    );
}

/// `name` written at the type arguments `arguments`.
fn instance(name: &str, arguments: Vec<TypeKind>) -> TypeKind {
    let arguments = type_arguments(arguments.into_iter().map(make_type));
    TypeKind::Custom(name.to_string(), Some(arguments))
}

#[test]
fn an_element_is_copied_through_its_clone_or_the_copy_of_the_collection_holding_it() {
    let result = type_checker_result(PROGRAM);
    let definitions = result.type_checker.type_definitions();
    assert_eq!(
        element_copy(&named("Copied"), definitions),
        ElementCopy::Class("Copied")
    );
    let held = [
        (BuiltinCollectionKind::List, CopiedCollection::List),
        (BuiltinCollectionKind::Set, CopiedCollection::Set),
    ];
    for (written, collection) in held {
        let kind = instance(written.name(), vec![named("Copied")]);
        assert_eq!(
            element_copy(&kind, definitions),
            ElementCopy::Collection(collection)
        );
    }
    let values = instance("Map", vec![TypeKind::Int, named("Copied")]);
    assert_eq!(
        element_copy(&values, definitions),
        ElementCopy::Collection(CopiedCollection::Map)
    );
}

#[test]
fn an_element_nothing_clones_is_shared() {
    let result = type_checker_result(PROGRAM);
    let definitions = result.type_checker.type_definitions();
    let keys = instance("Map", vec![named("Copied"), TypeKind::Int]);
    let array = instance("Array", vec![named("Copied")]);
    for kind in [named("Hooked"), named("Plain"), TypeKind::Int, keys, array] {
        assert_eq!(
            element_copy(&kind, definitions),
            ElementCopy::Shared,
            "{kind:?}"
        );
    }
}

#[test]
fn a_collection_named_at_no_types_copies_its_own_parameters() {
    let result = type_checker_result(PROGRAM);
    let definitions = result.type_checker.type_definitions();
    let kinds = copied_element_kinds(&named("List"), definitions);
    assert_eq!(kinds.len(), 1, "{kinds:?}");
    assert!(copied_element_kinds(&TypeKind::Int, definitions).is_empty());
}

#[test]
fn a_refused_class_names_the_copy_trait_and_a_refused_enum_does_not() {
    let result = type_checker_result(PROGRAM);
    let definitions = result.type_checker.type_definitions();
    let (class, tagged) = (named("Hooked"), named("Tagged"));
    assert_eq!(
        RefusedCopy::of(&class, definitions).copy_trait,
        Some("Cloneable")
    );
    assert_eq!(RefusedCopy::of(&tagged, definitions).copy_trait, None);
    assert_eq!(
        RefusedCopy::of(&TypeKind::Int, definitions).copy_trait,
        None
    );
}

#[test]
fn a_hooked_enum_is_refused_as_a_value_and_as_a_key() {
    for role in [ElementRole::Element, ElementRole::Value, ElementRole::Key] {
        assert!(refuses(&named("Tagged"), role), "{role:?}");
    }
}
