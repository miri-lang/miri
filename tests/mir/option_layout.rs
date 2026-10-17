// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional's layout: which layers stay boxed, and whether the innermost
//! layer can spell its absence as a null payload pointer.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{
    FunctionTypeData, Type, TypeDeclarationKind, TypeKind, ATOMIC_TYPE_NAME, VEC2_TYPE_NAME,
    VEC3_TYPE_NAME, VEC4_TYPE_NAME,
};
use miri::error::syntax::Span;
use miri::mir::option_layout::{
    is_nullable_pointer_optional, payload_is_nullable_pointer, resolve_alias, LayoutScope,
    OptionalLayers,
};
use miri::type_checker::context::{
    AliasDefinition, ClassDefinition, EnumDefinition, GenericDefinition, StructDefinition,
    TraitDefinition, TypeDefinition,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn type_expr(kind: TypeKind) -> Expression {
    Expression::new(
        0,
        ExpressionKind::Type(Box::new(ty(kind)), false),
        Span::default(),
    )
}

fn custom(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

fn custom_of(name: &str, args: Vec<TypeKind>) -> TypeKind {
    TypeKind::Custom(
        name.to_string(),
        Some(args.into_iter().map(type_expr).collect()),
    )
}

fn optional(kind: TypeKind) -> TypeKind {
    TypeKind::Option(Box::new(ty(kind)))
}

fn class(name: &str) -> TypeDefinition {
    TypeDefinition::Class(ClassDefinition {
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
        runtime_settled_methods: BTreeSet::new(),
    })
}

fn structure() -> TypeDefinition {
    TypeDefinition::Struct(StructDefinition {
        fields: Vec::new(),
        generics: None,
        module: String::new(),
    })
}

fn enumeration() -> TypeDefinition {
    TypeDefinition::Enum(EnumDefinition {
        variants: BTreeMap::from([("A".to_string(), Vec::new())]),
        generics: None,
        methods: BTreeMap::new(),
        module: String::new(),
        must_use: false,
        non_exhaustive: false,
    })
}

fn trait_definition(name: &str) -> TypeDefinition {
    TypeDefinition::Trait(TraitDefinition {
        name: name.to_string(),
        generics: None,
        parent_traits: Vec::new(),
        parent_trait_args: BTreeMap::new(),
        methods: BTreeMap::new(),
        module: String::new(),
    })
}

fn alias_to(kind: TypeKind) -> TypeDefinition {
    TypeDefinition::Alias(AliasDefinition {
        template: ty(kind),
        generics: None,
    })
}

/// `type <name><T> is <template>`, the template naming `T` as a bare name.
fn generic_alias_to(template: TypeKind) -> TypeDefinition {
    TypeDefinition::Alias(AliasDefinition {
        template: ty(template),
        generics: Some(vec![GenericDefinition {
            name: "T".to_string(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }]),
    })
}

fn generic_definition(name: &str) -> TypeDefinition {
    TypeDefinition::Generic(GenericDefinition {
        name: name.to_string(),
        constraint: None,
        kind: TypeDeclarationKind::None,
    })
}

/// A table holding one definition of each kind, the collections as classes
/// (as the standard library declares them), and the inline vector and atomic
/// names as structs — so the tests prove those are decided by their component,
/// not by being absent from the table.
fn definitions() -> HashMap<String, TypeDefinition> {
    [
        ("Node", class("Node")),
        ("Point", structure()),
        ("Color", enumeration()),
        ("Shape", trait_definition("Shape")),
        ("Meters", alias_to(TypeKind::Int)),
        ("Distance", alias_to(custom("Meters"))),
        ("Graph", alias_to(custom("Node"))),
        ("MaybeNode", alias_to(optional(custom("Node")))),
        ("Id", generic_alias_to(custom("T"))),
        ("Opt", generic_alias_to(optional(custom("T")))),
        ("U", generic_definition("U")),
        ("List", class("List")),
        ("Map", class("Map")),
        ("Set", class("Set")),
        ("Array", class("Array")),
        (VEC2_TYPE_NAME, structure()),
        (VEC3_TYPE_NAME, structure()),
        (VEC4_TYPE_NAME, structure()),
        (ATOMIC_TYPE_NAME, structure()),
    ]
    .into_iter()
    .map(|(name, def)| (name.to_string(), def))
    .collect()
}

fn function_kind() -> TypeKind {
    TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    }))
}

fn nullable_pointer_payloads() -> Vec<(&'static str, TypeKind)> {
    vec![
        ("String", TypeKind::String),
        ("List<int>", custom_of("List", vec![TypeKind::Int])),
        (
            "Map<String, int>",
            custom_of("Map", vec![TypeKind::String, TypeKind::Int]),
        ),
        ("Set<int>", custom_of("Set", vec![TypeKind::Int])),
        ("Array<int>", custom_of("Array", vec![TypeKind::Int])),
        ("(int, String)", {
            TypeKind::Tuple(vec![type_expr(TypeKind::Int), type_expr(TypeKind::String)])
        }),
        ("fn()", function_kind()),
        ("class", custom("Node")),
        ("struct", custom("Point")),
        ("enum", custom("Color")),
        ("trait", custom("Shape")),
        ("alias to a class", custom("Graph")),
    ]
}

fn scalar_kinds() -> Vec<TypeKind> {
    vec![
        TypeKind::Int,
        TypeKind::I8,
        TypeKind::I16,
        TypeKind::I32,
        TypeKind::I64,
        TypeKind::I128,
        TypeKind::U8,
        TypeKind::U16,
        TypeKind::U32,
        TypeKind::U64,
        TypeKind::U128,
        TypeKind::Float,
        TypeKind::F16,
        TypeKind::F32,
        TypeKind::F64,
        TypeKind::Boolean,
    ]
}

fn value_payloads() -> Vec<(String, TypeKind)> {
    let named = vec![
        ("alias to a scalar", custom("Meters")),
        ("alias chain to a scalar", custom("Distance")),
        ("alias to an optional", custom("MaybeNode")),
        (
            "Atomic<u32>",
            custom_of(ATOMIC_TYPE_NAME, vec![TypeKind::U32]),
        ),
        ("Vec2<f32>", custom_of(VEC2_TYPE_NAME, vec![TypeKind::F32])),
        ("Vec3<f32>", custom_of(VEC3_TYPE_NAME, vec![TypeKind::F32])),
        ("Vec4<f32>", custom_of(VEC4_TYPE_NAME, vec![TypeKind::F32])),
        (
            "generic parameter",
            TypeKind::Generic("T".to_string(), None, TypeDeclarationKind::None),
        ),
        ("generic definition", custom("U")),
        ("placeholder naming no type", custom("T")),
        ("collection self-reference", custom("List")),
        ("Self", custom("Self")),
        ("Option", optional(custom("Node"))),
        ("RawPtr", TypeKind::RawPtr),
        ("Linear", TypeKind::Linear(Box::new(ty(custom("Node"))))),
        (
            "Result",
            TypeKind::Result(
                Box::new(type_expr(TypeKind::Int)),
                Box::new(type_expr(TypeKind::String)),
            ),
        ),
        (
            "Future",
            TypeKind::Future(Box::new(type_expr(TypeKind::Int))),
        ),
        ("OneOf", TypeKind::OneOf(vec![type_expr(TypeKind::F32)])),
        ("Error", TypeKind::Error),
        ("Void", TypeKind::Void),
    ];
    let scalars = scalar_kinds()
        .into_iter()
        .map(|kind| (format!("{kind:?}"), kind));
    named
        .into_iter()
        .map(|(name, kind)| (name.to_string(), kind))
        .chain(scalars)
        .collect()
}

#[test]
fn test_option_layout_pointer_payloads_are_nullable() {
    let defs = definitions();
    for (name, kind) in nullable_pointer_payloads() {
        assert!(
            payload_is_nullable_pointer(&kind, LayoutScope::program(&defs)),
            "{name} is a pointer an optional may spell absent as null"
        );
    }
}

#[test]
fn test_option_layout_value_payloads_are_not_nullable() {
    let defs = definitions();
    for (name, kind) in value_payloads() {
        assert!(
            !payload_is_nullable_pointer(&kind, LayoutScope::program(&defs)),
            "{name} must stay boxed inside an optional"
        );
    }
}

#[test]
fn test_option_layout_non_optional_has_no_layers() {
    let defs = definitions();
    let kind = custom("Node");
    let layers = OptionalLayers::of(&kind, LayoutScope::program(&defs));
    assert_eq!(layers.boxed, 0);
    assert!(!layers.innermost_nullable_pointer);
    assert_eq!(*layers.value, kind);
}

fn assert_layers(kind: TypeKind, boxed: usize, nullable: bool, value: &TypeKind) {
    let defs = definitions();
    let layers = OptionalLayers::of(&kind, LayoutScope::program(&defs));
    assert_eq!(layers.boxed, boxed, "boxed layers of {kind:?}");
    assert_eq!(
        layers.innermost_nullable_pointer, nullable,
        "innermost nullable pointer of {kind:?}"
    );
    assert_eq!(&*layers.value, value, "innermost value of {kind:?}");
}

#[test]
fn test_option_layout_pointer_payload_layers() {
    let node = custom("Node");
    assert_layers(optional(node.clone()), 0, true, &node);
    assert_layers(optional(optional(node.clone())), 1, true, &node);
    assert_layers(optional(optional(optional(node.clone()))), 2, true, &node);
    assert_layers(optional(TypeKind::String), 0, true, &TypeKind::String);
    let list = custom_of("List", vec![TypeKind::Int]);
    assert_layers(optional(list.clone()), 0, true, &list);
}

#[test]
fn test_option_layout_value_payload_layers() {
    assert_layers(optional(TypeKind::Int), 1, false, &TypeKind::Int);
    assert_layers(optional(optional(TypeKind::Int)), 2, false, &TypeKind::Int);
    assert_layers(optional(custom("Meters")), 1, false, &TypeKind::Int);
    let vector = custom_of(VEC3_TYPE_NAME, vec![TypeKind::F32]);
    assert_layers(optional(vector.clone()), 1, false, &vector);
}

#[test]
fn test_option_layout_alias_naming_an_optional_adds_its_layers() {
    let node = custom("Node");
    assert_layers(custom("MaybeNode"), 0, true, &node);
    assert_layers(optional(custom("MaybeNode")), 1, true, &node);
    assert_eq!(
        OptionalLayers::of(
            &optional(custom("MaybeNode")),
            LayoutScope::program(&definitions())
        ),
        OptionalLayers::of(
            &optional(optional(node.clone())),
            LayoutScope::program(&definitions())
        ),
        "an optional of an alias naming `Node?` is laid out as `Node??`"
    );
}

#[test]
fn test_option_layout_generic_alias_is_judged_at_its_arguments() {
    let node = custom("Node");
    assert_layers(
        optional(custom_of("Id", vec![node.clone()])),
        0,
        true,
        &node,
    );
    assert_layers(
        optional(custom_of("Id", vec![TypeKind::Int])),
        1,
        false,
        &TypeKind::Int,
    );
    assert_layers(custom_of("Opt", vec![node.clone()]), 0, true, &node);
    assert_layers(
        optional(custom_of("Opt", vec![node.clone()])),
        1,
        true,
        &node,
    );
    assert_layers(
        custom_of("Opt", vec![TypeKind::Int]),
        1,
        false,
        &TypeKind::Int,
    );
}

#[test]
fn test_option_layout_nullable_pointer_optional_sees_through_aliases() {
    let defs = definitions();
    assert!(is_nullable_pointer_optional(
        &optional(custom("Node")),
        LayoutScope::program(&defs)
    ));
    assert!(is_nullable_pointer_optional(
        &custom("MaybeNode"),
        LayoutScope::program(&defs)
    ));
    assert!(is_nullable_pointer_optional(
        &custom_of("Opt", vec![custom("Node")]),
        LayoutScope::program(&defs)
    ));
    assert!(is_nullable_pointer_optional(
        &optional(custom_of("Id", vec![custom("Node")])),
        LayoutScope::program(&defs)
    ));
    assert!(!is_nullable_pointer_optional(
        &optional(custom("MaybeNode")),
        LayoutScope::program(&defs)
    ));
    assert!(!is_nullable_pointer_optional(
        &custom("Node"),
        LayoutScope::program(&defs)
    ));
}

#[test]
fn test_alias_resolves_to_its_underlying_kind() {
    let table = HashMap::from([("Name".to_string(), alias_to(TypeKind::String))]);
    assert_eq!(
        resolve_alias(&custom("Name"), &table).map(|target| target.kind),
        Some(TypeKind::String)
    );
}

#[test]
fn test_chained_alias_resolves_to_the_final_kind() {
    assert_eq!(
        resolve_alias(&custom("Distance"), &definitions()).map(|target| target.kind),
        Some(TypeKind::Int),
        "the drop path must see through every hop of an alias chain"
    );
}

#[test]
fn test_generic_alias_resolves_at_its_arguments() {
    let defs = definitions();
    assert_eq!(
        resolve_alias(&custom_of("Id", vec![custom("Node")]), &defs).map(|target| target.kind),
        Some(custom("Node"))
    );
    assert_eq!(
        resolve_alias(&custom_of("Opt", vec![custom("Node")]), &defs).map(|target| target.kind),
        Some(optional(custom("Node")))
    );
}

#[test]
fn test_alias_cycle_does_not_resolve() {
    let table = HashMap::from([
        ("A".to_string(), alias_to(custom("B"))),
        ("B".to_string(), alias_to(custom("A"))),
    ]);
    assert_eq!(resolve_alias(&custom("A"), &table), None);
}

#[test]
fn test_non_alias_kinds_do_not_resolve() {
    let defs = definitions();
    assert_eq!(resolve_alias(&custom("Node"), &defs), None);
    assert_eq!(resolve_alias(&TypeKind::String, &defs), None);
}

/// Inside a body that leaves `Node` open as a type parameter, the bare name
/// means the parameter, though the program defines a class of that name: its
/// optional is boxed like every other parameter's.
#[test]
fn test_option_layout_an_open_parameter_shadows_a_type_of_the_same_name() {
    let defs = definitions();
    let open: std::collections::HashSet<String> = ["Node".to_string()].into_iter().collect();
    let in_body = LayoutScope::new(&defs, &open);
    assert!(!payload_is_nullable_pointer(&custom("Node"), in_body));
    assert!(!is_nullable_pointer_optional(
        &optional(custom("Node")),
        in_body
    ));
    assert!(is_nullable_pointer_optional(
        &optional(custom("Node")),
        LayoutScope::program(&defs)
    ));
}
