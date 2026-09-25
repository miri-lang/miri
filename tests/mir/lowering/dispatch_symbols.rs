// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Vtable slots, the trait default a class inherits, and the symbols codegen
//! names outside any MIR call.

use miri::mir::lowering::dispatch_symbols::{
    collect_vtable_methods, inherited_trait_default, instantiation_substitution, method_symbol,
    resolve_vtable_method, synthesized_references, vtable_slot_index, vtable_slot_symbols,
    VtableLayout, ELEMENT_METHOD_NAMES,
};
use miri::type_checker::context::TypeDefinition;
use std::collections::HashMap;

use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::ast::MemberVisibility;
use miri::error::syntax::Span;
use miri::type_checker::context::{
    ClassDefinition, GenericDefinition, MethodInfo, TraitDefinition,
};
use std::collections::BTreeMap;

fn span() -> Span {
    Span::new(0, 0)
}

fn void_type() -> Type {
    Type::new(TypeKind::Void, span())
}

fn method(is_abstract: bool, is_constructor: bool) -> MethodInfo {
    MethodInfo {
        params: Vec::new(),
        is_out_flags: Vec::new(),
        return_type: void_type(),
        visibility: MemberVisibility::Public,
        is_constructor,
        is_abstract,
        is_static: false,
        attributes: Vec::new(),
    }
}

fn class(
    name: &str,
    base: Option<&str>,
    traits: &[&str],
    methods: &[(&str, MethodInfo)],
    is_abstract: bool,
) -> ClassDefinition {
    let mut method_map: BTreeMap<String, MethodInfo> = BTreeMap::new();
    for (n, m) in methods {
        method_map.insert(n.to_string(), m.clone());
    }
    ClassDefinition {
        name: name.to_string(),
        generics: None,
        base_class: base.map(String::from),
        base_class_args: None,
        traits: traits.iter().map(|s| s.to_string()).collect(),
        trait_args: std::collections::HashMap::new(),
        fields: Vec::new(),
        methods: method_map,
        module: String::new(),
        is_abstract,
        has_drop: false,
    }
}

fn trait_def(name: &str, parents: &[&str], methods: &[(&str, MethodInfo)]) -> TraitDefinition {
    let mut method_map: BTreeMap<String, MethodInfo> = BTreeMap::new();
    for (n, m) in methods {
        method_map.insert(n.to_string(), m.clone());
    }
    TraitDefinition {
        name: name.to_string(),
        generics: None,
        parent_traits: parents.iter().map(|s| s.to_string()).collect(),
        parent_trait_args: BTreeMap::new(),
        methods: method_map,
        module: String::new(),
    }
}

fn make_defs<I: IntoIterator<Item = (String, TypeDefinition)>>(
    entries: I,
) -> HashMap<String, TypeDefinition> {
    entries.into_iter().collect()
}

#[test]
fn resolve_vtable_method_picks_concrete_override() {
    let base = class("Base", None, &[], &[("greet", method(true, false))], true);
    let derived = class(
        "Derived",
        Some("Base"),
        &[],
        &[("greet", method(false, false))],
        false,
    );
    let defs = make_defs([
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Derived".to_string(), TypeDefinition::Class(derived)),
    ]);
    assert_eq!(
        resolve_vtable_method("Derived", "greet", &defs),
        Some("Derived_greet".to_string()),
    );
}

#[test]
fn resolve_vtable_method_walks_to_base_when_derived_is_abstract() {
    let base = class("Base", None, &[], &[("greet", method(false, false))], false);
    let mid = class(
        "Mid",
        Some("Base"),
        &[],
        &[("greet", method(true, false))],
        true,
    );
    let defs = make_defs([
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Mid".to_string(), TypeDefinition::Class(mid)),
    ]);
    // Mid declares greet abstract — resolver continues to Base.
    assert_eq!(
        resolve_vtable_method("Mid", "greet", &defs),
        Some("Base_greet".to_string()),
    );
}

/// A default a non-generic class inherits resolves to the class's own copy,
/// the body lowered at the types its clauses pin.
#[test]
fn resolve_vtable_method_names_the_class_copy_of_a_trait_default() {
    let trait_with_default = trait_def("Greeter", &[], &[("greet", method(false, false))]);
    let impl_class = class("Impl", None, &["Greeter"], &[], false);
    let defs = make_defs([
        (
            "Greeter".to_string(),
            TypeDefinition::Trait(trait_with_default),
        ),
        ("Impl".to_string(), TypeDefinition::Class(impl_class)),
    ]);
    assert_eq!(
        resolve_vtable_method("Impl", "greet", &defs),
        Some("Impl_greet".to_string()),
    );
}

#[test]
fn resolve_vtable_method_names_the_shared_default_for_a_generic_class() {
    let trait_with_default = trait_def("Greeter", &[], &[("greet", method(false, false))]);
    let mut impl_class = class("Impl", None, &["Greeter"], &[], false);
    impl_class.generics = Some(vec![GenericDefinition {
        name: "T".to_string(),
        constraint: None,
        kind: TypeDeclarationKind::None,
    }]);
    let defs = make_defs([
        (
            "Greeter".to_string(),
            TypeDefinition::Trait(trait_with_default),
        ),
        ("Impl".to_string(), TypeDefinition::Class(impl_class)),
    ]);
    assert_eq!(
        resolve_vtable_method("Impl", "greet", &defs),
        Some("Greeter_greet".to_string()),
    );
}

/// A chain that declares the method abstractly gets no class copy of the
/// default, so the slot names the trait's shared body.
#[test]
fn resolve_vtable_method_names_the_shared_default_under_an_abstract_declaration() {
    let trait_with_default = trait_def("Greeter", &[], &[("greet", method(false, false))]);
    let base = class(
        "Base",
        None,
        &["Greeter"],
        &[("greet", method(true, false))],
        true,
    );
    let impl_class = class("Impl", Some("Base"), &[], &[], false);
    let defs = make_defs([
        (
            "Greeter".to_string(),
            TypeDefinition::Trait(trait_with_default),
        ),
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Impl".to_string(), TypeDefinition::Class(impl_class)),
    ]);
    assert_eq!(
        resolve_vtable_method("Impl", "greet", &defs),
        Some("Greeter_greet".to_string()),
    );
}

#[test]
fn resolve_vtable_method_returns_none_when_method_absent() {
    let standalone = class("Standalone", None, &[], &[], false);
    let defs = make_defs([("Standalone".to_string(), TypeDefinition::Class(standalone))]);
    assert!(resolve_vtable_method("Standalone", "missing", &defs).is_none());
}

#[test]
fn resolve_vtable_method_returns_none_for_non_class_types() {
    let defs = make_defs([(
        "AliasName".to_string(),
        TypeDefinition::Alias(miri::type_checker::context::AliasDefinition {
            template: void_type(),
            generics: None,
        }),
    )]);
    assert!(resolve_vtable_method("AliasName", "any", &defs).is_none());
}

#[test]
fn collect_vtable_methods_orders_alphabetically_and_dedups() {
    let base = class(
        "Base",
        None,
        &[],
        &[
            ("zeta", method(true, false)),
            ("alpha", method(true, false)),
        ],
        true,
    );
    let derived = class(
        "Derived",
        Some("Base"),
        &[],
        &[
            ("alpha", method(false, false)),
            ("zeta", method(false, false)),
        ],
        false,
    );
    let defs = make_defs([
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Derived".to_string(), TypeDefinition::Class(derived)),
    ]);
    let methods = collect_vtable_methods("Derived", &defs);
    assert_eq!(methods, vec!["alpha", "zeta"]);
}

#[test]
fn collect_vtable_methods_skips_constructors() {
    let base = class(
        "Base",
        None,
        &[],
        &[("init", method(true, true)), ("greet", method(true, false))],
        true,
    );
    let derived = class("Derived", Some("Base"), &[], &[], false);
    let defs = make_defs([
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Derived".to_string(), TypeDefinition::Class(derived)),
    ]);
    let methods = collect_vtable_methods("Derived", &defs);
    assert_eq!(methods, vec!["greet"]);
}

#[test]
fn collect_vtable_methods_merges_trait_required_methods() {
    let trait_def_obj = trait_def("Greeter", &[], &[("greet", method(true, false))]);
    let class_obj = class("Impl", None, &["Greeter"], &[], false);
    let defs = make_defs([
        ("Greeter".to_string(), TypeDefinition::Trait(trait_def_obj)),
        ("Impl".to_string(), TypeDefinition::Class(class_obj)),
    ]);
    let methods = collect_vtable_methods("Impl", &defs);
    assert!(
        methods.contains(&"greet"),
        "expected 'greet' from trait, got {methods:?}",
    );
}

fn static_method() -> MethodInfo {
    MethodInfo {
        is_static: true,
        ..method(false, false)
    }
}

/// The slot a call through `receiver` reads under the layout `defs` gives.
fn slot_of(receiver: &str, method: &str, defs: &HashMap<String, TypeDefinition>) -> Option<usize> {
    vtable_slot_index(&VtableLayout::of(defs), receiver, method, defs)
}

/// `trait A` requires `zeta`, `trait B` requires `alpha`, and `class C`
/// implements both.
fn two_traits_one_class() -> HashMap<String, TypeDefinition> {
    make_defs([
        (
            "A".to_string(),
            TypeDefinition::Trait(trait_def("A", &[], &[("zeta", method(true, false))])),
        ),
        (
            "B".to_string(),
            TypeDefinition::Trait(trait_def("B", &[], &[("alpha", method(true, false))])),
        ),
        (
            "C".to_string(),
            TypeDefinition::Class(class(
                "C",
                None,
                &["A", "B"],
                &[
                    ("alpha", method(false, false)),
                    ("zeta", method(false, false)),
                ],
                false,
            )),
        ),
    ])
}

#[test]
fn vtable_slot_index_numbers_a_trait_method_across_every_trait() {
    let defs = two_traits_one_class();
    assert_eq!(slot_of("A", "zeta", &defs), Some(1));
    assert_eq!(slot_of("B", "alpha", &defs), Some(0));
}

#[test]
fn vtable_slot_index_agrees_with_the_slot_the_layout_fills() {
    let defs = two_traits_one_class();
    let layout = VtableLayout::of(&defs);
    assert_eq!(layout.slot_count(), 2);
    for method in collect_vtable_methods("C", &defs) {
        let receiver = if method == "zeta" { "A" } else { "B" };
        assert_eq!(
            vtable_slot_index(&layout, receiver, method, &defs),
            layout.slot(method)
        );
    }
}

#[test]
fn vtable_slot_index_is_none_for_a_method_the_receiver_does_not_declare() {
    let defs = two_traits_one_class();
    assert_eq!(slot_of("A", "alpha", &defs), None);
    assert_eq!(slot_of("C", "zeta", &defs), None);
}

#[test]
fn vtable_layout_gives_statics_and_constructors_no_slot() {
    let base = class(
        "Base",
        None,
        &[],
        &[
            ("apex", static_method()),
            ("init", method(false, true)),
            ("area", method(true, false)),
        ],
        true,
    );
    let defs = make_defs([("Base".to_string(), TypeDefinition::Class(base))]);
    let layout = VtableLayout::of(&defs);
    assert_eq!(layout.slot_count(), 1);
    assert_eq!(layout.slot("apex"), None);
    assert_eq!(slot_of("Base", "area", &defs), Some(0));
    assert_eq!(slot_of("Base", "apex", &defs), None);
}

#[test]
fn vtable_layout_counts_an_abstract_receivers_inherited_trait_methods() {
    let named = trait_def(
        "Named",
        &[],
        &[
            ("greet", method(false, false)),
            ("name", method(true, false)),
        ],
    );
    let base = class(
        "Base",
        None,
        &["Named"],
        &[("shout", method(false, false))],
        true,
    );
    let plain = class("Plain", None, &[], &[("aaa", method(false, false))], false);
    let defs = make_defs([
        ("Named".to_string(), TypeDefinition::Trait(named)),
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Plain".to_string(), TypeDefinition::Class(plain)),
    ]);
    assert_eq!(VtableLayout::of(&defs).slot_count(), 3);
    assert_eq!(slot_of("Base", "greet", &defs), Some(0));
    assert_eq!(slot_of("Base", "shout", &defs), Some(2));
}

fn generic(mut class_def: ClassDefinition, param: &str) -> ClassDefinition {
    class_def.generics = Some(vec![GenericDefinition {
        name: param.to_string(),
        constraint: None,
        kind: TypeDeclarationKind::None,
    }]);
    class_def
}

/// `trait Op<T>` defaults `describe`, `abstract class Base<U> implements
/// Op<U>` and `class Box<T> extends Base<T>`: Box's slot names the trait's
/// shared body, which no call spells, so the slot set must.
#[test]
fn vtable_slot_symbols_names_the_shared_default_a_generic_class_inherits() {
    let op = trait_def("Op", &[], &[("describe", method(false, false))]);
    let base = generic(class("Base", None, &["Op"], &[], true), "U");
    let boxed = generic(class("Box", Some("Base"), &[], &[], false), "T");
    let defs = make_defs([
        ("Op".to_string(), TypeDefinition::Trait(op)),
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Box".to_string(), TypeDefinition::Class(boxed)),
    ]);
    assert!(vtable_slot_symbols(&defs).contains("Op_describe"));
}

/// `trait B<T> extends A<T>` with the default in `A`: a class implementing
/// `B` reaches it through the parent-trait chain.
#[test]
fn resolve_vtable_method_finds_a_default_through_a_parent_trait() {
    let parent = trait_def("A", &[], &[("who", method(false, false))]);
    let child = trait_def("B", &["A"], &[]);
    let impl_class = class("Impl", None, &["B"], &[], false);
    let generic_impl = generic(class("GenericImpl", None, &["B"], &[], false), "T");
    let defs = make_defs([
        ("A".to_string(), TypeDefinition::Trait(parent)),
        ("B".to_string(), TypeDefinition::Trait(child)),
        ("Impl".to_string(), TypeDefinition::Class(impl_class)),
        (
            "GenericImpl".to_string(),
            TypeDefinition::Class(generic_impl),
        ),
    ]);
    assert_eq!(
        resolve_vtable_method("Impl", "who", &defs),
        Some("Impl_who".to_string()),
    );
    assert_eq!(
        resolve_vtable_method("GenericImpl", "who", &defs),
        Some("A_who".to_string()),
    );
}

/// Two traits defaulting one method: the first the class lists supplies it,
/// and a vtable slot of a generic class names that same trait's body.
#[test]
fn inherited_trait_default_prefers_the_first_listed_trait() {
    let first = trait_def("First", &[], &[("who", method(false, false))]);
    let second = trait_def("Second", &[], &[("who", method(false, false))]);
    let boxed = generic(class("Box", None, &["First", "Second"], &[], false), "U");
    let defs = make_defs([
        ("First".to_string(), TypeDefinition::Trait(first)),
        ("Second".to_string(), TypeDefinition::Trait(second)),
        ("Box".to_string(), TypeDefinition::Class(boxed)),
    ]);
    assert_eq!(inherited_trait_default(&defs, "Box", "who"), Some("First"));
    assert_eq!(
        resolve_vtable_method("Box", "who", &defs),
        Some("First_who".to_string()),
    );
}

/// An abstract class inheriting a trait's default `drop` releases through a
/// thunk naming the trait's shared body.
#[test]
fn synthesized_references_names_the_drop_hook_an_abstract_class_inherits() {
    let closer = trait_def("Closer", &[], &[("drop", method(false, false))]);
    let base = class("A", None, &["Closer"], &[], true);
    let defs = make_defs([
        ("Closer".to_string(), TypeDefinition::Trait(closer)),
        ("A".to_string(), TypeDefinition::Class(base)),
    ]);
    assert!(synthesized_references(&defs).contains("Closer_drop"));
}

/// A method the chain declares names its declaring class; one it does not
/// names the receiver's own symbol.
#[test]
fn method_symbol_names_the_declaring_class_else_the_type_itself() {
    let base = class("Base", None, &[], &[("area", method(false, false))], false);
    let sub = class("Sub", Some("Base"), &[], &[], false);
    let defs = make_defs([
        ("Base".to_string(), TypeDefinition::Class(base)),
        ("Sub".to_string(), TypeDefinition::Class(sub)),
    ]);
    assert_eq!(method_symbol(&defs, "Sub", "area"), "Base_area");
    assert_eq!(method_symbol(&defs, "Sub", "clone"), "Sub_clone");
}

/// Every method a container's thunk asks of an element, and its `clone`, is
/// a body codegen names without a MIR call.
#[test]
fn synthesized_references_names_each_element_method_and_clone_a_class_declares() {
    let methods: Vec<(&str, MethodInfo)> = ELEMENT_METHOD_NAMES
        .into_iter()
        .chain(["clone"])
        .map(|name| (name, method(false, false)))
        .collect();
    let item = class("Item", None, &[], &methods, false);
    let defs = make_defs([("Item".to_string(), TypeDefinition::Class(item))]);
    let symbols = synthesized_references(&defs);
    for name in ELEMENT_METHOD_NAMES.into_iter().chain(["clone"]) {
        assert!(
            symbols.contains(&format!("Item_{name}")),
            "{name}: {symbols:?}"
        );
    }
}

fn instantiation_kind(source: &str, class: &str, method: &str, param: &str) -> Option<TypeKind> {
    let result = crate::type_checker::utils::type_checker_result(source);
    let class_subs = HashMap::from([("T".to_string(), Type::new(TypeKind::Float, span()))]);
    instantiation_substitution(&result.type_checker, class, method, &class_subs)
        .get(param)
        .map(|ty| ty.kind.clone())
}

const NAME_SHARING_SOURCE: &str = "
trait Op<T>
    fn keep(a T) T
        return a

class Box<T> implements Op<int>
    v T
    fn value() T
        return self.v
";

/// A default reads the trait's parameter at the clause's pin even where the
/// class names its own parameter the same.
#[test]
fn instantiation_substitution_reads_a_default_at_the_trait_pin() {
    let kind = instantiation_kind(NAME_SHARING_SOURCE, "Box", "keep", "T");
    assert_eq!(kind, Some(TypeKind::Int));
}

/// A method the class declares reads the class's own instantiation.
#[test]
fn instantiation_substitution_reads_a_declared_method_at_the_instantiation() {
    let kind = instantiation_kind(NAME_SHARING_SOURCE, "Box", "value", "T");
    assert_eq!(kind, Some(TypeKind::Float));
}
