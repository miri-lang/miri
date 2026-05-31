// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::codegen::cranelift::FunctionTranslator;
use miri::type_checker::context::TypeDefinition;
use std::collections::HashMap;

use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::error::syntax::Span;
use miri::type_checker::context::{ClassDefinition, MethodInfo, TraitDefinition};
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
        FunctionTranslator::resolve_vtable_method("Derived", "greet", &defs),
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
        FunctionTranslator::resolve_vtable_method("Mid", "greet", &defs),
        Some("Base_greet".to_string()),
    );
}

#[test]
fn resolve_vtable_method_falls_back_to_default_trait_impl() {
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
        FunctionTranslator::resolve_vtable_method("Impl", "greet", &defs),
        Some("Greeter_greet".to_string()),
    );
}

#[test]
fn resolve_vtable_method_returns_none_when_method_absent() {
    let standalone = class("Standalone", None, &[], &[], false);
    let defs = make_defs([("Standalone".to_string(), TypeDefinition::Class(standalone))]);
    assert!(FunctionTranslator::resolve_vtable_method("Standalone", "missing", &defs).is_none());
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
    assert!(FunctionTranslator::resolve_vtable_method("AliasName", "any", &defs).is_none());
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
    let methods = FunctionTranslator::collect_vtable_methods("Derived", &defs);
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
    let methods = FunctionTranslator::collect_vtable_methods("Derived", &defs);
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
    let methods = FunctionTranslator::collect_vtable_methods("Impl", &defs);
    assert!(
        methods.contains(&"greet"),
        "expected 'greet' from trait, got {methods:?}",
    );
}
