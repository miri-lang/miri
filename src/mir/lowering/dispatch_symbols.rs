// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The function symbols a class's dispatch resolves to where no MIR call spells
//! them: the slots of its vtable, the drop hook releasing it runs, the `clone`
//! a container copies it through and the methods a container asks of its
//! elements. Codegen emits those references and the pipeline compiles every
//! body they name, so both read them from here.
//!
//! This is also where the rule for which trait default a class inherits
//! lives, and what a copy of one compiled under the class's name is typed at:
//! static dispatch, vtable slots and the per-class copies the pipeline lowers
//! all choose by [`trait_default_among`]. A container's equality thunk does
//! not yet: it asks only the class chain for `equals`, so an `equals` a class
//! inherits from a trait default is not what a set or map matches by.

use super::method_dispatch::resolve_inherited_method;
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{Type, CLONE_METHOD_NAME, EQUALS_METHOD_NAME, ORDERING_METHOD_NAME};
use crate::type_checker::context::{
    class_needs_vtable, collect_trait_vtable_methods, find_trait_default_method, ClassDefinition,
    MethodInfo, TypeDefinition,
};
use crate::type_checker::utils::has_drop_hook;
use crate::type_checker::TypeChecker;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};

/// The methods a container's runtime thunk asks of two class elements: the
/// ordering it sorts by and the equality it matches by.
pub const ELEMENT_METHOD_NAMES: [&str; 2] = [ORDERING_METHOD_NAME, EQUALS_METHOD_NAME];

/// The methods a runtime thunk calls on a class instance, besides its drop hook.
const THUNK_METHOD_NAMES: [&str; 3] = {
    let [ordering, equals] = ELEMENT_METHOD_NAMES;
    [CLONE_METHOD_NAME, ordering, equals]
};

/// The first of `traits`, in the order a class lists them, to supply a default
/// `method_name` — itself or through a parent trait — paired with the trait
/// that declares the default and its signature.
pub fn trait_default_among<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    traits: &'td [String],
    method_name: &str,
) -> Option<(&'td str, &'td MethodInfo)> {
    traits
        .iter()
        .find_map(|trait_name| find_trait_default_method(type_defs, trait_name, method_name))
}

/// The trait whose default `method_name` a `class_name` receiver inherits: the
/// class's own traits by [`trait_default_among`], then each base's in turn.
pub fn inherited_trait_default<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
) -> Option<&'td str> {
    class_chain(type_defs, class_name)
        .find_map(|(_, class)| trait_default_among(type_defs, &class.traits, method_name))
        .map(|(trait_name, _)| trait_name)
}

/// The substitution the copy of `method_name` compiled under `class_name` for
/// one instantiation reads its body through, given the class's own parameters
/// at that instantiation.
///
/// A method the class declares reads `class_substitution` as it is. A trait
/// default it inherits reads it with the trait's parameters laid over it at
/// what the chain pins them to, as
/// [`TypeChecker::trait_default_substitution`] states.
pub fn instantiation_substitution<'s>(
    type_checker: &TypeChecker,
    class_name: &str,
    method_name: &str,
    class_substitution: &'s HashMap<String, Type>,
) -> Cow<'s, HashMap<String, Type>> {
    let type_defs = type_checker.type_definitions();
    let is_declared = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(class)) if class.methods.contains_key(method_name)
    );
    let default_owner = (!is_declared)
        .then(|| inherited_trait_default(type_defs, class_name, method_name))
        .flatten();
    match default_owner {
        Some(trait_name) => Cow::Owned(type_checker.trait_default_substitution(
            class_name,
            trait_name,
            class_substitution,
        )),
        None => Cow::Borrowed(class_substitution),
    }
}

/// Every method a trait anywhere in `class_name`'s chain gives a default body
/// that no class in the chain declares, each paired with the trait
/// [`inherited_trait_default`] chooses, sorted by method name.
pub fn inherited_trait_defaults<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<(&'td str, &'td str)> {
    let chain: Vec<&ClassDefinition> = class_chain(type_defs, class_name)
        .map(|(_, class)| class)
        .collect();
    let traits = chain.iter().flat_map(|class| class.traits.iter());
    defaulted_method_names(type_defs, traits)
        .into_iter()
        .filter(|method| {
            !chain
                .iter()
                .any(|class| class.methods.contains_key(*method))
        })
        .filter_map(|method| {
            inherited_trait_default(type_defs, class_name, method).map(|owner| (method, owner))
        })
        .collect()
}

/// Every method `class_name`'s own trait clauses give a default body that the
/// class does not declare itself, each paired with the trait
/// [`trait_default_among`] chooses, sorted by method name. These are the
/// defaults compiled under the class's own name per instantiation.
pub fn own_trait_defaults<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<(&'td str, &'td str)> {
    let Some(TypeDefinition::Class(class)) = type_defs.get(class_name) else {
        return Vec::new();
    };
    defaulted_method_names(type_defs, class.traits.iter())
        .into_iter()
        .filter(|method| !class.methods.contains_key(*method))
        .filter_map(|method| {
            trait_default_among(type_defs, &class.traits, method).map(|(owner, _)| (method, owner))
        })
        .collect()
}

/// The names of every default body `traits` and their parent traits declare.
fn defaulted_method_names<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    traits: impl Iterator<Item = &'td String>,
) -> BTreeSet<&'td str> {
    let mut pending: Vec<&str> = traits.map(String::as_str).collect();
    let mut visited = HashSet::new();
    let mut names = BTreeSet::new();
    while let Some(trait_name) = pending.pop() {
        if !visited.insert(trait_name) {
            continue;
        }
        let Some(TypeDefinition::Trait(trait_def)) = type_defs.get(trait_name) else {
            continue;
        };
        names.extend(
            trait_def
                .methods
                .iter()
                .filter(|(_, method)| !method.is_abstract)
                .map(|(name, _)| name.as_str()),
        );
        pending.extend(trait_def.parent_traits.iter().map(String::as_str));
    }
    names
}

/// `class_name` and every class it extends, nearest first, each with its name.
///
/// Bounded by the number of definitions, so a circular `extends` — reported
/// where the class is declared — cannot hang the walk.
fn class_chain<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> impl Iterator<Item = (&'td str, &'td ClassDefinition)> {
    let mut next = class_entry(type_defs, class_name);
    std::iter::from_fn(move || {
        let (name, class) = next?;
        next = class
            .base_class
            .as_deref()
            .and_then(|base| class_entry(type_defs, base));
        Some((name, class))
    })
    .take(type_defs.len() + 1)
}

/// The class registered under `name`, with the name as the table holds it.
fn class_entry<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    name: &str,
) -> Option<(&'td str, &'td ClassDefinition)> {
    let (name, TypeDefinition::Class(class)) = type_defs.get_key_value(name)? else {
        return None;
    };
    Some((name.as_str(), class))
}

/// The concrete (non-abstract) classes that participate in virtual dispatch,
/// sorted by name so codegen emits their vtables in one order. Generic classes
/// are included: their slots name bodies shared by every instantiation.
pub fn collect_classes_needing_vtable(
    type_defs: &HashMap<String, TypeDefinition>,
) -> Vec<(&str, &ClassDefinition)> {
    let mut classes: Vec<(&str, &ClassDefinition)> = type_defs
        .iter()
        .filter_map(|(name, def)| {
            let TypeDefinition::Class(class) = def else {
                return None;
            };
            (!class.is_abstract && class_needs_vtable(name, type_defs))
                .then_some((name.as_str(), class))
        })
        .collect();
    classes.sort_unstable_by_key(|(name, _)| *name);
    classes
}

/// The method names of `class_name`'s vtable slots: every method its abstract
/// ancestors declare (constructors and statics aside) and every method a trait
/// in its chain requires, sorted so slot indices are deterministic.
pub fn collect_vtable_methods<'td>(
    class_name: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> Vec<&'td str> {
    let chain: Vec<&ClassDefinition> = class_chain(type_defs, class_name)
        .map(|(_, class)| class)
        .collect();
    let mut methods: BTreeSet<&str> = chain
        .iter()
        .filter(|class| class.is_abstract)
        .flat_map(|class| class.methods.iter())
        .filter(|(_, info)| !info.is_constructor && !info.is_static)
        .map(|(name, _)| name.as_str())
        .collect();
    for trait_name in chain.iter().flat_map(|class| class.traits.iter()) {
        methods.extend(collect_trait_vtable_methods(type_defs, trait_name));
    }
    methods.into_iter().collect()
}

/// The symbol `class_name`'s vtable slot for `method_name` names: the nearest
/// class in its chain that gives the method a body, else the trait default
/// [`inherited_trait_default`] chooses.
///
/// A default a class without type parameters of its own inherits resolves to
/// the class's own copy, `"{class_name}_{method_name}"`: the pipeline lowers
/// one for every concrete class that declares the method nowhere in its chain,
/// typed at what its clauses pin the trait's parameters to, and a static call
/// on the class names the same body. A generic class's copy leaves its own
/// parameters open and is called at their width rather than the trait's, so
/// its slot — like one whose chain declares the method abstractly — names the
/// trait's shared `"{Trait}_{method_name}"`.
pub fn resolve_vtable_method(
    class_name: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<String> {
    let mut is_declared_in_chain = false;
    for (name, class) in class_chain(type_defs, class_name) {
        if let Some(method) = class.methods.get(method_name) {
            if !method.is_abstract {
                return Some(format!("{name}_{method_name}"));
            }
            is_declared_in_chain = true;
        }
    }
    let defining_trait = inherited_trait_default(type_defs, class_name, method_name)?;
    // TODO: a generic class's slot names the trait's shared body, compiled at
    // the trait's bare parameters, so a default holding a managed local at such
    // a parameter releases it as an opaque value. MIR verification is opt-in,
    // so the program compiles and misbehaves at runtime: `class Impl<T>
    // implements Op<T>` whose default `keep(a T, b T) T` reassigns a local
    // `var x T`, called through an `Op<String>` parameter, double-frees and
    // prints nothing. Settling it needs each instantiation's slot to name a
    // body specialized to that instantiation's types.
    let is_generic_class = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(class)) if class.generics.is_some()
    );
    let owner = if is_declared_in_chain || is_generic_class {
        defining_trait
    } else {
        class_name
    };
    Some(format!("{owner}_{method_name}"))
}

/// Every function symbol some vtable slot names.
pub fn vtable_slot_symbols(type_defs: &HashMap<String, TypeDefinition>) -> HashSet<String> {
    collect_classes_needing_vtable(type_defs)
        .into_iter()
        .flat_map(|(class_name, _)| {
            collect_vtable_methods(class_name, type_defs)
                .into_iter()
                .filter_map(move |method| resolve_vtable_method(class_name, method, type_defs))
        })
        .collect()
}

/// The symbol of the body a call to `method_name` on a `type_name` receiver
/// names, resolved as static dispatch resolves it, or `None` when nothing in
/// the type's chain declares the method.
pub fn inherited_method_symbol(
    type_defs: &HashMap<String, TypeDefinition>,
    type_name: &str,
    method_name: &str,
) -> Option<String> {
    resolve_inherited_method(type_defs, type_name, method_name)
        .map(|(owner, _)| format!("{owner}_{method_name}"))
}

/// The symbol a call to `method_name` on a `type_name` receiver names:
/// [`inherited_method_symbol`] where the chain declares the method, else the
/// type's own `"{type_name}_{method_name}"`.
pub fn method_symbol(
    type_defs: &HashMap<String, TypeDefinition>,
    type_name: &str,
    method_name: &str,
) -> String {
    inherited_method_symbol(type_defs, type_name, method_name)
        .unwrap_or_else(|| format!("{type_name}_{method_name}"))
}

/// The symbol of the drop hook releasing a `type_name` value runs, or `None`
/// when the type has none. A class reaches the hook its chain declares or
/// inherits from a trait; a struct names its own.
pub fn drop_hook_symbol(
    type_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<String> {
    if !has_drop_hook(type_name, type_defs) {
        return None;
    }
    Some(method_symbol(type_defs, type_name, DROP_HOOK_NAME))
}

/// The symbol of the `clone` a copy of a `type_name` element calls.
pub fn clone_method_symbol(type_name: &str, type_defs: &HashMap<String, TypeDefinition>) -> String {
    method_symbol(type_defs, type_name, CLONE_METHOD_NAME)
}

/// Every function symbol codegen names without a MIR call carrying it: each
/// vtable slot, and for every class the drop hook, `clone` and element
/// methods its runtime thunks call.
///
/// A thunk's own predicate can still skip a class, so this may name a body no
/// thunk ends up calling; that costs one body compiled needlessly, never one
/// missing at link time.
pub fn synthesized_references(type_defs: &HashMap<String, TypeDefinition>) -> HashSet<String> {
    let mut symbols = vtable_slot_symbols(type_defs);
    for (type_name, definition) in type_defs {
        let TypeDefinition::Class(_) = definition else {
            continue;
        };
        symbols.extend(drop_hook_symbol(type_name, type_defs));
        symbols.extend(
            THUNK_METHOD_NAMES
                .iter()
                .filter_map(|method| inherited_method_symbol(type_defs, type_name, method)),
        );
    }
    symbols
}
