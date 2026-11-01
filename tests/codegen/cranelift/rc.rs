// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the reference-counting code emission decisions.
//!
//! The drop/decref/clone thunks themselves need a Cranelift module to emit
//! into, but every *decision* they make is a pure function of the type table:
//! which symbol the drop call targets, which fields get a DecRef, which
//! element types have a resolvable decref helper at all. Those decisions are
//! what a wrong RC diff gets wrong — an unresolved symbol fails at link time,
//! a skipped managed field leaks, a mangled name that disagrees with the
//! thunk-generation site drops through to the wrong layout.

use miri::ast::types::{Type, TypeKind, CLONEABLE_TRAIT_NAME, STRING_TYPE_NAME};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::FunctionTranslator;
use miri::error::syntax::Span;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::TypeFacts;
use miri::type_checker::context::{
    ClassDefinition, EnumDefinition, MethodInfo, StructDefinition, TypeDefinition,
};
use miri::type_checker::ModuleId;
use std::collections::{BTreeMap, HashMap};

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn custom(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

fn clone_method() -> MethodInfo {
    MethodInfo {
        params: Vec::new(),
        is_out_flags: Vec::new(),
        return_type: ty(TypeKind::Void),
        visibility: MemberVisibility::Public,
        is_constructor: false,
        is_abstract: false,
        is_static: false,
        attributes: Vec::new(),
    }
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

#[test]
fn test_instantiation_without_type_arguments_mangles_to_the_bare_name() {
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box", &[]).link_name(),
        "miri.Box"
    );
}

#[test]
fn test_string_type_argument_mangles_to_the_canonical_string_name() {
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box", &[ty(TypeKind::String)]).link_name(),
        format!("miri.Box${STRING_TYPE_NAME}")
    );
}

#[test]
fn test_scalar_type_arguments_mangle_to_their_width_tokens() {
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box", &[ty(TypeKind::Int)]).link_name(),
        "miri.Box$int"
    );
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box", &[ty(TypeKind::F32)]).link_name(),
        "miri.Box$f32"
    );
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box", &[ty(TypeKind::Boolean)]).link_name(),
        "miri.Box$bool"
    );
}

#[test]
fn test_multiple_type_arguments_mangle_in_declaration_order() {
    assert_eq!(
        Symbol::function(
            &ModuleId::Program,
            "Pair",
            &[ty(TypeKind::Int), ty(TypeKind::String)]
        )
        .link_name(),
        format!("miri.Pair$int${STRING_TYPE_NAME}")
    );
    assert_ne!(
        Symbol::function(
            &ModuleId::Program,
            "Pair",
            &[ty(TypeKind::Int), ty(TypeKind::String)]
        )
        .link_name(),
        Symbol::function(
            &ModuleId::Program,
            "Pair",
            &[ty(TypeKind::String), ty(TypeKind::Int)]
        )
        .link_name(),
        "argument order must change the thunk symbol"
    );
}

/// A user-defined type argument spells its own name, so `Box<Widget>` and
/// `Box<Gadget>` name two drop thunks, each laid out for its own argument.
#[test]
fn test_class_type_arguments_mangle_to_their_own_names() {
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box", &[ty(custom("Widget"))]).link_name(),
        "miri.Box$Widget"
    );
    assert_ne!(
        Symbol::function(&ModuleId::Program, "Box", &[ty(custom("Gadget"))]).link_name(),
        Symbol::function(&ModuleId::Program, "Box", &[ty(custom("Widget"))]).link_name(),
        "two instantiations sharing a symbol would run one body against the other's layout"
    );
}

#[test]
fn test_clone_resolves_to_the_class_that_defines_it() {
    let mut widget = class("Widget");
    widget.methods.insert("clone".to_string(), clone_method());
    let table = defs([("Widget", TypeDefinition::Class(widget))]);

    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Widget", &facts_of(table)),
        Symbol::method("Widget", &[], "clone", &[]).link_name()
    );
}

#[test]
fn test_clone_inherited_from_a_concrete_base_uses_the_base_name() {
    let mut base = class("Base");
    base.methods.insert("clone".to_string(), clone_method());
    let mut derived = class("Derived");
    derived.base_class = Some("Base".to_string());
    let table = defs([
        ("Base", TypeDefinition::Class(base)),
        ("Derived", TypeDefinition::Class(derived)),
    ]);

    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Derived", &facts_of(table)),
        Symbol::method("Base", &[], "clone", &[]).link_name()
    );
}

#[test]
fn test_clone_inherited_from_an_abstract_base_uses_the_caller_name() {
    // The abstract class has no emitted body, so the concrete caller's mangled
    // name is the one that exists — matching how call sites are mangled.
    let mut base = class("Base");
    base.is_abstract = true;
    base.methods.insert("clone".to_string(), clone_method());
    let mut derived = class("Derived");
    derived.base_class = Some("Base".to_string());
    let table = defs([
        ("Base", TypeDefinition::Class(base)),
        ("Derived", TypeDefinition::Class(derived)),
    ]);

    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Derived", &facts_of(table)),
        Symbol::method("Derived", &[], "clone", &[]).link_name()
    );
}

#[test]
fn test_clone_falls_back_to_the_requested_type_name() {
    let table = defs([("Widget", TypeDefinition::Class(class("Widget")))]);
    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Widget", &facts_of(table)),
        Symbol::method("Widget", &[], "clone", &[]).link_name()
    );
}

#[test]
fn test_class_listing_the_cloneable_trait_implements_it() {
    let mut widget = class("Widget");
    widget.traits = vec![CLONEABLE_TRAIT_NAME.to_string()];
    let table = defs([("Widget", TypeDefinition::Class(widget))]);
    let facts = facts_of(table);

    assert!(FunctionTranslator::class_implements_cloneable(
        "Widget", &facts
    ));
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

    assert!(FunctionTranslator::class_implements_cloneable(
        "Widget", &facts
    ));
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

    assert!(FunctionTranslator::class_implements_cloneable(
        "Leaf", &facts
    ));
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

    assert!(!FunctionTranslator::class_implements_cloneable(
        "Derived", &facts
    ));
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

    assert!(!FunctionTranslator::class_implements_cloneable(
        "Point", &facts
    ));
    assert!(!FunctionTranslator::class_implements_cloneable(
        "Shape", &facts
    ));
    assert!(!FunctionTranslator::class_implements_cloneable(
        "Missing", &facts
    ));
}
