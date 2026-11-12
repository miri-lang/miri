// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The calling convention of each vtable slot: the one answer to "which
//! positions does a call through this slot hand over".
//!
//! A call through a trait or an overridable method cannot see which body it
//! reaches, so every call reading one slot, and every body filling it, agree
//! on one convention: the slot owns each position, in operand order with the
//! receiver first, that some body filling it owns
//! ([`super::kept_parameters`]). A slot therefore always hands at least as
//! much as any body keeps. A body keeping less is reached through an adapter
//! that takes what the slot hands, calls the body with the body's own
//! convention, and releases at its end what the body did not keep; the
//! adapter never retains.
//!
//! A trait's default and a method of a generic class are compiled once per
//! class and per instantiation, each copy owning what it keeps at its own
//! types ([`super::kept_parameters::owned_at`]). The slot owns a position
//! such a body keeps wherever some copy may own it: at a type its declared
//! one hands on, or at one a copy reads through a type parameter or `Self`,
//! as for a trait or an extended class held at the copy's own arguments.
//! Each side reads the convention at its own types: a position whose operand
//! is no counted value — a generic trait's parameter at a scalar type — is
//! lent on both sides of the call. The consumption check at a virtual call
//! ([`super::use_after_move`]), MIR lowering of the call, and the adapters
//! the pipeline builds for the slot all read this table.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::context::{class_is_extended, resolve_method_source, MethodSource, TypeDefinition};
use super::instantiation_requirements::spells_a_type;
use super::kept_parameters::{owned_at, KeptParameters};
use super::utils::{hands_on_a_reference, ResourceTypes};
use super::vtable_selectors::{
    class_fills, dispatch_candidates, dispatched_slot_root, overridden_method_names,
};
use crate::ast::types::TypeKind;

/// For each vtable slot owning a position, which positions it owns, by the
/// slot's root and then its method; a slot absent from the table lends every
/// position. Beside it, decided once for the whole program: the root of the
/// slot each call through a vtable reads, and the method names some class
/// overrides.
#[derive(Debug, Default)]
pub struct SlotConventions {
    by_root: BTreeMap<String, BTreeMap<String, Vec<bool>>>,
    /// The root of the slot a call to a method on a receiver reads, by the
    /// receiver's type and the method; absent for a static call.
    dispatch: HashMap<String, HashMap<String, String>>,
    /// The name of every method some class overrides.
    overridden: BTreeSet<String>,
}

impl SlotConventions {
    /// Which positions the slot of `selector` numbered under `root` owns, in
    /// operand order, the receiver first; empty when it owns none.
    pub fn of(&self, root: &str, selector: &str) -> &[bool] {
        (self.by_root.get(root))
            .and_then(|selectors| selectors.get(selector))
            .map_or(&[], Vec::as_slice)
    }

    /// The root of the slot a call to `selector` on a `receiver` reads, or
    /// `None` when the call is static
    /// ([`super::vtable_selectors::dispatched_slot_root`]).
    pub fn dispatched_root(&self, receiver: &str, selector: &str) -> Option<&str> {
        (self.dispatch.get(receiver))
            .and_then(|selectors| selectors.get(selector))
            .map(String::as_str)
    }

    /// Whether some class overrides a method named `method`.
    pub fn is_overridden(&self, method: &str) -> bool {
        self.overridden.contains(method)
    }

    /// Marks every position `owned` marks as owned by the slot of
    /// `selector` under `root`.
    fn own(&mut self, root: &str, selector: &str, owned: &[bool]) {
        let convention = (self.by_root.entry(root.to_string()).or_default())
            .entry(selector.to_string())
            .or_default();
        if convention.len() < owned.len() {
            convention.resize(owned.len(), false);
        }
        for (slot, &is_owned) in convention.iter_mut().zip(owned) {
            *slot |= is_owned;
        }
    }
}

/// The convention of every slot the classes of `type_defs` fill, with the
/// slot every call through a vtable reads: for each class and each slot it
/// fills ([`class_fills`]), the positions some copy of the body filling it
/// may own ([`owned_by_filling_body`]) join the slot's.
pub fn decide_slot_conventions(
    type_defs: &HashMap<String, TypeDefinition>,
    kept: &KeptParameters,
    resources: &ResourceTypes,
) -> SlotConventions {
    let mut conventions = SlotConventions {
        overridden: overridden_method_names(type_defs)
            .map(str::to_string)
            .collect(),
        ..SlotConventions::default()
    };
    for (name, definition) in type_defs {
        record_dispatch(name, type_defs, &mut conventions.dispatch);
        if !matches!(definition, TypeDefinition::Class(_)) {
            continue;
        }
        let is_overridden = |method: &str| conventions.overridden.contains(method);
        for (selector, roots) in class_fills(name, type_defs, &is_overridden) {
            let owned = owned_by_filling_body((name, selector), type_defs, kept, resources);
            if !owned.contains(&true) {
                continue;
            }
            for root in roots {
                conventions.own(root, selector, &owned);
            }
        }
    }
    conventions
}

/// Records the root of the slot each call on a `receiver` reads, for every
/// method it might read through a vtable.
fn record_dispatch(
    receiver: &str,
    type_defs: &HashMap<String, TypeDefinition>,
    dispatch: &mut HashMap<String, HashMap<String, String>>,
) {
    let roots: HashMap<String, String> = (dispatch_candidates(receiver, type_defs).into_iter())
        .filter_map(|selector| {
            let root = dispatched_slot_root(receiver, selector, type_defs)?;
            Some((selector.to_string(), root.to_string()))
        })
        .collect();
    if !roots.is_empty() {
        dispatch.insert(receiver.to_string(), roots);
    }
}

/// The positions some copy of the body `class_name` runs for `selector` may
/// own ([`owned_at`]): the method the nearest class of its chain declares,
/// compiled for that class, or the trait default it inherits, compiled for
/// `class_name`; none for an abstract declaration. A position is read at its
/// declared type, and one some copy may read at a type handing its reference
/// on ([`may_hand_on_in_some_copy`]) is owned wherever the body keeps it.
fn owned_by_filling_body(
    (class_name, selector): (&str, &str),
    type_defs: &HashMap<String, TypeDefinition>,
    kept: &KeptParameters,
    resources: &ResourceTypes,
) -> Vec<bool> {
    let (declaring, compiled_for, info) =
        match resolve_method_source(type_defs, class_name, selector) {
            Some(MethodSource::Declared { class, info }) => (class, class, info),
            Some(MethodSource::Default(default)) => (default.trait_name, class_name, default.info),
            Some(MethodSource::AbstractOnly { .. }) | None => return Vec::new(),
        };
    let Some(declaration) = kept.method_declaration(declaring, selector) else {
        return Vec::new();
    };
    let receiver = (!info.is_static).then(|| TypeKind::Custom(compiled_for.to_string(), None));
    let params = info.params.iter().map(|(_, ty)| &ty.kind);
    let may_hand_on = |kind: &TypeKind| {
        hands_on_a_reference(kind, type_defs, resources)
            || may_hand_on_in_some_copy(kind, type_defs)
    };
    owned_at(
        kept.kept_by_method(declaration),
        receiver.iter().chain(params),
        may_hand_on,
    )
}

/// Whether some copy of a body may read a value declared at `kind` at a type
/// handing its reference on ([`hands_on_a_reference`]) where `kind` itself
/// does not: a type parameter or `Self`, which each copy reads at its own
/// arguments, or a trait or a class another class extends, held at arguments
/// a copy reads at its own, which hands on what a resource instantiation
/// below it holds. Any other named type hands on whatever its arguments are
/// exactly when its hook runs, which its name decides.
fn may_hand_on_in_some_copy(kind: &TypeKind, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    if is_type_parameter(kind, type_defs) {
        return true;
    }
    let TypeKind::Custom(name, args) = kind else {
        return false;
    };
    let Some(definition) = type_defs.get(name) else {
        return false;
    };
    let is_held_at_a_supertype =
        matches!(definition, TypeDefinition::Trait(_)) || class_is_extended(name, type_defs);
    let has_open_arguments = match args {
        None => definition
            .generics()
            .is_some_and(|generics| !generics.is_empty()),
        Some(_) => spells_a_type(kind, &|kind| is_type_parameter(kind, type_defs)),
    };
    is_held_at_a_supertype && has_open_arguments
}

/// Whether `kind` names a type parameter, `Self` among them, rather than a
/// type the program declares.
fn is_type_parameter(kind: &TypeKind, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    if let TypeKind::Custom(name, None) = kind {
        return (type_defs.get(name))
            .is_none_or(|definition| matches!(definition, TypeDefinition::Generic(_)));
    }
    matches!(kind, TypeKind::Generic(..))
}
