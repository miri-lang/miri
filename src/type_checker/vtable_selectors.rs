// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which vtable slots a program has, and which slot a call reads: the one
//! answer to "may a call to this method reach a body through a vtable slot,
//! and through which". A slot is numbered within one hierarchy, by the trait
//! or class at its top and the method it stands for, so two hierarchies that
//! share a method name never share a slot. MIR's slot numbering is built
//! from [`vtable_slots`], virtual calls read [`dispatched_slot_root`], and the
//! calling convention of each slot is settled over the bodies filling it
//! ([`super::slot_conventions`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::context::{
    class_ancestry, trait_lineage, ClassDefinition, MethodInfo, TraitDefinition, TypeDefinition,
};

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

/// Every vtable slot of the program, each by the trait or class at the top
/// of the hierarchy numbering it and the method it stands for: each slot a
/// class's vtable fills ([`class_slot_roots`]), and each slot a trait's
/// methods take, which a call through the trait reads even where no class
/// implements it.
///
/// Two hierarchies that share a method name take two slots, so the calling
/// convention one of them settles ([`super::slot_conventions`]) never reaches
/// the other.
pub(crate) fn vtable_slots(type_defs: &HashMap<String, TypeDefinition>) -> BTreeSet<(&str, &str)> {
    let overridden: BTreeSet<&str> = overridden_method_names(type_defs).collect();
    let is_overridden = |method: &str| overridden.contains(method);
    let mut slots = BTreeSet::new();
    for (name, definition) in type_defs {
        match definition {
            TypeDefinition::Class(_) => {
                for (selector, roots) in class_fills(name, type_defs, &is_overridden) {
                    slots.extend(roots.into_iter().map(|root| (root, selector)));
                }
            }
            TypeDefinition::Trait(trait_def) => {
                for selector in dispatched_method_names(&trait_def.methods) {
                    slots
                        .extend(trait_root(name, selector, type_defs).map(|root| (root, selector)));
                }
            }
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_) => {}
        }
    }
    slots
}

/// Each method `class_name`'s vtable stands for ([`vtable_methods`]), with
/// the root of every slot it fills with its body for that method
/// ([`class_slot_roots`]): the one enumeration of what a class's vtable
/// fills, which the slot numbering, the slot conventions and the vtables MIR
/// writes all read. `is_overridden` answers whether some class overrides a
/// method name, computed once by the caller.
pub(crate) fn class_fills<'td>(
    class_name: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
    is_overridden: &dyn Fn(&str) -> bool,
) -> Vec<(&'td str, BTreeSet<&'td str>)> {
    (vtable_methods(class_name, type_defs, is_overridden).into_iter())
        .map(|selector| (selector, class_slot_roots(class_name, selector, type_defs)))
        .collect()
}

/// The root of every slot `class_name`'s vtable fills with its body for
/// `selector`: the topmost class of its chain declaring the method, where a
/// call through a class reaches the method by the vtable, and the root of
/// each trait in its chain's hierarchy that declares it. A class implementing
/// two unrelated traits that both declare the method fills one slot of each.
pub(crate) fn class_slot_roots<'td>(
    class_name: &str,
    selector: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> BTreeSet<&'td str> {
    let class_root = class_root(class_name, selector, type_defs)
        .filter(|root| is_class_dispatched(root, selector, type_defs));
    let mut roots = chain_trait_roots(class_name, selector, type_defs);
    roots.extend(class_root);
    roots
}

/// The root of the slot a call to `selector` on a `receiver` reads, or `None`
/// when the receiver dispatches no such method and the call is static: the
/// trait's root for a trait receiver, and for a class the root of its own
/// chain where a class there declares the method as one a subclass may
/// override, else the root of a trait its chain implements.
///
/// Every class standing behind `receiver` fills the slot named here
/// ([`class_slot_roots`]), since its chain and its traits include the
/// receiver's.
pub(crate) fn dispatched_slot_root<'td>(
    receiver: &str,
    selector: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> Option<&'td str> {
    if !dispatches_through_vtable(receiver, selector, type_defs) {
        return None;
    }
    match type_defs.get(receiver)? {
        TypeDefinition::Trait(_) => trait_root(receiver, selector, type_defs),
        TypeDefinition::Class(_) => class_root(receiver, selector, type_defs)
            .filter(|root| is_class_dispatched(root, selector, type_defs))
            .or_else(|| (chain_trait_roots(receiver, selector, type_defs).into_iter()).next()),
        TypeDefinition::Struct(_)
        | TypeDefinition::Enum(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_) => None,
    }
}

/// Every method name a call on a `receiver` might read through a vtable:
/// each one a class of its chain, or a trait of its chain's or its own
/// hierarchy, declares as one a slot can stand for.
pub(crate) fn dispatch_candidates<'td>(
    receiver: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> BTreeSet<&'td str> {
    let classes: Vec<&ClassDefinition> = (class_ancestry(receiver, type_defs))
        .map(|(_, class)| class)
        .collect();
    let traits = (classes.iter())
        .flat_map(|class| class.traits.iter().map(String::as_str))
        .chain(std::iter::once(receiver));
    let class_methods = (classes.iter()).flat_map(|class| dispatched_method_names(&class.methods));
    let trait_methods = trait_hierarchy(type_defs, traits)
        .flat_map(|(_, trait_def)| dispatched_method_names(&trait_def.methods));
    class_methods.chain(trait_methods).collect()
}

/// The topmost class of `class_name`'s chain declaring `selector` as a
/// method a vtable slot can stand for.
fn class_root<'td>(
    class_name: &str,
    selector: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> Option<&'td str> {
    class_ancestry(class_name, type_defs)
        .filter(|(_, class)| declares_dispatched(&class.methods, selector))
        .last()
        .map(|(name, _)| name)
}

/// Whether a call through the class `root`, which declares `selector`, reads
/// the method from a vtable: `root` is abstract, or a class below it
/// overrides the method.
fn is_class_dispatched(
    root: &str,
    selector: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> bool {
    let is_abstract =
        matches!(type_defs.get(root), Some(TypeDefinition::Class(class)) if class.is_abstract);
    is_abstract || overridden_below(root, selector, type_defs)
}

/// The root of the slot `selector` takes through the trait `trait_name`: the
/// smallest-named trait of its lineage that declares the method and extends
/// no trait that does. `None` when no trait of the lineage declares it.
fn trait_root<'td>(
    trait_name: &str,
    selector: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> Option<&'td str> {
    let declares = |trait_def: &TraitDefinition| declares_dispatched(&trait_def.methods, selector);
    trait_lineage(type_defs, trait_name)
        .filter(|(_, trait_def)| declares(trait_def))
        .map(|(name, _)| name)
        .filter(|name| {
            !trait_lineage(type_defs, name)
                .skip(1)
                .any(|(_, parent)| declares(parent))
        })
        .min()
}

/// The roots of the slots `selector` takes through every trait in the
/// hierarchy of `class_name`'s chain.
fn chain_trait_roots<'td>(
    class_name: &str,
    selector: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> BTreeSet<&'td str> {
    let traits = class_ancestry(class_name, type_defs)
        .flat_map(|(_, class)| class.traits.iter().map(String::as_str));
    trait_hierarchy(type_defs, traits)
        .filter_map(|(name, _)| trait_root(name, selector, type_defs))
        .collect()
}

/// The method names of `class_name`'s filled vtable slots: every method its
/// abstract ancestors declare, every method a trait in its chain requires and
/// every method its chain declares that some class overrides, constructors and
/// statics aside, sorted by name.
///
/// Which slots each fills is [`class_slot_roots`]'s to say.
pub fn collect_vtable_methods<'td>(
    class_name: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> Vec<&'td str> {
    let overridden: BTreeSet<&str> = overridden_method_names(type_defs).collect();
    vtable_methods(class_name, type_defs, &|method| overridden.contains(method))
}

/// [`collect_vtable_methods`], reading which method names some class
/// overrides from `is_overridden`.
fn vtable_methods<'td>(
    class_name: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
    is_overridden: &dyn Fn(&str) -> bool,
) -> Vec<&'td str> {
    let chain: Vec<&ClassDefinition> = class_ancestry(class_name, type_defs)
        .map(|(_, class)| class)
        .collect();
    let abstract_methods = chain
        .iter()
        .filter(|class| class.is_abstract)
        .flat_map(|class| dispatched_method_names(&class.methods));
    let traits = chain
        .iter()
        .flat_map(|class| class.traits.iter().map(String::as_str));
    let trait_methods = trait_hierarchy(type_defs, traits)
        .flat_map(|(_, trait_def)| dispatched_method_names(&trait_def.methods));
    let overridden_in_chain = chain
        .iter()
        .flat_map(|class| dispatched_method_names(&class.methods))
        .filter(|method| is_overridden(method));
    let methods: BTreeSet<&str> = abstract_methods
        .chain(trait_methods)
        .chain(overridden_in_chain)
        .collect();
    methods.into_iter().collect()
}

/// Every trait `roots` name and every parent trait they extend, by
/// [`trait_lineage`]. A trait two roots share is listed once per root, which
/// no caller here — each asks for a set or for any match — can tell apart.
pub(crate) fn trait_hierarchy<'td, 'n>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    roots: impl Iterator<Item = &'n str>,
) -> impl Iterator<Item = (&'td str, &'td TraitDefinition)> {
    let traits: Vec<(&'td str, &'td TraitDefinition)> = roots
        .flat_map(|root| trait_lineage(type_defs, root))
        .collect();
    traits.into_iter()
}

/// Whether `receiver` reaches `method_name` through its vtable: a trait whose
/// hierarchy declares it, an abstract class whose abstract ancestors or whose
/// chain's traits do, or a class some subclass of which overrides it.
fn dispatches_through_vtable(
    receiver: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> bool {
    match type_defs.get(receiver) {
        Some(TypeDefinition::Trait(_)) => {
            traits_dispatch(type_defs, std::iter::once(receiver), method_name)
        }
        Some(TypeDefinition::Class(class)) if class.is_abstract => {
            class_ancestry(receiver, type_defs).any(|(_, class)| {
                (class.is_abstract && declares_dispatched(&class.methods, method_name))
                    || traits_dispatch(
                        type_defs,
                        class.traits.iter().map(String::as_str),
                        method_name,
                    )
            }) || overridden_below(receiver, method_name, type_defs)
        }
        Some(TypeDefinition::Class(_)) => overridden_below(receiver, method_name, type_defs),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

/// Whether a class that extends `receiver`, directly or further down, declares
/// `method_name` as one a vtable slot stands for: a call on a `receiver` may
/// then reach that class's body, so it reads the vtable. A method nothing
/// below the receiver overrides is called statically.
fn overridden_below(
    receiver: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> bool {
    type_defs.iter().any(|(name, definition)| {
        let TypeDefinition::Class(class) = definition else {
            return false;
        };
        name != receiver
            && declares_dispatched(&class.methods, method_name)
            && class_ancestry(name, type_defs).any(|(ancestor, _)| ancestor == receiver)
    })
}

/// Whether a trait `roots` names, or a parent trait of one, declares
/// `method_name` as one a vtable slot stands for.
fn traits_dispatch<'n>(
    type_defs: &HashMap<String, TypeDefinition>,
    roots: impl Iterator<Item = &'n str>,
    method_name: &str,
) -> bool {
    trait_hierarchy(type_defs, roots)
        .any(|(_, trait_def)| declares_dispatched(&trait_def.methods, method_name))
}

/// Whether `methods` declares `method_name` as one a vtable slot stands for.
fn declares_dispatched(methods: &BTreeMap<String, MethodInfo>, method_name: &str) -> bool {
    methods.get(method_name).is_some_and(takes_vtable_slot)
}
