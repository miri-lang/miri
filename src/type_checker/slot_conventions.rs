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
//! The convention lists what the bodies own at their declared types. Each
//! side reads it at its own types: a position whose operand is no counted
//! value — a generic trait's parameter at a scalar type — is lent on both
//! sides of the call. The consumption check at a virtual call
//! ([`super::use_after_move`]), MIR lowering of the call, and the adapters
//! the pipeline builds for the slot all read this table.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::context::{resolve_method_source, MethodSource, TypeDefinition};
use super::kept_parameters::KeptParameters;
use super::vtable_selectors::{
    class_fills, dispatch_candidates, dispatched_slot_root, overridden_method_names,
};

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
/// fills ([`class_fills`]), the positions the body filling it keeps
/// ([`KeptParameters::kept_by_method`]) join the slot's. A trait's default
/// or a generic class's method keeps what it keeps while lent, and is
/// reached through an adapter owning what the slot hands.
pub fn decide_slot_conventions(
    type_defs: &HashMap<String, TypeDefinition>,
    kept: &KeptParameters,
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
            let owned = owned_by_filling_body(name, selector, type_defs, kept).to_vec();
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

/// The positions the body `class_name` runs for `selector` keeps: the
/// method the nearest class of its chain declares, or the trait default it
/// inherits; none for an abstract declaration.
fn owned_by_filling_body<'k>(
    class_name: &str,
    selector: &str,
    type_defs: &HashMap<String, TypeDefinition>,
    kept: &'k KeptParameters,
) -> &'k [bool] {
    match resolve_method_source(type_defs, class_name, selector) {
        Some(MethodSource::Declared { class, .. }) => kept.kept_by_method(class, selector),
        Some(MethodSource::Default(default)) => kept.kept_by_method(default.trait_name, selector),
        Some(MethodSource::AbstractOnly { .. }) | None => &[],
    }
}
