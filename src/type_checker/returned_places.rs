// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The methods that hand back a place of their receiver.
//!
//! A method hands back a place of its receiver when a value it returns is
//! borrowed from a place rooted at `self` ([`super::place_reads`], the one
//! answer to "is this value borrowed from a place"): an element or an
//! optional field of `self`, what a call handing back a place of `self`
//! returns, or a local bound to either. The receiver still holds the value
//! after the call, so the call's result is borrowed from the receiver as a
//! read of the place itself would be
//! ([`super::place_reads::BorrowedPlace::Returned`]). The table is worked
//! out to a fixpoint together with lending ([`super::lent_functions`]),
//! whose walk of each body reads it.
//!
//! A method of a type that declares it abstract has no body to read. A body
//! returning what such a method hands back on `self` hands back a place at
//! the classes whose own body for it does ([`Handed::AnyOf`]): the answer is
//! settled where the call's receiver names a class, and inside a body
//! compiled once per inheriting class it is that class's own
//! ([`Returned::PerCopy`]).

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::RECEIVER_PARAM_NAME;
use crate::ast::types::{Type, TypeKind};

use super::context::{resolve_method_source, trait_lineage, MethodSource, TypeDefinition};
use super::trait_self::self_trait_name;

/// Whether a call hands back a place of its receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Returned {
    /// The body the call reaches hands back a place of its receiver.
    Always,
    /// The call is made on `self` in a body compiled once per inheriting
    /// class, and hands back a place at the classes whose body for one of
    /// these abstract methods does.
    PerCopy(BTreeSet<String>),
}

/// When one method body hands back a place of its receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Handed {
    /// Whatever class the body runs on.
    Always,
    /// At the classes whose own body for one of these abstract methods,
    /// called on `self`, hands back a place.
    AnyOf(BTreeSet<String>),
}

impl Handed {
    /// The body handing back `other` too, on another return path.
    pub(crate) fn join(self, other: Handed) -> Handed {
        match (self, other) {
            (Handed::Always, _) | (_, Handed::Always) => Handed::Always,
            (Handed::AnyOf(mut methods), Handed::AnyOf(more)) => {
                methods.extend(more);
                Handed::AnyOf(methods)
            }
        }
    }
}

/// Every method whose body hands back a place of its receiver, by the
/// identity of the type declaring it and the method's name.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReturnedPlaces {
    methods: HashMap<(String, String), Handed>,
}

/// Where a call of a method on a receiver of some type lands.
enum Landing<'d> {
    /// The body declared by the type named here.
    Body(&'d str),
    /// An abstract declaration, whose body is each class's own.
    Abstract,
}

impl ReturnedPlaces {
    /// Records that the method `method` of the type `owner` hands back a
    /// place of its receiver when `handed` says.
    pub(crate) fn insert(&mut self, (owner, method): (&str, &str), handed: Handed) {
        let key = (owner.to_string(), method.to_string());
        let joined = match self.methods.remove(&key) {
            Some(before) => before.join(handed),
            None => handed,
        };
        self.methods.insert(key, joined);
    }

    /// Whether a call of `method` on a receiver read at the type named
    /// `type_name` hands back a place of the receiver; `on_self` when the
    /// receiver is the `self` of the body calling it.
    pub(crate) fn at(
        &self,
        type_definitions: &HashMap<String, TypeDefinition>,
        (type_name, method): (&str, &str),
        on_self: bool,
    ) -> Option<Returned> {
        self.at_unless_seen(
            type_definitions,
            (type_name, method),
            on_self,
            &mut HashSet::new(),
        )
    }

    /// Whether a call of any of `methods` on a receiver of a class named
    /// `type_name` hands back a place of the receiver.
    pub(crate) fn any_at(
        &self,
        type_definitions: &HashMap<String, TypeDefinition>,
        type_name: &str,
        methods: &BTreeSet<String>,
    ) -> Option<Returned> {
        let mut seen = HashSet::new();
        self.any_unless_seen(type_definitions, type_name, methods, &mut seen)
    }

    /// [`Self::at`], answering no for a method `seen` already holds.
    fn at_unless_seen(
        &self,
        type_definitions: &HashMap<String, TypeDefinition>,
        (type_name, method): (&str, &str),
        on_self: bool,
        seen: &mut HashSet<String>,
    ) -> Option<Returned> {
        let per_copy = || on_self.then(|| Returned::PerCopy(BTreeSet::from([method.to_string()])));
        let owner = match landing(type_definitions, type_name, method)? {
            Landing::Body(owner) => owner,
            Landing::Abstract => return per_copy(),
        };
        let handed = self.methods.get(&(owner.to_string(), method.to_string()))?;
        match handed {
            Handed::Always => Some(Returned::Always),
            Handed::AnyOf(methods) if on_self => Some(Returned::PerCopy(methods.clone())),
            Handed::AnyOf(methods) => {
                if !seen.insert(format!("{type_name}.{method}")) {
                    return None;
                }
                self.any_unless_seen(type_definitions, type_name, methods, seen)
            }
        }
    }

    /// [`Self::any_at`], answering no for a method `seen` already holds.
    fn any_unless_seen(
        &self,
        type_definitions: &HashMap<String, TypeDefinition>,
        type_name: &str,
        methods: &BTreeSet<String>,
        seen: &mut HashSet<String>,
    ) -> Option<Returned> {
        let hands_back = methods.iter().any(|method| {
            let returned = self.at_unless_seen(type_definitions, (type_name, method), false, seen);
            returned.is_some()
        });
        hands_back.then_some(Returned::Always)
    }

    /// Whether the call `value` hands back a place of its receiver, read at
    /// a named type ([`Self::at`]).
    pub(crate) fn of_call(
        &self,
        value: &Expression,
        (types, type_definitions): (&HashMap<usize, Type>, &HashMap<String, TypeDefinition>),
    ) -> Option<Returned> {
        let ExpressionKind::Call(callee, _) = &value.node else {
            return None;
        };
        let ExpressionKind::Member(receiver, method) = &callee.node else {
            return None;
        };
        let ExpressionKind::Identifier(method, _) = &method.node else {
            return None;
        };
        let type_name = receiver_type_name(types.get(&receiver.id)?)?;
        self.at(type_definitions, (type_name, method), is_self(receiver))
    }
}

/// Where a call of `method` on a receiver read at the type named
/// `type_name` lands; `None` where the type declares no such method.
fn landing<'d>(
    type_definitions: &'d HashMap<String, TypeDefinition>,
    type_name: &'d str,
    method: &str,
) -> Option<Landing<'d>> {
    match type_definitions.get(type_name)? {
        TypeDefinition::Class(_) => {
            match resolve_method_source(type_definitions, type_name, method)? {
                MethodSource::Declared { class, .. } => Some(Landing::Body(class)),
                MethodSource::Default(default) => Some(Landing::Body(default.trait_name)),
                MethodSource::AbstractOnly { .. } => Some(Landing::Abstract),
            }
        }
        TypeDefinition::Trait(_) => {
            let (owner, info) = trait_lineage(type_definitions, type_name)
                .find_map(|(owner, def)| def.methods.get(method).map(|info| (owner, info)))?;
            Some(if info.is_abstract {
                Landing::Abstract
            } else {
                Landing::Body(owner)
            })
        }
        TypeDefinition::Struct(_) | TypeDefinition::Enum(_) => Some(Landing::Body(type_name)),
        TypeDefinition::Generic(_) | TypeDefinition::Alias(_) => None,
    }
}

/// The named type a receiver of the type `ty` is read at: the type itself,
/// or the trait a trait's `Self` implements.
pub(crate) fn receiver_type_name(ty: &Type) -> Option<&str> {
    if let TypeKind::Custom(type_name, _) = &ty.kind {
        return Some(type_name);
    }
    self_trait_name(ty)
}

/// Whether `expr` names the receiver of the body it is written in.
fn is_self(expr: &Expression) -> bool {
    matches!(&expr.node, ExpressionKind::Identifier(name, _) if name == RECEIVER_PARAM_NAME)
}
