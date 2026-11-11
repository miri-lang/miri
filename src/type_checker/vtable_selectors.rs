// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which method names a vtable slot stands for: the one answer to "may a call
//! to this method reach a body through a vtable slot". MIR's shared slot
//! numbering is built from it, and a method whose name is not among them is
//! only ever called by name, so it may own the parameters it keeps
//! ([`super::kept_parameters`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::context::{class_ancestry, ClassDefinition, MethodInfo, TypeDefinition};

/// Every method name a vtable slot stands for, sorted: each instance method a
/// trait or an abstract class declares, and each one a class overrides from
/// a class it extends, constructors, statics and generic methods aside.
///
/// The one answer to "may a call to this method name reach a body through a
/// vtable slot": the shared slot numbering is built from it, and a method
/// whose name is not among them is only ever called by name.
pub(crate) fn vtable_selectors(type_defs: &HashMap<String, TypeDefinition>) -> BTreeSet<&str> {
    let declared = type_defs
        .values()
        .filter_map(dispatching_methods)
        .flat_map(dispatched_method_names);
    declared.chain(overridden_method_names(type_defs)).collect()
}

/// The methods of a definition whose instance methods take vtable slots: a
/// trait's, or an abstract class's.
fn dispatching_methods(definition: &TypeDefinition) -> Option<&BTreeMap<String, MethodInfo>> {
    match definition {
        TypeDefinition::Trait(trait_def) => Some(&trait_def.methods),
        TypeDefinition::Class(class) => class.is_abstract.then_some(&class.methods),
        TypeDefinition::Struct(_)
        | TypeDefinition::Enum(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_) => None,
    }
}

/// The name of every method some class overrides: one it declares that a
/// class it extends, directly or further up, declares too.
pub(crate) fn overridden_method_names(
    type_defs: &HashMap<String, TypeDefinition>,
) -> impl Iterator<Item = &str> {
    type_defs
        .values()
        .filter_map(|definition| match definition {
            TypeDefinition::Class(class) => Some(class),
            TypeDefinition::Trait(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_) => None,
        })
        .flat_map(move |class| overrides_of(class, type_defs))
}

/// The methods `class` declares that a class it extends declares too.
fn overrides_of<'td>(
    class: &'td ClassDefinition,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> impl Iterator<Item = &'td str> {
    let ancestors: Vec<&ClassDefinition> = class
        .base_class
        .as_deref()
        .map(|base| {
            class_ancestry(base, type_defs)
                .map(|(_, def)| def)
                .collect()
        })
        .unwrap_or_default();
    dispatched_method_names(&class.methods).filter(move |method| {
        ancestors
            .iter()
            .any(|ancestor| ancestor.methods.get(*method).is_some_and(takes_vtable_slot))
    })
}

/// The names among `methods` a vtable slot can stand for.
pub(crate) fn dispatched_method_names(
    methods: &BTreeMap<String, MethodInfo>,
) -> impl Iterator<Item = &str> {
    methods
        .iter()
        .filter(|(_, info)| takes_vtable_slot(info))
        .map(|(name, _)| name.as_str())
}

/// Whether a method takes a vtable slot: every one but the constructors,
/// statics and generic methods. A generic method is compiled once per type
/// argument a call binds, and a slot holds one body, so every call to one is
/// static.
pub(crate) fn takes_vtable_slot(info: &MethodInfo) -> bool {
    !info.is_constructor && !info.is_static && !info.declares_type_parameters()
}
