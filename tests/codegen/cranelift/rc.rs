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

use miri::ast::types::{
    Type, TypeDeclarationKind, TypeKind, ATOMIC_TYPE_NAME, CLONEABLE_TRAIT_NAME, STRING_TYPE_NAME,
    VEC3_TYPE_NAME,
};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::{mangle_class_instantiation, FunctionTranslator};
use miri::error::syntax::Span;
use miri::type_checker::context::{
    AliasDefinition, ClassDefinition, EnumDefinition, GenericDefinition, MethodInfo,
    StructDefinition, TypeDefinition,
};
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

fn generic_param(name: &str) -> GenericDefinition {
    GenericDefinition {
        name: name.to_string(),
        constraint: None,
        kind: TypeDeclarationKind::None,
    }
}

fn clone_method() -> MethodInfo {
    MethodInfo {
        params: Vec::new(),
        is_out_flags: Vec::new(),
        return_type: ty(TypeKind::Void),
        visibility: MemberVisibility::Public,
        is_constructor: false,
        is_abstract: false,
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
        has_drop: false,
    }
}

fn defs<const N: usize>(entries: [(&str, TypeDefinition); N]) -> HashMap<String, TypeDefinition> {
    entries
        .into_iter()
        .map(|(name, def)| (name.to_string(), def))
        .collect()
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
    }
}

fn alias_to(kind: TypeKind) -> TypeDefinition {
    TypeDefinition::Alias(AliasDefinition {
        template: ty(kind),
        generics: None,
    })
}

#[test]
fn test_instantiation_without_type_arguments_mangles_to_the_bare_name() {
    assert_eq!(mangle_class_instantiation("Box", &[]), "Box");
}

#[test]
fn test_string_type_argument_mangles_to_the_canonical_string_name() {
    assert_eq!(
        mangle_class_instantiation("Box", &[ty(TypeKind::String)]),
        format!("Box__{STRING_TYPE_NAME}")
    );
}

#[test]
fn test_scalar_type_arguments_mangle_to_their_width_tokens() {
    assert_eq!(
        mangle_class_instantiation("Box", &[ty(TypeKind::Int)]),
        "Box__int"
    );
    assert_eq!(
        mangle_class_instantiation("Box", &[ty(TypeKind::F32)]),
        "Box__f32"
    );
    assert_eq!(
        mangle_class_instantiation("Box", &[ty(TypeKind::Boolean)]),
        "Box__bool"
    );
}

#[test]
fn test_multiple_type_arguments_mangle_in_declaration_order() {
    assert_eq!(
        mangle_class_instantiation("Pair", &[ty(TypeKind::Int), ty(TypeKind::String)]),
        format!("Pair__int__{STRING_TYPE_NAME}")
    );
    assert_ne!(
        mangle_class_instantiation("Pair", &[ty(TypeKind::Int), ty(TypeKind::String)]),
        mangle_class_instantiation("Pair", &[ty(TypeKind::String), ty(TypeKind::Int)]),
        "argument order must change the thunk symbol"
    );
}

/// Every user-defined type argument mangles to the same `custom` token, so
/// `Box<Widget>` and `Box<Gadget>` name one shared drop thunk. Both sides of
/// the call agree, so the symbol always resolves — the shared thunk is why the
/// two instantiations cannot be given different field layouts.
#[test]
fn test_class_type_arguments_share_one_mangled_token() {
    assert_eq!(
        mangle_class_instantiation("Box", &[ty(custom("Widget"))]),
        "Box__custom"
    );
    assert_eq!(
        mangle_class_instantiation("Box", &[ty(custom("Gadget"))]),
        mangle_class_instantiation("Box", &[ty(custom("Widget"))])
    );
}

#[test]
fn test_bare_generic_element_is_unresolved() {
    let elem = TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None);
    assert!(FunctionTranslator::is_unresolved_generic_elem(
        &elem,
        &HashMap::new()
    ));
}

#[test]
fn test_element_naming_an_unknown_type_is_unresolved() {
    assert!(
        FunctionTranslator::is_unresolved_generic_elem(&custom("Widget"), &HashMap::new()),
        "no __decref_Widget symbol exists when Widget is not in the type table"
    );
}

#[test]
fn test_element_naming_a_generic_parameter_definition_is_unresolved() {
    let table = defs([("T", TypeDefinition::Generic(generic_param("T")))]);
    assert!(FunctionTranslator::is_unresolved_generic_elem(
        &custom("T"),
        &table
    ));
}

#[test]
fn test_element_naming_a_known_class_is_resolved() {
    let table = defs([("Widget", TypeDefinition::Class(class("Widget")))]);
    assert!(!FunctionTranslator::is_unresolved_generic_elem(
        &custom("Widget"),
        &table
    ));
}

#[test]
fn test_builtin_collection_element_is_resolved_without_a_definition() {
    // Built-in collections have a shared per-shape decref helper, so they never
    // depend on a per-type symbol.
    for name in ["List", "Array", "Map", "Set"] {
        assert!(
            !FunctionTranslator::is_unresolved_generic_elem(&custom(name), &HashMap::new()),
            "{name} must resolve to its per-shape helper"
        );
    }
}

#[test]
fn test_concrete_element_kinds_are_resolved() {
    for kind in [
        TypeKind::String,
        TypeKind::Int,
        TypeKind::I32,
        TypeKind::F64,
        TypeKind::Boolean,
        TypeKind::Option(Box::new(ty(TypeKind::String))),
        TypeKind::Tuple(Vec::new()),
    ] {
        assert!(
            !FunctionTranslator::is_unresolved_generic_elem(&kind, &HashMap::new()),
            "{kind:?} must resolve"
        );
    }
}

#[test]
fn test_generic_field_resolves_to_the_argument_at_its_parameter_position() {
    let mut box_def = class("Pair");
    box_def.generics = Some(vec![generic_param("K"), generic_param("V")]);

    let first = FunctionTranslator::generic_field_concrete_kind(
        &box_def,
        &TypeKind::Generic("K".to_string(), None, TypeDeclarationKind::None),
        Some(&[ty(TypeKind::String), ty(TypeKind::Int)]),
    );
    let second = FunctionTranslator::generic_field_concrete_kind(
        &box_def,
        &TypeKind::Generic("V".to_string(), None, TypeDeclarationKind::None),
        Some(&[ty(TypeKind::String), ty(TypeKind::Int)]),
    );

    assert_eq!(first, Some(TypeKind::String));
    assert_eq!(second, Some(TypeKind::Int));
}

#[test]
fn test_generic_field_spelled_as_a_bare_custom_name_also_resolves() {
    // A field declared `value T` can reach codegen as `Custom("T", None)`
    // rather than `Generic("T")`; both spellings must resolve identically.
    let mut box_def = class("Box");
    box_def.generics = Some(vec![generic_param("T")]);

    assert_eq!(
        FunctionTranslator::generic_field_concrete_kind(
            &box_def,
            &custom("T"),
            Some(&[ty(TypeKind::String)])
        ),
        Some(TypeKind::String)
    );
}

#[test]
fn test_field_naming_a_non_parameter_type_does_not_resolve() {
    let mut box_def = class("Box");
    box_def.generics = Some(vec![generic_param("T")]);

    assert_eq!(
        FunctionTranslator::generic_field_concrete_kind(
            &box_def,
            &custom("Widget"),
            Some(&[ty(TypeKind::String)])
        ),
        None,
        "a concrete field type is not a generic placeholder to substitute"
    );
}

#[test]
fn test_generic_field_without_instantiation_arguments_does_not_resolve() {
    // The shared bare-name thunk carries no arguments; the field must be
    // skipped there rather than guessed at.
    let mut box_def = class("Box");
    box_def.generics = Some(vec![generic_param("T")]);

    assert_eq!(
        FunctionTranslator::generic_field_concrete_kind(
            &box_def,
            &TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None),
            None
        ),
        None
    );
}

#[test]
fn test_generic_field_of_a_non_generic_class_does_not_resolve() {
    assert_eq!(
        FunctionTranslator::generic_field_concrete_kind(
            &class("Widget"),
            &TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None),
            Some(&[ty(TypeKind::String)])
        ),
        None
    );
}

#[test]
fn test_generic_field_beyond_the_supplied_arguments_does_not_resolve() {
    let mut pair = class("Pair");
    pair.generics = Some(vec![generic_param("K"), generic_param("V")]);

    assert_eq!(
        FunctionTranslator::generic_field_concrete_kind(
            &pair,
            &TypeKind::Generic("V".to_string(), None, TypeDeclarationKind::None),
            Some(&[ty(TypeKind::String)])
        ),
        None,
        "a short argument list must not index past its end"
    );
}

#[test]
fn test_alias_resolves_to_its_underlying_kind() {
    let table = defs([("Name", alias_to(TypeKind::String))]);
    assert_eq!(
        FunctionTranslator::resolve_alias(&custom("Name"), &table),
        Some(&TypeKind::String)
    );
}

#[test]
fn test_chained_alias_resolves_to_the_final_kind() {
    let table = defs([
        ("Outer", alias_to(custom("Inner"))),
        ("Inner", alias_to(TypeKind::String)),
    ]);
    assert_eq!(
        FunctionTranslator::resolve_alias(&custom("Outer"), &table),
        Some(&TypeKind::String),
        "the drop path must see through every hop of an alias chain"
    );
}

#[test]
fn test_non_alias_custom_type_does_not_resolve() {
    let table = defs([("Widget", TypeDefinition::Class(class("Widget")))]);
    assert_eq!(
        FunctionTranslator::resolve_alias(&custom("Widget"), &table),
        None
    );
}

#[test]
fn test_builtin_kind_does_not_resolve_as_an_alias() {
    assert_eq!(
        FunctionTranslator::resolve_alias(&TypeKind::String, &HashMap::new()),
        None
    );
}

#[test]
fn test_only_variants_carrying_managed_fields_are_collected() {
    // BTreeMap ordering fixes the discriminants: Empty=0, Labelled=1, Sized=2.
    let shape = enum_def([
        ("Empty", vec![]),
        ("Labelled", vec![TypeKind::String, TypeKind::Int]),
        ("Sized", vec![TypeKind::Int]),
    ]);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape),
        vec![(1, vec![(0, TypeKind::String)])],
    );
}

#[test]
fn test_managed_field_index_is_the_position_within_its_variant() {
    let shape = enum_def([("Tagged", vec![TypeKind::Int, TypeKind::String])]);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape),
        vec![(0, vec![(1, TypeKind::String)])],
        "the DecRef offset is derived from the field's index inside the variant"
    );
}

#[test]
fn test_enum_of_scalar_variants_collects_nothing() {
    let shape = enum_def([("A", vec![TypeKind::Int]), ("B", vec![TypeKind::F64])]);

    assert!(FunctionTranslator::enum_variants_with_managed_fields(&shape).is_empty());
}

#[test]
fn test_inline_value_fields_are_not_collected_as_managed() {
    // `Vec3` and `Atomic` are stored by value inside the payload, so a DecRef
    // on them would treat raw bytes as a pointer.
    let shape = enum_def([(
        "Inline",
        vec![custom(VEC3_TYPE_NAME), custom(ATOMIC_TYPE_NAME)],
    )]);

    assert!(FunctionTranslator::enum_variants_with_managed_fields(&shape).is_empty());
}

#[test]
fn test_clone_resolves_to_the_class_that_defines_it() {
    let mut widget = class("Widget");
    widget.methods.insert("clone".to_string(), clone_method());
    let table = defs([("Widget", TypeDefinition::Class(widget))]);

    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Widget", &table),
        "Widget_clone"
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
        FunctionTranslator::resolve_clone_method_name("Derived", &table),
        "Base_clone"
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
        FunctionTranslator::resolve_clone_method_name("Derived", &table),
        "Derived_clone"
    );
}

#[test]
fn test_clone_falls_back_to_the_requested_type_name() {
    let table = defs([("Widget", TypeDefinition::Class(class("Widget")))]);
    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Widget", &table),
        "Widget_clone"
    );
}

#[test]
fn test_class_listing_the_cloneable_trait_implements_it() {
    let mut widget = class("Widget");
    widget.traits = vec![CLONEABLE_TRAIT_NAME.to_string()];
    let table = defs([("Widget", TypeDefinition::Class(widget))]);

    assert!(FunctionTranslator::class_implements_cloneable(
        "Widget", &table
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

    assert!(FunctionTranslator::class_implements_cloneable(
        "Leaf", &table
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

    assert!(!FunctionTranslator::class_implements_cloneable(
        "Derived", &table
    ));
}

#[test]
fn test_non_class_definitions_do_not_implement_cloneable() {
    let table = defs([
        (
            "Point",
            TypeDefinition::Struct(StructDefinition {
                generics: None,
                traits: Vec::new(),
                fields: vec![("x".to_string(), ty(TypeKind::Int), MemberVisibility::Public)],
                module: String::new(),
                has_drop: false,
            }),
        ),
        ("Shape", TypeDefinition::Enum(enum_def([("A", vec![])]))),
    ]);

    assert!(!FunctionTranslator::class_implements_cloneable(
        "Point", &table
    ));
    assert!(!FunctionTranslator::class_implements_cloneable(
        "Shape", &table
    ));
    assert!(!FunctionTranslator::class_implements_cloneable(
        "Missing", &table
    ));
}

#[test]
fn test_managed_class_field_survives_as_managed_in_a_variant() {
    // A user class field inside an enum variant is a pointer and must be
    // DecRef'd, unlike the inline value wrappers above.
    let shape = enum_def([("Wrapped", vec![custom("Widget")])]);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape),
        vec![(0, vec![(0, custom("Widget"))])],
    );
}
