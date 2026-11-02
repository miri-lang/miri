// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a set recognises two of its elements, or a map two of its keys, as the
//! same one, and which element types answer a container's questions
//! themselves.

use miri::ast::types::{Type, TypeDeclarationKind, TypeKind, CLONEABLE_TRAIT_NAME};
use miri::ast::MemberVisibility;
use miri::error::syntax::Span;
use miri::mir::type_facts::{
    ClassDefinition, ElementIdentity, FloatWidth, TypeDefinition, TypeFacts,
};
use miri::type_checker::context::{EnumDefinition, StructDefinition};
use std::collections::{BTreeMap, HashMap, HashSet};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn class(name: &str) -> ClassDefinition {
    ClassDefinition {
        name: name.to_string(),
        generics: None,
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: Vec::new(),
        methods: BTreeMap::new(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: std::collections::BTreeSet::new(),
    }
}

fn defs<const N: usize>(entries: [(&str, TypeDefinition); N]) -> HashMap<String, TypeDefinition> {
    entries
        .into_iter()
        .map(|(name, def)| (name.to_string(), def))
        .collect()
}

/// The settled facts over `table`, with no instantiations and no bodies.
fn facts_of(table: HashMap<String, TypeDefinition>) -> TypeFacts {
    TypeFacts::new(table, HashMap::new(), Default::default(), []).unwrap()
}

fn enum_def<const N: usize>(variants: [(&str, Vec<TypeKind>); N]) -> EnumDefinition {
    EnumDefinition {
        variants: variants
            .into_iter()
            .map(|(name, kinds)| (name.to_string(), kinds.into_iter().map(ty).collect()))
            .collect(),
        generics: None,
        methods: BTreeMap::new(),
        module: String::new(),
        must_use: false,
        non_exhaustive: false,
    }
}

fn identity_of(kind: TypeKind) -> ElementIdentity {
    facts_of(HashMap::new()).element_identity(&kind, &HashSet::new())
}

#[test]
fn a_string_element_is_matched_by_its_content() {
    assert_eq!(
        identity_of(TypeKind::String),
        ElementIdentity::StringContent
    );
}

#[test]
fn a_float_element_is_matched_by_the_number_of_its_width() {
    assert_eq!(
        identity_of(TypeKind::F32),
        ElementIdentity::Float(FloatWidth::F32)
    );
    assert_eq!(
        identity_of(TypeKind::F64),
        ElementIdentity::Float(FloatWidth::F64)
    );
}

#[test]
fn an_integer_element_is_matched_by_its_bytes() {
    assert_eq!(identity_of(TypeKind::Int), ElementIdentity::Bytes);
}

#[test]
fn an_open_type_parameter_registers_no_rule() {
    let parameter = TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None);
    let open = HashSet::from(["T".to_string()]);
    let identity = facts_of(HashMap::new()).element_identity(&parameter, &open);
    assert_eq!(identity, ElementIdentity::Unregistered);
}
#[test]
fn test_class_listing_the_cloneable_trait_implements_it() {
    let mut widget = class("Widget");
    widget.traits = vec![CLONEABLE_TRAIT_NAME.to_string()];
    let table = defs([("Widget", TypeDefinition::Class(widget))]);
    let facts = facts_of(table);

    assert!(facts.is_cloneable("Widget"));
}

#[test]
fn test_class_with_multiple_traits_implements_cloneable() {
    let mut widget = class("Widget");
    widget.traits = vec![
        "Printable".to_string(),
        CLONEABLE_TRAIT_NAME.to_string(),
        "Equatable".to_string(),
    ];
    let table = defs([("Widget", TypeDefinition::Class(widget))]);
    let facts = facts_of(table);

    assert!(facts.is_cloneable("Widget"));
}

#[test]
fn test_cloneable_is_inherited_through_the_base_chain() {
    let mut root = class("Root");
    root.traits = vec![CLONEABLE_TRAIT_NAME.to_string()];
    let mut middle = class("Middle");
    middle.base_class = Some("Root".to_string());
    let mut leaf = class("Leaf");
    leaf.base_class = Some("Middle".to_string());
    let table = defs([
        ("Root", TypeDefinition::Class(root)),
        ("Middle", TypeDefinition::Class(middle)),
        ("Leaf", TypeDefinition::Class(leaf)),
    ]);
    let facts = facts_of(table);

    assert!(facts.is_cloneable("Leaf"));
}

#[test]
fn test_class_without_the_cloneable_trait_does_not_implement_it() {
    let mut base = class("Base");
    base.traits = vec!["Printable".to_string()];
    let mut derived = class("Derived");
    derived.base_class = Some("Base".to_string());
    let table = defs([
        ("Base", TypeDefinition::Class(base)),
        ("Derived", TypeDefinition::Class(derived)),
    ]);
    let facts = facts_of(table);

    assert!(!facts.is_cloneable("Derived"));
}

#[test]
fn test_non_class_definitions_do_not_implement_cloneable() {
    let table = defs([
        (
            "Point",
            TypeDefinition::Struct(StructDefinition {
                generics: None,
                fields: vec![("x".to_string(), ty(TypeKind::Int), MemberVisibility::Public)],
                module: String::new(),
            }),
        ),
        ("Shape", TypeDefinition::Enum(enum_def([("A", vec![])]))),
    ]);
    let facts = facts_of(table);

    assert!(!facts.is_cloneable("Point"));
    assert!(!facts.is_cloneable("Shape"));
    assert!(!facts.is_cloneable("Missing"));
}
