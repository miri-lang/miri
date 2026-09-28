// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The function symbols a class's dispatch resolves to where no MIR call spells
//! them: the slots of its vtable, the drop hook releasing it runs, the `clone`
//! a container copies it through and the methods a container asks of its
//! elements. Codegen emits those references and the pipeline compiles every
//! body they name, so both read them from here.
//!
//! A vtable belongs to one instantiation of a class: the one a constructor
//! builds the instance at, which [`VtableInstance`] names. Its slots name what
//! a static call on that instantiation names, so a call through a trait
//! receiver reaches the body compiled at the instance's own type arguments.
//!
//! Which body a class runs for a method — its chain's or a trait default's —
//! is [`resolve_method_source`]'s to say, in the type checker's layer: static
//! dispatch, vtable slots, the drop hook and the per-class copies the pipeline
//! lowers all map its answer to a symbol here. A container's equality thunk
//! does not yet: it asks only the class chain for `equals`, so an `equals` a
//! class inherits from a trait default is not what a set or map matches by.

use super::method_dispatch::{instantiated_callee, resolve_inherited_method};
use crate::ast::implicit_methods::THUNK_METHOD_NAMES;
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{Type, CLONE_METHOD_NAME};
use crate::mir::dispatch::{dispatched_method_names, takes_vtable_slot};
use crate::mir::symbol::Symbol;
use crate::type_checker::context::{
    class_ancestry, resolve_method_source, trait_lineage, ClassDefinition, MethodInfo,
    MethodSource, TraitDefinition, TypeDefinition,
};
use crate::type_checker::utils::has_drop_hook;
use crate::type_checker::TypeChecker;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) use crate::mir::dispatch::constructed_class;
pub use crate::mir::dispatch::{
    constructed_vtable_symbols, VtableInstance, VtableLayout, ELEMENT_METHOD_NAMES,
};

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
    let default_owner = inherited_default(type_checker.type_definitions(), class_name, method_name);
    match default_owner {
        Some((_, trait_name)) => Cow::Owned(type_checker.trait_default_substitution(
            class_name,
            trait_name,
            class_substitution,
        )),
        None => Cow::Borrowed(class_substitution),
    }
}

/// The class whose `implements` list supplies the default a `class_name`
/// receiver runs for `method_name`, with the trait declaring it, or `None`
/// when a class in the chain gives the method its body or nothing does.
fn inherited_default<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
) -> Option<(&'td str, &'td str)> {
    match resolve_method_source(type_defs, class_name, method_name)? {
        MethodSource::Default(default) => Some((default.class_level, default.trait_name)),
        MethodSource::Declared { .. } | MethodSource::AbstractOnly { .. } => None,
    }
}

/// Every method whose shared body can be compiled under `class_name`'s own
/// name: the ones it or an ancestor declares — an abstract ancestor's are
/// re-lowered per concrete class — and the trait defaults it inherits.
pub fn methods_compiled_under<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> impl Iterator<Item = &'td str> {
    let declared = class_ancestry(class_name, type_defs)
        .flat_map(|(_, class)| class.methods.keys().map(String::as_str));
    let defaulted = inherited_trait_defaults(type_defs, class_name)
        .into_iter()
        .map(|(method, _)| method);
    declared.chain(defaulted)
}

/// Every method a trait anywhere in `class_name`'s chain gives a default body
/// that no class in the chain gives a body, each paired with the trait
/// [`resolve_method_source`] chooses, sorted by method name.
pub fn inherited_trait_defaults<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<(&'td str, &'td str)> {
    let traits = class_ancestry(class_name, type_defs).flat_map(|(_, class)| class.traits.iter());
    defaulted_method_names(type_defs, traits)
        .into_iter()
        .filter_map(|method| {
            inherited_default(type_defs, class_name, method).map(|(_, owner)| (method, owner))
        })
        .collect()
}

/// Every default [`inherited_trait_defaults`] lists for `class_name` that the
/// class's own trait clauses supply rather than a base's, sorted by method
/// name. These are the defaults compiled under the class's own name per
/// instantiation; one a class in the chain gives a body is not among them.
pub fn own_trait_defaults<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<(&'td str, &'td str)> {
    let Some(TypeDefinition::Class(class)) = type_defs.get(class_name) else {
        return Vec::new();
    };
    defaulted_method_names(type_defs, class.traits.iter())
        .into_iter()
        .filter_map(
            |method| match inherited_default(type_defs, class_name, method) {
                Some((class_level, owner)) if class_level == class_name => Some((method, owner)),
                Some(_) | None => None,
            },
        )
        .collect()
}

/// The names of every default body `traits` and their parent traits declare.
fn defaulted_method_names<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    traits: impl Iterator<Item = &'td String>,
) -> BTreeSet<&'td str> {
    trait_hierarchy(type_defs, traits.map(String::as_str))
        .flat_map(|trait_def| trait_def.methods.iter())
        .filter(|(_, method)| !method.is_abstract)
        .map(|(name, _)| name.as_str())
        .collect()
}

/// Every trait `roots` name and every parent trait they extend, by
/// [`trait_lineage`]. A trait two roots share is listed once per root, which
/// no caller here — each asks for a set or for any match — can tell apart.
fn trait_hierarchy<'td, 'n>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    roots: impl Iterator<Item = &'n str>,
) -> impl Iterator<Item = &'td TraitDefinition> {
    let traits: Vec<&'td TraitDefinition> = roots
        .flat_map(|root| trait_lineage(type_defs, root).map(|(_, trait_def)| trait_def))
        .collect();
    traits.into_iter()
}

/// The slots of an instance's vtable. They name what a static call on the
/// instantiation names, which only method dispatch can resolve, so they are
/// answered here rather than where [`VtableInstance`] is defined.
impl VtableInstance {
    /// Every method this vtable has a slot for, with the symbol the slot names
    /// when it is filled, in the order [`collect_vtable_methods`] lists them.
    pub fn slot_targets<'td>(
        &self,
        type_defs: &'td HashMap<String, TypeDefinition>,
    ) -> Vec<(&'td str, Option<String>)> {
        collect_vtable_methods(self.class(), type_defs)
            .into_iter()
            .map(|method| (method, self.slot_target(method, type_defs)))
            .collect()
    }

    /// The symbol this vtable's slot for `method_name` names: the body a
    /// static call on the instantiation names where one is compiled per
    /// instantiation, else what [`resolve_vtable_method`] resolves for the
    /// class. `None` when nothing in the chain gives the method a body.
    fn slot_target(
        &self,
        method_name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        instantiated_callee(type_defs, self.class(), self.args(), method_name)
            .map(|callee| callee.symbol)
            .or_else(|| resolve_vtable_method(self.class(), method_name, type_defs))
    }
}

/// The method names of `class_name`'s filled vtable slots: every method its
/// abstract ancestors declare and every method a trait in its chain requires,
/// constructors and statics aside, sorted by name.
///
/// Where each lands in the vtable is [`VtableLayout::slot`]'s to say.
pub fn collect_vtable_methods<'td>(
    class_name: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
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
        .flat_map(|trait_def| dispatched_method_names(&trait_def.methods));
    let methods: BTreeSet<&str> = abstract_methods.chain(trait_methods).collect();
    methods.into_iter().collect()
}

/// The slot of `layout` a call to `method_name` through a `receiver` — a
/// trait or an abstract class — reads, or `None` when the receiver dispatches
/// no such method and the call is static.
pub fn vtable_slot_index(
    layout: &VtableLayout,
    receiver: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<usize> {
    dispatches_through_vtable(receiver, method_name, type_defs)
        .then(|| layout.slot(method_name))
        .flatten()
}

/// Whether `receiver` reaches `method_name` through its vtable: a trait whose
/// hierarchy declares it, or an abstract class whose abstract ancestors or
/// whose chain's traits do.
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
            })
        }
        Some(
            TypeDefinition::Class(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

/// Whether a trait `roots` names, or a parent trait of one, declares
/// `method_name` as one a vtable slot stands for.
fn traits_dispatch<'n>(
    type_defs: &HashMap<String, TypeDefinition>,
    roots: impl Iterator<Item = &'n str>,
    method_name: &str,
) -> bool {
    trait_hierarchy(type_defs, roots)
        .any(|trait_def| declares_dispatched(&trait_def.methods, method_name))
}

/// Whether `methods` declares `method_name` as one a vtable slot stands for.
fn declares_dispatched(methods: &BTreeMap<String, MethodInfo>, method_name: &str) -> bool {
    methods.get(method_name).is_some_and(takes_vtable_slot)
}

/// The symbol the slot for `method_name` names in `class_name`'s vtable where
/// no per-instantiation body applies: the nearest class in its chain that
/// gives the method a body, else the trait default [`resolve_method_source`]
/// chooses.
///
/// A default a class without type parameters of its own inherits resolves to
/// the class's own copy, `"{class_name}_{method_name}"`: the pipeline lowers
/// one for every concrete class whose chain gives the method no body — an
/// abstract declaration included — typed at what its clauses pin the trait's
/// parameters to, and a static call on the class names the same body. A
/// generic class reaches this only through its bare vtable; its own copy there
/// leaves its parameters open, so its slot names the trait's shared
/// `"{Trait}_{method_name}"`.
///
/// A generic class's bare slots are what an instance runs when its arguments
/// could not be spelled where it was built, as [`VtableInstance::of`] lists.
/// The shared body they name reads a type parameter as an unmanaged word, so a
/// default
/// that overwrites a managed `T` local double-frees it: a construction inside
/// a body shared by every instantiation of its enclosing declaration is not
/// specialized per instantiation, and neither is its instance's vtable.
// TODO: an instance constructed inside a shared generic body — a method of
// `class Wrapper<T>` doing `let o Op<T> = Impl<T>()`, a closure building
// `Impl<T>`, a generic function whose parameter appears only in its return
// type — reaches these bare slots at every instantiation, and a trait default
// reassigning a `var x T` there double-frees a managed argument. Settling it
// needs the enclosing body compiled per instantiation, so the construction
// inside it spells its arguments.
pub fn resolve_vtable_method(
    class_name: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<String> {
    let owner = match resolve_method_source(type_defs, class_name, method_name)? {
        MethodSource::Declared { class, .. } => class,
        MethodSource::Default(default) if is_generic_class(type_defs, class_name) => {
            default.trait_name
        }
        MethodSource::Default(_) => class_name,
        MethodSource::AbstractOnly { .. } => return None,
    };
    Some(Symbol::method(owner, &[], method_name, &[]).link_name())
}

/// Whether `class_name` registers a class declaring type parameters.
fn is_generic_class(type_defs: &HashMap<String, TypeDefinition>, class_name: &str) -> bool {
    matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(class)) if class.generics.is_some()
    )
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
        .map(|(owner, _)| Symbol::method(&owner, &[], method_name, &[]).link_name())
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
        .unwrap_or_else(|| Symbol::method(type_name, &[], method_name, &[]).link_name())
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

/// Every function symbol codegen names without a MIR call carrying it, for
/// every class: the drop hook and the `clone` and element methods its runtime
/// thunks call. A vtable slot is named only where a reached body constructs an
/// instance and a reached virtual call reads it, so
/// [`super::vtable_demand::VtableDemand`] reads those off the lowered bodies.
///
/// A thunk's own predicate can still skip a class, so this may name a body no
/// thunk ends up calling; that costs one body compiled needlessly, never one
/// missing at link time.
pub fn synthesized_references(type_defs: &HashMap<String, TypeDefinition>) -> HashSet<String> {
    let mut symbols = HashSet::new();
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
