// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What each drop thunk releases: the hook it runs and the counted members it
//! releases, at the instantiation the thunk is emitted for.

use crate::type_checker::utils::type_checker_result;
use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{Type, TypeKind, ATOMIC_TYPE_NAME, VEC3_TYPE_NAME};
use miri::error::syntax::Span;
use miri::mir::type_facts::{
    is_unresolved_element, ClassDefinition, DropPlan, DroppedMembers, EnumDefinition,
    GenericDefinition, StructDefinition, TypeDefinition, TypeFacts,
};
use miri::type_checker::context::{AliasDefinition, TraitDefinition};
use std::collections::{BTreeMap, HashMap};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn string() -> Type {
    ty(TypeKind::String)
}

fn parameter(name: &str) -> Type {
    ty(TypeKind::Generic(
        name.to_string(),
        miri::ast::bounds::Bounds::none(),
    ))
}

fn custom(name: &str) -> TypeKind {
    TypeKind::Custom(name.to_string(), None)
}

fn type_expr(kind: TypeKind) -> TypeArgument {
    TypeArgument::new(
        0,
        TypeArgKind::Type(Box::new(ty(kind)), false),
        Span::default(),
    )
}

/// `name<component>`: the form a compiler-known inline value type is
/// written in.
fn custom_of(name: &str, component: TypeKind) -> TypeKind {
    TypeKind::Custom(name.to_string(), Some(vec![type_expr(component)]))
}

fn generic_param(name: &str) -> GenericDefinition {
    GenericDefinition::unbounded(name.to_string())
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

/// The facts over the types `source` declares, with `registry` the
/// instantiations whose method bodies were lowered.
fn facts_of(source: &str, registry: HashMap<String, Vec<Vec<Type>>>) -> TypeFacts {
    let result = type_checker_result(source);
    let definitions = result.type_checker.type_definitions().clone();
    TypeFacts::new(definitions, registry, Default::default(), []).unwrap()
}

/// The counted fields a struct or class drop plan releases, by index and kind.
fn released_fields(plan: &DropPlan) -> Vec<(usize, TypeKind)> {
    match &plan.members {
        DroppedMembers::Fields(fields) => fields
            .iter()
            .map(|slot| (slot.index, slot.ty.kind.clone()))
            .collect(),
        DroppedMembers::Variants(_) => panic!("a struct or class plan lists fields"),
    }
}

/// The counted payloads an enum drop plan releases, by variant.
fn released_variants(plan: &DropPlan) -> Vec<(usize, Vec<(usize, TypeKind)>)> {
    match &plan.members {
        DroppedMembers::Variants(variants) => (variants.iter())
            .map(|(variant, slots)| {
                let slots = slots.iter().map(|slot| (slot.index, slot.ty.kind.clone()));
                (*variant, slots.collect())
            })
            .collect(),
        DroppedMembers::Fields(_) => panic!("an enum plan lists variants"),
    }
}

const BOX: &str = "
class Box<T>
    var value T
    var count int

fn main()
    return
";

#[test]
fn a_box_of_strings_releases_its_value_field_and_a_box_of_ints_releases_nothing() {
    let registry = HashMap::from([(
        "Box".to_string(),
        vec![vec![string()], vec![ty(TypeKind::Int)]],
    )]);
    let facts = facts_of(BOX, registry);

    let strings = facts.drop_plan("Box", Some(&[string()])).unwrap();
    assert_eq!(released_fields(strings), vec![(0, TypeKind::String)]);
    assert_eq!(strings.hook, None);
    let ints = facts.drop_plan("Box", Some(&[ty(TypeKind::Int)])).unwrap();
    assert!(!ints.releases_anything(), "a `Box<int>` holds two words");
}

#[test]
fn the_shared_thunk_of_a_generic_class_skips_a_field_written_at_its_parameter() {
    let facts = facts_of(BOX, HashMap::new());

    let shared = facts.drop_plan("Box", None).unwrap();
    assert!(
        released_fields(shared).is_empty(),
        "`value T` names nothing the shared thunk could release"
    );
}

#[test]
fn a_child_of_an_instantiated_base_releases_the_inherited_field_at_the_base_argument() {
    let facts = facts_of(
        "
class Base<T>
    var value T

class Child extends Base<String>
    var label int

fn main()
    return
",
        HashMap::new(),
    );

    let child = facts.drop_plan("Child", None).unwrap();
    assert_eq!(released_fields(child), vec![(0, TypeKind::String)]);
}

#[test]
fn a_partly_bound_result_releases_the_bound_payload_and_skips_the_unbound_one() {
    let unbound = parameter("E");
    let registry = HashMap::from([("Result".to_string(), vec![vec![string(), unbound.clone()]])]);
    let facts = facts_of(BOX, registry);

    let plan = facts
        .drop_plan("Result", Some(&[string(), unbound]))
        .unwrap();
    // Variants are numbered in name order: `Err` is 0, `Ok` is 1.
    assert_eq!(
        released_variants(plan),
        vec![(1, vec![(0, TypeKind::String)])]
    );
}

#[test]
fn a_type_with_a_drop_hook_names_it_in_its_plan() {
    let facts = facts_of(
        "
class Handle
    var id int

    fn drop(self)
        return

fn main()
    return
",
        HashMap::new(),
    );

    let plan = facts.drop_plan("Handle", None).unwrap();
    assert!(plan.hook.is_some());
    assert!(
        plan.releases_anything(),
        "the hook runs before the block is freed"
    );
}

/// The counted payloads the shared drop thunk of the enum `shape` releases.
fn released_payloads(shape: EnumDefinition) -> Vec<(usize, Vec<(usize, TypeKind)>)> {
    let table = defs([("Shape", TypeDefinition::Enum(shape))]);
    let facts = TypeFacts::new(table, HashMap::new(), Default::default(), []).unwrap();
    released_variants(facts.drop_plan("Shape", None).unwrap())
}

#[test]
fn test_bare_generic_element_is_unresolved() {
    let elem = TypeKind::Generic("T".to_string(), miri::ast::bounds::Bounds::none());
    assert!(is_unresolved_element(&elem, &HashMap::new()));
}

#[test]
fn test_element_naming_an_unknown_type_is_unresolved() {
    assert!(
        is_unresolved_element(&custom("Widget"), &HashMap::new()),
        "no __decref_Widget symbol exists when Widget is not in the type table"
    );
}

#[test]
fn test_element_naming_a_generic_parameter_definition_is_unresolved() {
    let table = defs([("T", TypeDefinition::Generic(generic_param("T")))]);
    assert!(is_unresolved_element(&custom("T"), &table));
}

#[test]
fn test_element_naming_a_known_class_is_resolved() {
    let table = defs([("Widget", TypeDefinition::Class(class("Widget")))]);
    assert!(!is_unresolved_element(&custom("Widget"), &table));
}

#[test]
fn test_builtin_collection_element_is_resolved_without_a_definition() {
    // Built-in collections have a shared per-shape decref helper, so they never
    // depend on a per-type symbol.
    for name in ["List", "Array", "Map", "Set"] {
        assert!(
            !is_unresolved_element(&custom(name), &HashMap::new()),
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
            return_type: Some(Box::new(miri::ast::factory::type_expr_non_null(ty(
                TypeKind::Void,
            )))),
        })),
        TypeKind::Meta(Box::new(ty(TypeKind::Int))),
        TypeKind::Option(Box::new(ty(TypeKind::String))),
        TypeKind::Void,
        TypeKind::Error,
        TypeKind::Linear(Box::new(ty(TypeKind::Int))),
    ] {
        assert!(
            !is_unresolved_element(&kind, &HashMap::new()),
            "{kind:?} must resolve"
        );
    }
}

#[test]
fn test_all_type_definition_variants_are_resolved_for_custom_kind() {
    let struct_def = TypeDefinition::Struct(StructDefinition {
        generics: None,
        fields: Vec::new(),
        module: String::new(),
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
            !is_unresolved_element(&custom(name), &table),
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
        !is_unresolved_element(&custom_with_args, &table),
        "Container<Int> must resolve if Container is defined"
    );

    let unknown_with_args = custom_of("UnknownContainer", TypeKind::Int);
    assert!(
        is_unresolved_element(&unknown_with_args, &table),
        "UnknownContainer<Int> must be unresolved when UnknownContainer is absent from definitions"
    );
}

#[test]
fn test_an_optional_of_an_unresolved_parameter_is_unresolved() {
    // A shared drop thunk keeps a `v T?` field only if it can release it; at
    // `T = Node` the optional is the bare `Node` pointer, not a box, so a
    // thunk that cannot tell must skip it exactly as it skips a bare `T`.
    let table = defs([("Node", TypeDefinition::Class(class("Node")))]);
    let parameter = TypeKind::Generic("T".to_string(), miri::ast::bounds::Bounds::none());
    let optional = |kind: TypeKind| TypeKind::Option(Box::new(ty(kind)));
    for kind in [
        optional(parameter.clone()),
        optional(optional(parameter.clone())),
        optional(custom("T")),
    ] {
        assert!(
            is_unresolved_element(&kind, &table),
            "{kind:?} must be skipped like the parameter it wraps"
        );
    }
    for kind in [optional(custom("Node")), optional(TypeKind::Int)] {
        assert!(
            !is_unresolved_element(&kind, &table),
            "{kind:?} is resolved"
        );
    }
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
        released_payloads(shape),
        vec![(1, vec![(0, TypeKind::String)])],
    );
}

#[test]
fn test_managed_field_index_is_the_position_within_its_variant() {
    let shape = enum_def([("Tagged", vec![TypeKind::Int, TypeKind::String])]);
    assert_eq!(
        released_payloads(shape),
        vec![(0, vec![(1, TypeKind::String)])],
        "the DecRef offset is derived from the field's index inside the variant"
    );
}

#[test]
fn test_enum_of_scalar_variants_collects_nothing() {
    let shape = enum_def([("A", vec![TypeKind::Int]), ("B", vec![TypeKind::F64])]);
    assert!(released_payloads(shape).is_empty());
}

/// An atomic written with its scalar component is stored as that scalar, by
/// value inside the payload slot, so a DecRef on it would treat raw bytes as a
/// pointer.
#[test]
fn test_inline_atomic_payload_is_not_collected_as_managed() {
    let shape = enum_def([("Inline", vec![custom_of(ATOMIC_TYPE_NAME, TypeKind::U32)])]);
    assert!(released_payloads(shape).is_empty());
}

/// A `Vec3<f32>` payload is held as its bytes inline in the enum's column,
/// as the atomic beside it is, so dropping the enum releases neither.
#[test]
fn test_vector_payload_is_held_inline_and_not_collected() {
    let vector = custom_of(VEC3_TYPE_NAME, TypeKind::F32);
    let shape = enum_def([(
        "Mixed",
        vec![vector.clone(), custom_of(ATOMIC_TYPE_NAME, TypeKind::U32)],
    )]);
    assert!(
        released_payloads(shape).is_empty(),
        "the vector and the atomic are both inline"
    );
}

/// A declaration that reuses a vector's name without the component it holds is
/// an ordinary type: it is laid out from its own fields and heap-allocated, so
/// its payload position holds a pointer that has to be released.
#[test]
fn test_a_type_reusing_a_vector_name_is_collected_as_managed() {
    let shape = enum_def([("Payload", vec![custom(VEC3_TYPE_NAME)])]);
    assert_eq!(
        released_payloads(shape),
        vec![(0, vec![(0, custom(VEC3_TYPE_NAME))])]
    );
}

#[test]
fn test_managed_class_field_survives_as_managed_in_a_variant() {
    // A user class field inside an enum variant is a pointer and must be
    // DecRef'd, unlike the inline value wrappers above.
    let shape = enum_def([("Wrapped", vec![custom("Widget")])]);
    assert_eq!(
        released_payloads(shape),
        vec![(0, vec![(0, custom("Widget"))])],
    );
}

/// Whether the thunk of `name` is planned to bound its release depth.
fn may_recurse(source: &str, name: &str) -> bool {
    facts_of(source, HashMap::new())
        .drop_plan(name, None)
        .unwrap()
        .may_recurse
}

const MAIN: &str = "\nfn main()\n    return\n";

#[test]
fn a_class_with_only_scalars_and_strings_cannot_recurse() {
    let source = format!("class Flat\n    var v int\n    var s String\n{MAIN}");
    assert!(!may_recurse(&source, "Flat"));
}

#[test]
fn a_class_holding_itself_through_an_optional_may_recurse() {
    let source = format!("class Node\n    var next Node?\n{MAIN}");
    assert!(may_recurse(&source, "Node"));
}

#[test]
fn a_flat_class_holding_a_recursive_one_cannot_itself_recurse() {
    let source =
        format!("class Node\n    var next Node?\n\nclass Holder\n    var head Node\n{MAIN}");
    assert!(may_recurse(&source, "Node"));
    assert!(!may_recurse(&source, "Holder"));
}

#[test]
fn two_classes_holding_each_other_may_each_recurse() {
    let source = format!("class A\n    var b B?\n\nclass B\n    var a A?\n{MAIN}");
    assert!(may_recurse(&source, "A"));
    assert!(may_recurse(&source, "B"));
}

#[test]
fn a_cycle_through_a_list_a_map_or_a_set_element_may_recurse() {
    for (imports, member) in [
        ("use system.collections.list\n\n", "List<Node>"),
        ("use system.collections.map\n\n", "Map<int, Node>"),
        ("", "[Node]"),
    ] {
        let source = format!("{imports}class Node\n    var kids {member}\n{MAIN}");
        assert!(may_recurse(&source, "Node"), "{member}");
    }
}

#[test]
fn a_cycle_through_an_enum_payload_may_recurse() {
    let source = format!("enum Chain\n    End\n    Link(int, Chain)\n{MAIN}");
    assert!(may_recurse(&source, "Chain"));
}

#[test]
fn a_cycle_through_a_base_class_may_recurse() {
    let source = format!(
        "class Base\n    var next Base?\n\nclass Derived extends Base\n    var w int\n{MAIN}"
    );
    assert!(may_recurse(&source, "Base"));
    assert!(may_recurse(&source, "Derived"));
}

#[test]
fn a_trait_or_a_function_member_may_recurse() {
    let traits = format!("trait T\n    fn f() int\n\nclass C\n    var t T\n{MAIN}");
    assert!(may_recurse(&traits, "C"));
    let function = format!("class C\n    var f fn() int\n{MAIN}");
    assert!(may_recurse(&function, "C"));
}

#[test]
fn a_box_recurses_at_a_recursive_argument_only_through_a_cycle_it_is_on() {
    let source = format!(
        "class Box<T>\n    var value T\n\nclass Node\n    var next Box<Node>?\n\nclass Leaf\n    var s String\n{MAIN}"
    );
    let node = ty(custom("Node"));
    let leaf = ty(custom("Leaf"));
    let registry = HashMap::from([(
        "Box".to_string(),
        vec![
            vec![ty(TypeKind::Int)],
            vec![leaf.clone()],
            vec![node.clone()],
        ],
    )]);
    let facts = facts_of(&source, registry);
    let of = |arg: &Type| {
        facts
            .drop_plan("Box", Some(std::slice::from_ref(arg)))
            .unwrap()
            .may_recurse
    };
    assert!(!of(&ty(TypeKind::Int)), "a Box<int> holds a word");
    assert!(!of(&leaf), "a Box<Leaf> holds a flat class");
    assert!(of(&node), "a Box<Node> holds a Node holding a Box<Node>");
    assert!(facts.drop_plan("Node", None).unwrap().may_recurse);
}

#[test]
fn the_shared_thunk_of_a_generic_class_may_recurse() {
    let source = format!("class Box<T>\n    var value T\n{MAIN}");
    assert!(may_recurse(&source, "Box"));
}
