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

use cranelift_frontend::Variable;
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::types::{
    Type, TypeDeclarationKind, TypeKind, ATOMIC_TYPE_NAME, CLONEABLE_TRAIT_NAME, STRING_TYPE_NAME,
    VEC3_TYPE_NAME,
};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::translator::TypeCtx;
use miri::codegen::cranelift::FunctionTranslator;
use miri::error::syntax::Span;
use miri::mir::symbol::Symbol;
use miri::type_checker::context::{
    AliasDefinition, ClassDefinition, EnumDefinition, GenericDefinition, MethodInfo,
    StructDefinition, TraitDefinition, TypeDefinition,
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

/// `name<component>` — the form a compiler-known inline value type is always
/// written in, and what tells it apart from a user type reusing the name.
fn type_expr(kind: TypeKind) -> Expression {
    Expression::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
}

fn custom_of(name: &str, component: TypeKind) -> TypeKind {
    let arg = type_expr(component);
    TypeKind::Custom(name.to_string(), Some(vec![arg]))
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
        non_exhaustive: false,
    }
}

fn alias_to(kind: TypeKind) -> TypeDefinition {
    TypeDefinition::Alias(AliasDefinition {
        template: ty(kind),
        generics: None,
    })
}

fn minimal_type_ctx<'a>(
    type_defs: &'a HashMap<String, TypeDefinition>,
    captures: &'a HashMap<miri::mir::Local, Vec<Type>>,
    out_ptrs: &'a HashMap<miri::mir::Local, Variable>,
    instantiations: &'a HashMap<String, Vec<Vec<Type>>>,
) -> TypeCtx<'a> {
    use cranelift_codegen::ir::types;
    TypeCtx {
        local_types: &[],
        type_definitions: type_defs,
        ptr_type: types::I64,
        closure_capture_ast_types: captures,
        out_param_ptr_vars: out_ptrs,
        generic_class_instantiations: instantiations,
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
        TypeKind::String,
        TypeKind::Boolean,
        TypeKind::Identifier,
        TypeKind::RawPtr,
        TypeKind::List(Box::new(type_expr(TypeKind::Int))),
        TypeKind::Array(
            Box::new(type_expr(TypeKind::Int)),
            Box::new(type_expr(TypeKind::Int)),
        ),
        TypeKind::Map(
            Box::new(type_expr(TypeKind::String)),
            Box::new(type_expr(TypeKind::Int)),
        ),
        TypeKind::Tuple(Vec::new()),
        TypeKind::Set(Box::new(type_expr(TypeKind::Int))),
        TypeKind::Result(
            Box::new(type_expr(TypeKind::Int)),
            Box::new(type_expr(TypeKind::String)),
        ),
        TypeKind::Future(Box::new(type_expr(TypeKind::Int))),
        TypeKind::Function(Box::new(miri::ast::types::FunctionTypeData {
            generics: None,
            params: Vec::new(),
            return_type: Some(Box::new(type_expr(TypeKind::Void))),
        })),
        TypeKind::Meta(Box::new(ty(TypeKind::Int))),
        TypeKind::Option(Box::new(ty(TypeKind::String))),
        TypeKind::Void,
        TypeKind::Error,
        TypeKind::Linear(Box::new(ty(TypeKind::Int))),
    ] {
        assert!(
            !FunctionTranslator::is_unresolved_generic_elem(&kind, &HashMap::new()),
            "{kind:?} must resolve"
        );
    }
}

#[test]
fn test_all_type_definition_variants_are_resolved_for_custom_kind() {
    let struct_def = TypeDefinition::Struct(StructDefinition {
        generics: None,
        traits: Vec::new(),
        fields: Vec::new(),
        module: String::new(),
        has_drop: false,
    });
    let enum_d = TypeDefinition::Enum(enum_def([("A", vec![])]));
    let alias_d = alias_to(TypeKind::Int);
    let trait_d = TypeDefinition::Trait(TraitDefinition {
        name: "MyTrait".to_string(),
        generics: None,
        parent_traits: Vec::new(),
        parent_trait_args: BTreeMap::new(),
        methods: BTreeMap::new(),
        module: String::new(),
    });

    let table = defs([
        ("MyStruct", struct_def),
        ("MyEnum", enum_d),
        ("MyAlias", alias_d),
        ("MyTrait", trait_d),
    ]);

    for name in ["MyStruct", "MyEnum", "MyAlias", "MyTrait"] {
        assert!(
            !FunctionTranslator::is_unresolved_generic_elem(&custom(name), &table),
            "custom type '{name}' defined in type_definitions must resolve"
        );
    }
}

#[test]
fn test_custom_type_with_generic_args_resolves_if_definition_known() {
    let mut class_d = class("Container");
    class_d.generics = Some(vec![generic_param("T")]);
    let table = defs([("Container", TypeDefinition::Class(class_d))]);

    let custom_with_args = custom_of("Container", TypeKind::Int);
    assert!(
        !FunctionTranslator::is_unresolved_generic_elem(&custom_with_args, &table),
        "Container<Int> must resolve if Container is defined"
    );

    let unknown_with_args = custom_of("UnknownContainer", TypeKind::Int);
    assert!(
        FunctionTranslator::is_unresolved_generic_elem(&unknown_with_args, &table),
        "UnknownContainer<Int> must be unresolved when UnknownContainer is absent from definitions"
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
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx),
        vec![(1, vec![(0, TypeKind::String)])],
    );
}

#[test]
fn test_managed_field_index_is_the_position_within_its_variant() {
    let shape = enum_def([("Tagged", vec![TypeKind::Int, TypeKind::String])]);
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx),
        vec![(0, vec![(1, TypeKind::String)])],
        "the DecRef offset is derived from the field's index inside the variant"
    );
}

#[test]
fn test_enum_of_scalar_variants_collects_nothing() {
    let shape = enum_def([("A", vec![TypeKind::Int]), ("B", vec![TypeKind::F64])]);
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx).is_empty()
    );
}

/// An atomic written with its scalar component is stored as that scalar, by
/// value inside the payload slot, so a DecRef on it would treat raw bytes as a
/// pointer.
#[test]
fn test_inline_atomic_payload_is_not_collected_as_managed() {
    let shape = enum_def([("Inline", vec![custom_of(ATOMIC_TYPE_NAME, TypeKind::U32)])]);
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx).is_empty()
    );
}

/// An enum payload slot is one word wide (`enum_payload_slot_size` never
/// grows it for a vector, which translates to a pointer), so a `Vec3<f32>`
/// payload is held as the pointer to its own allocation, and dropping the enum
/// must release it.
#[test]
fn test_vector_payload_is_collected_as_managed() {
    let vector = custom_of(VEC3_TYPE_NAME, TypeKind::F32);
    let shape = enum_def([(
        "Mixed",
        vec![vector.clone(), custom_of(ATOMIC_TYPE_NAME, TypeKind::U32)],
    )]);
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx),
        vec![(0, vec![(0, vector)])],
        "only the vector's pointer word is released; the atomic is inline"
    );
}

/// A declaration that reuses a vector's name without the component it holds is
/// an ordinary type: it is laid out from its own fields and heap-allocated, so
/// its payload position holds a pointer that has to be released.
#[test]
fn test_a_type_reusing_a_vector_name_is_collected_as_managed() {
    let shape = enum_def([("Payload", vec![custom(VEC3_TYPE_NAME)])]);
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx),
        vec![(0, vec![(0, custom(VEC3_TYPE_NAME))])]
    );
}

#[test]
fn test_clone_resolves_to_the_class_that_defines_it() {
    let mut widget = class("Widget");
    widget.methods.insert("clone".to_string(), clone_method());
    let table = defs([("Widget", TypeDefinition::Class(widget))]);

    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Widget", &table),
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
        FunctionTranslator::resolve_clone_method_name("Derived", &table),
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
        FunctionTranslator::resolve_clone_method_name("Derived", &table),
        Symbol::method("Derived", &[], "clone", &[]).link_name()
    );
}

#[test]
fn test_clone_falls_back_to_the_requested_type_name() {
    let table = defs([("Widget", TypeDefinition::Class(class("Widget")))]);
    assert_eq!(
        FunctionTranslator::resolve_clone_method_name("Widget", &table),
        Symbol::method("Widget", &[], "clone", &[]).link_name()
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
fn test_class_with_multiple_traits_implements_cloneable() {
    let mut widget = class("Widget");
    widget.traits = vec![
        "Printable".to_string(),
        CLONEABLE_TRAIT_NAME.to_string(),
        "Equatable".to_string(),
    ];
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
    let defs = HashMap::new();
    let captures = HashMap::new();
    let out_ptrs = HashMap::new();
    let instantiations = HashMap::new();
    let type_ctx = minimal_type_ctx(&defs, &captures, &out_ptrs, &instantiations);

    assert_eq!(
        FunctionTranslator::enum_variants_with_managed_fields(&shape, None, &type_ctx),
        vec![(0, vec![(0, custom("Widget"))])],
    );
}
