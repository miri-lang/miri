// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The class instantiations a generic body builds at the types its callers
//! bind its parameters to.
//!
//! A generic function, or a method of a generic class, is checked once
//! against its own parameters, so a class it builds from them, `Box<T>(x)`
//! inside `fn f<T>(x T)`, names no type the program can be judged at. Each
//! such construction is recorded against the body that writes it, with the
//! parameters it spells ([`BuiltInBodies`]). Once every body is checked, the
//! bodies the program reaches are followed: from each site pinning a body
//! at concrete types, through each site a reached body writes, its pins
//! restated at the types that body is reached at. Every class a reached
//! body builds is recorded at those types.
//!
//! So `f(Handle(1))` records `Box<Handle>` before ownership is decided: a
//! trait or a base class above it ([`super::resource_types`]) and each copy
//! of a generic class's method ([`super::open_consumption`]) are decided
//! over every instantiation the program builds, whichever body builds it
//! ([`TypeChecker::ownership_instantiations`]).
//!
//! A site a trait slot reaches ([`SlotRead`]) is followed only where
//! lowering compiles the slot's body: where the call through the trait still
//! agrees with the converted trait's arguments once the body writing the
//! conversion is reached at concrete types, and where the body's obligations
//! hold at the instance, as lowering withholds a body only a slot names
//! otherwise. So a conversion `Impl<T>` to `Maker<T>` inside a generic
//! function reached at `Handle`, beside calls through `Maker<int>` alone,
//! records nothing `Impl<Handle>.make` would build.
// TODO: two functions still answer which instantiations a program builds,
// this record and lowering's own worklist, which finds every instance it
// compiles from the bodies it lowers and never reads this one. They still
// disagree where a call through a trait is written inside a generic body:
// its receiver is recorded with the body's parameters open, so it agrees
// with every conversion here, while lowering reads it at the types the body
// is compiled at. And lowering reads a call whose receiver is not a bare
// local as reaching every instance, so it compiles slot bodies this record
// does not follow. Lowering should read one answer decided here.
//!
//! A body calling itself at a type built around its own parameter reaches a
//! larger type on every step, and one doing so at two types reaches twice as
//! many each step. A type built from more than [`MAX_FOLLOWED_TYPE_SIZE`]
//! types is not followed, and no more than [`MAX_REACHED_BODIES`] bodies are.
//! Past either bound an instantiation goes unrecorded, and ownership reads
//! the trait or the copy as the bodies followed show it, as it did before
//! any were: a value is lent where it could have been moved, never the
//! reverse.

use super::compiled_instances::CompiledInstance;
use super::context::Context;
use super::instantiation_requirements::{
    parameters_spelled, spells_a_type, GenericBodyId, Pin, PinningSite, SlotRead,
};
use super::used_methods::reads_converted_slot;
use super::utils::add_instantiation;
use super::TypeChecker;
use crate::ast::types::{Type, TypeKind};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

/// How many types a type a reached body is bound to may be built from, all
/// told, before the bodies it reaches are no longer followed.
pub(crate) const MAX_FOLLOWED_TYPE_SIZE: usize = 64;

/// How many reached bodies are followed, each at one set of types.
pub(crate) const MAX_REACHED_BODIES: usize = 4096;

/// The classes each generic body builds from its own parameters, by body.
pub(crate) type BuiltInBodies = HashMap<GenericBodyId, Vec<BuiltClass>>;

/// A generic class a body builds at arguments spelling the body's own
/// parameters.
#[derive(Debug, Clone)]
pub(crate) struct BuiltClass {
    class: String,
    arguments: Vec<Type>,
    /// The body's parameters `arguments` spell.
    parameters: Vec<String>,
}

/// Whether lowering compiles a slot's body at an instance, by the class, the
/// method and the instance's type.
type SlotVerdicts = HashMap<(String, String, TypeKind), bool>;

/// A generic body the program reaches, with the type each of its parameters
/// is bound to there.
struct ReachedBody {
    body: GenericBodyId,
    bound: HashMap<String, Type>,
}

impl ReachedBody {
    /// What tells this reached body from another: the body and the kinds of
    /// its bound types, sorted by parameter name.
    fn identity(&self) -> (GenericBodyId, Vec<(String, TypeKind)>) {
        let mut bound: Vec<(String, TypeKind)> = (self.bound.iter())
            .map(|(parameter, ty)| (parameter.clone(), ty.kind.clone()))
            .collect();
        bound.sort_by(|(lhs, _), (rhs, _)| lhs.cmp(rhs));
        (self.body.clone(), bound)
    }
}

impl TypeChecker {
    /// Record that the body being checked builds the generic class `class`
    /// at `arguments`, when they spell that body's own parameters.
    pub(crate) fn record_class_built_in_body(
        &mut self,
        class: &str,
        arguments: &[Type],
        context: &Context,
    ) {
        let parameters = parameters_spelled(arguments.iter(), context);
        if parameters.is_empty() {
            return;
        }
        let Some(body) = self.current_body(context) else {
            return;
        };
        let built = self.classes_built_in_bodies.entry(body).or_default();
        let is_recorded = built.iter().any(|known| {
            known.class == class
                && (known.arguments.iter().map(|ty| &ty.kind))
                    .eq(arguments.iter().map(|ty| &ty.kind))
        });
        if !is_recorded {
            built.push(BuiltClass {
                class: class.to_string(),
                arguments: arguments.to_vec(),
                parameters,
            });
        }
    }

    /// Record every class instantiation a body the program reaches through
    /// `sites` builds, at the types that body is reached at, in
    /// [`TypeChecker::reached_class_instantiations`]. `context` is the
    /// program's global scope, which a slot's obligations are answered in.
    pub(super) fn record_reached_instantiations(
        &mut self,
        sites: &[PinningSite],
        context: &Context,
    ) {
        let built = std::mem::take(&mut self.classes_built_in_bodies);
        let mut written_in: HashMap<&GenericBodyId, Vec<&PinningSite>> = HashMap::new();
        for site in sites {
            if let Some(caller) = &site.caller {
                written_in.entry(caller).or_default().push(site);
            }
        }
        let mut verdicts = SlotVerdicts::new();
        let no_bound = HashMap::new();
        let mut pending: Vec<ReachedBody> = (sites.iter())
            .filter(|site| site.caller_parameters.is_empty())
            .filter_map(|site| self.reached_through(site, &no_bound, context, &mut verdicts))
            .collect();
        let mut seen = HashSet::new();
        while let Some(reached) = pending.pop() {
            if seen.len() == MAX_REACHED_BODIES {
                break;
            }
            if !seen.insert(reached.identity()) {
                continue;
            }
            for class in built.get(&reached.body).into_iter().flatten() {
                self.record_built_at(class, &reached.bound);
            }
            for site in written_in.get(&reached.body).into_iter().flatten() {
                let next = self.reached_through(site, &reached.bound, context, &mut verdicts);
                pending.extend(next);
            }
        }
        self.classes_built_in_bodies = built;
        self.resource_types.take();
    }

    /// The body `site` pins, reached at its pins restated at `bound`, the
    /// types the body writing the site is reached at. `None` when a pin
    /// spells a parameter `bound` leaves open, or is built past
    /// [`MAX_FOLLOWED_TYPE_SIZE`], and when the site is a trait slot lowering
    /// does not compile at `bound` ([`Self::compiles_slot_at`]).
    fn reached_through(
        &mut self,
        site: &PinningSite,
        bound: &HashMap<String, Type>,
        context: &Context,
        verdicts: &mut SlotVerdicts,
    ) -> Option<ReachedBody> {
        if !(site.caller_parameters.iter()).all(|parameter| bound.contains_key(parameter)) {
            return None;
        }
        let mut restated = HashMap::with_capacity(site.pins.len());
        for (parameter, pin) in &site.pins {
            let ty = match pin {
                Pin::CallerParameter(name) => bound.get(name)?.clone(),
                Pin::Concrete(ty) => self.substitute_type(ty, bound),
            };
            if !is_within_followed_size(&ty) {
                return None;
            }
            restated.insert(parameter.clone(), ty);
        }
        if let Some(read) = &site.through_slot {
            if !self.compiles_slot_at(read, bound, context, verdicts) {
                return None;
            }
        }
        Some(ReachedBody {
            body: site.callee.clone(),
            bound: restated,
        })
    }

    /// Whether lowering compiles the body of the slot `read` names, in a
    /// body reached at `bound`: the call through the trait still agrees with
    /// the converted trait's arguments restated at `bound`, and the method's
    /// obligations hold at the instance restated there. Lowering fills a slot
    /// only for a call whose receiver the instance stands behind, and
    /// withholds a body only a slot names where its obligations fail.
    fn compiles_slot_at(
        &mut self,
        read: &SlotRead,
        bound: &HashMap<String, Type>,
        context: &Context,
        verdicts: &mut SlotVerdicts,
    ) -> bool {
        let converted: Vec<(String, Vec<Option<Type>>)> = (read.converted.iter())
            .map(|(name, arguments)| (name.clone(), self.restated_arguments(arguments, bound)))
            .collect();
        let (called_trait, called) = &read.called;
        if !reads_converted_slot(&converted, (called_trait, called)) {
            return false;
        }
        let receiver = self.substitute_type(&read.instance, bound);
        let key = (
            read.class_name.clone(),
            read.method.clone(),
            receiver.kind.clone(),
        );
        if let Some(&accepted) = verdicts.get(&key) {
            return accepted;
        }
        let instance = CompiledInstance {
            class_name: read.class_name.clone(),
            method: read.method.clone(),
            substitution: (read.substitution.iter())
                .map(|(parameter, ty)| (parameter.clone(), self.substitute_type(ty, bound)))
                .collect(),
            span: receiver.span,
            receiver,
        };
        let accepted = self.instance_refusals_in(&instance, context).is_empty();
        verdicts.insert(key, accepted);
        accepted
    }

    /// `arguments`, each restated at `bound`.
    fn restated_arguments(
        &self,
        arguments: &[Option<Type>],
        bound: &HashMap<String, Type>,
    ) -> Vec<Option<Type>> {
        (arguments.iter())
            .map(|argument| (argument.as_ref()).map(|ty| self.substitute_type(ty, bound)))
            .collect()
    }

    /// Record `class`, built in a body reached at `bound`, at the types
    /// `bound` gives its arguments, when it binds every parameter they spell.
    fn record_built_at(&mut self, class: &BuiltClass, bound: &HashMap<String, Type>) {
        if !(class.parameters.iter()).all(|parameter| bound.contains_key(parameter)) {
            return;
        }
        let arguments: Vec<Type> = (class.arguments.iter())
            .map(|ty| self.substitute_type(ty, bound))
            .collect();
        if !arguments.iter().all(is_within_followed_size) {
            return;
        }
        if let Some(arguments) = self.instantiation_arguments(&arguments) {
            add_instantiation(
                &mut self.reached_class_instantiations,
                &class.class,
                arguments,
            );
        }
    }
}

/// Whether `ty` is built from at most [`MAX_FOLLOWED_TYPE_SIZE`] types.
fn is_within_followed_size(ty: &Type) -> bool {
    let counted = Cell::new(0usize);
    !spells_a_type(&ty.kind, &|_| {
        counted.set(counted.get() + 1);
        counted.get() > MAX_FOLLOWED_TYPE_SIZE
    })
}
