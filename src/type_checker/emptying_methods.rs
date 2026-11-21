// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The methods that take a value out of their receiver and hand it back,
//! inferred from their bodies.
//!
//! A method empties its receiver when a value it returns is taken out of
//! `self`: what `replace` hands back of the place it empties, a binding read
//! out of an element that a runtime entry point declared to remove it
//! ([`crate::runtime_effects::Removal`]) has since taken out, or what an
//! emptying method hands back called on a place of `self`. The place is the
//! whole receiver, a field of it, or an element of a place of `self` at the
//! index one of the method's parameters carries. The walk inferring it is the one inferring
//! lending ([`super::lent_functions`]), worked out to the same fixpoint, so
//! the compiler learns which method empties from its body alone, never from
//! its name.
//!
//! A refusal of handing a value out of a place names the way to take it out
//! instead: the method of the optional that empties it, or the method of a
//! collection that takes an element out by its index. A type with no such
//! method offers no way to take a value out, so a refusal reading one of its
//! elements has no repair to name. An inferred method is named only where
//! the call the repair spells compiles: an instance method handing back an
//! optional, taking no parameter when it empties the whole receiver and only
//! the `int` index when it takes out an element.

use std::collections::{HashMap, HashSet};

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::RECEIVER_PARAM_NAME;
use crate::ast::types::TypeKind;

use super::context::MethodInfo;
use super::place_reads::{is_spelled_under, place_spelling};

/// What an emptying method takes out of its receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Emptied {
    /// The whole receiver, which it leaves empty.
    Receiver,
    /// The element at the index its parameter `index` carries.
    Element { index: String },
    /// The receiver's own field `name`.
    Field { name: String },
}

/// What each emptying method of the program and its modules takes out of
/// its receiver, by the identity of the type declaring it and its name.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct EmptyingMethods {
    methods: HashMap<String, HashMap<String, Emptied>>,
}

impl EmptyingMethods {
    /// Records that the method `method` of the type `owner` empties its
    /// receiver as `emptied` says.
    pub(crate) fn insert(&mut self, (owner, method): (&str, &str), emptied: Emptied) {
        (self.methods.entry(owner.to_string()).or_default()).insert(method.to_string(), emptied);
    }
}

/// What the method `method` of the type `owner`, declared as `info`, takes
/// out of its receiver as its body shows; `None` when it takes nothing out,
/// or when the call a repair would spell does not compile.
pub(crate) fn emptied_by<'t>(
    table: &'t EmptyingMethods,
    (owner, method): (&str, &str),
    info: &MethodInfo,
) -> Option<&'t Emptied> {
    let emptied = table.methods.get(owner)?.get(method)?;
    is_named_in_a_repair(info, emptied).then_some(emptied)
}

/// Whether the repair calling the method declared as `info`, emptying as
/// `emptied` says, compiles: an instance method handing back an optional,
/// called with no argument or with only its `int` index.
fn is_named_in_a_repair(info: &MethodInfo, emptied: &Emptied) -> bool {
    let is_instance_method = !info.is_static && !info.is_constructor;
    if !is_instance_method || !matches!(info.return_type.kind, TypeKind::Option(_)) {
        return false;
    }
    match emptied {
        Emptied::Receiver => info.params.is_empty(),
        Emptied::Element { index } => matches!(
            info.params.as_slice(),
            [(name, ty)] if name == index && ty.kind == TypeKind::Int && !info.is_param_out(0)
        ),
        Emptied::Field { .. } => false,
    }
}

/// What a method whose parameters are `params` takes out of its receiver by
/// emptying `place`: the whole receiver when `place` is `self`, the element
/// at a parameter's index when it is an element of a place of `self` there,
/// and a field of the receiver when it is `self.<field>`.
///
/// TODO: a field is recorded but never named in a repair
/// ([`is_named_in_a_repair`]): whether a refusal of keeping what a getter of
/// that field hands back should name the method taking it out, as it names a
/// collection's element-taking method, is still to be decided. A field of a
/// field of `self` (`self.inner.slot`) is not recorded.
pub(crate) fn emptied_at(place: &Expression, params: &HashSet<String>) -> Option<Emptied> {
    if let ExpressionKind::Index(base, index) = &place.node {
        let index = parameter_named(index, params).filter(|_| is_rooted_at_receiver(base))?;
        return Some(Emptied::Element {
            index: index.to_string(),
        });
    }
    if let ExpressionKind::Member(base, field) = &place.node {
        let ExpressionKind::Identifier(field, _) = &field.node else {
            return None;
        };
        return is_receiver(base).then(|| Emptied::Field {
            name: field.clone(),
        });
    }
    is_receiver(place).then_some(Emptied::Receiver)
}

/// Whether `expr` is `self` itself.
fn is_receiver(expr: &Expression) -> bool {
    matches!(&expr.node, ExpressionKind::Identifier(name, _) if name == RECEIVER_PARAM_NAME)
}

/// The parameter among `params` that `expr` names.
pub(crate) fn parameter_named<'e>(
    expr: &'e Expression,
    params: &HashSet<String>,
) -> Option<&'e str> {
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        return None;
    };
    params.contains(name).then_some(name.as_str())
}

/// Whether `expr` is `self` or a field or an element of it.
pub(crate) fn is_rooted_at_receiver(expr: &Expression) -> bool {
    place_spelling(expr).is_some_and(|spelled| is_spelled_under(&spelled, RECEIVER_PARAM_NAME))
}

#[cfg(test)]
mod tests;
