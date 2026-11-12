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
//! lowering compiles the slot's body: where the call through the trait
//! agrees with the converted trait's arguments, each side restated at the
//! types its own body is reached at, and where the body's obligations hold
//! at the instance, as lowering withholds a body only a slot names
//! otherwise. A call spelling its body's parameters is read only at the
//! types the program reaches that body at, so one in a generic body nothing
//! reaches reads no slot. So a conversion `Impl<T>` to `Maker<T>` inside a
//! generic function reached at `Handle`, beside calls through `Maker<int>`
//! alone, records nothing `Impl<Handle>.make` would build.
// TODO: two functions still answer which instantiations a program builds,
// this record and lowering's own worklist, which finds every instance it
// compiles from the bodies it lowers and never reads this one. They still
// disagree where a call's receiver is not a bare local: lowering reads it
// as reaching every instance, so it compiles slot bodies this record does
// not follow, while this record reads it at the type of the place holding
// it. Lowering should read one answer decided here.
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

/// Each set of types the walk has reached a body at so far, by body.
type ReachedBindings = HashMap<GenericBodyId, Vec<HashMap<String, Type>>>;

/// A trait-slot site waiting on the call reading its slot, with the types
/// the body writing the conversion is reached at.
type WaitingSite<'s> = (&'s PinningSite, HashMap<String, Type>);

/// What the walk over reached bodies knows as it goes.
#[derive(Default)]
struct ReachWalk<'s> {
    /// Whether lowering compiles a slot's body at an instance.
    verdicts: SlotVerdicts,
    /// Each set of types each body is reached at so far.
    reached: ReachedBindings,
    /// The trait-slot sites whose call reads their slot at no types reached
    /// so far, by the body writing the call.
    waiting: HashMap<GenericBodyId, Vec<WaitingSite<'s>>>,
}

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
        let mut walk = ReachWalk::default();
        let no_bound = HashMap::new();
        let mut pending: Vec<ReachedBody> = (sites.iter())
            .filter(|site| site.caller_parameters.is_empty())
            .filter_map(|site| self.reach_site(site, &no_bound, context, &mut walk))
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
            pending.extend(self.resume_waiting_slots(&reached, context, &mut walk));
            for site in written_in.get(&reached.body).into_iter().flatten() {
                pending.extend(self.reach_site(site, &reached.bound, context, &mut walk));
            }
        }
        self.classes_built_in_bodies = built;
        self.resource_types.take();
    }

    /// The body `site`, written in a body reached at `bound`, reaches. A
    /// site a trait slot reaches waits, when the call reading the slot reads
    /// it at no types reached so far, until the body writing the call is
    /// reached at types it reads it at ([`Self::resume_waiting_slots`]).
    fn reach_site<'s>(
        &mut self,
        site: &'s PinningSite,
        bound: &HashMap<String, Type>,
        context: &Context,
        walk: &mut ReachWalk<'s>,
    ) -> Option<ReachedBody> {
        if let Some(read) = &site.through_slot {
            if !self.is_slot_read_at(read, bound, &walk.reached) {
                let writer = &read.writer;
                if let (Some(body), false) = (&writer.body, writer.parameters.is_empty()) {
                    let waiting = walk.waiting.entry(body.clone()).or_default();
                    waiting.push((site, bound.clone()));
                }
                return None;
            }
        }
        self.reached_through(site, bound, context, &mut walk.verdicts)
    }

    /// Record `reached` as a set of types its body is reached at, and the
    /// bodies the sites waiting on a call it writes reach now that the call
    /// reads their slot at those types.
    fn resume_waiting_slots<'s>(
        &mut self,
        reached: &ReachedBody,
        context: &Context,
        walk: &mut ReachWalk<'s>,
    ) -> Vec<ReachedBody> {
        let bindings = walk.reached.entry(reached.body.clone()).or_default();
        bindings.push(reached.bound.clone());
        let Some(waiting) = walk.waiting.remove(&reached.body) else {
            return Vec::new();
        };
        let (resumed, still): (Vec<_>, Vec<_>) = waiting.into_iter().partition(|(site, bound)| {
            (site.through_slot.as_ref()).is_some_and(|read| {
                let converted = self.restated_conversion(read, bound);
                self.is_call_read_at(read, &converted, &reached.bound)
            })
        });
        if !still.is_empty() {
            walk.waiting.insert(reached.body.clone(), still);
        }
        (resumed.iter())
            .filter_map(|(site, bound)| {
                self.reached_through(site, bound, context, &mut walk.verdicts)
            })
            .collect()
    }

    /// The body `site` pins, reached at its pins restated at `bound`, the
    /// types the body writing the site is reached at. `None` when a pin
    /// spells a parameter `bound` leaves open, or is built past
    /// [`MAX_FOLLOWED_TYPE_SIZE`], and when the site is a trait slot whose
    /// body lowering withholds at `bound` ([`Self::compiles_slot_at`]).
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

    /// Whether the call `read` names reads the slot of the conversion it
    /// names, written in a body reached at `bound`: at the call's receiver
    /// as written, when it spells no parameter of the body writing it, and
    /// otherwise at that receiver restated at a set of types its body is
    /// reached at so far ([`Self::is_call_read_at`]).
    fn is_slot_read_at(
        &self,
        read: &SlotRead,
        bound: &HashMap<String, Type>,
        reached: &ReachedBindings,
    ) -> bool {
        let converted = self.restated_conversion(read, bound);
        let (called_trait, called) = &read.called;
        let writer = &read.writer;
        if writer.parameters.is_empty() {
            return reads_converted_slot(&converted, (called_trait, called));
        }
        let bindings = writer.body.as_ref().and_then(|body| reached.get(body));
        (bindings.into_iter().flatten())
            .any(|binding| self.is_call_read_at(read, &converted, binding))
    }

    /// Whether the call `read` names, written in a body reached at `binding`,
    /// reads a slot of a vtable whose instance stands behind each of
    /// `converted`: its receiver's trait arguments, restated at `binding`,
    /// spell no parameter left open and agree with one of them.
    fn is_call_read_at(
        &self,
        read: &SlotRead,
        converted: &[(String, Vec<Option<Type>>)],
        binding: &HashMap<String, Type>,
    ) -> bool {
        let writer = &read.writer;
        if !(writer.parameters.iter()).all(|parameter| binding.contains_key(parameter)) {
            return false;
        }
        let (called_trait, called) = &read.called;
        let called = self.restated_arguments(called, binding);
        let is_open = (called.iter().flatten()).any(|ty| spells_a_type(&ty.kind, &is_open_kind));
        !is_open && reads_converted_slot(converted, (called_trait, &called))
    }

    /// The trait `read`'s conversion converts to and each trait above it,
    /// with their arguments restated at `bound`, the types the body writing
    /// the conversion is reached at.
    fn restated_conversion(
        &self,
        read: &SlotRead,
        bound: &HashMap<String, Type>,
    ) -> Vec<(String, Vec<Option<Type>>)> {
        (read.converted.iter())
            .map(|(name, arguments)| (name.clone(), self.restated_arguments(arguments, bound)))
            .collect()
    }

    /// Whether lowering compiles the body of the slot `read` names, in a
    /// body reached at `bound`: the method's obligations hold at the
    /// instance restated there, as lowering withholds a body only a slot
    /// names where they fail.
    fn compiles_slot_at(
        &mut self,
        read: &SlotRead,
        bound: &HashMap<String, Type>,
        context: &Context,
        verdicts: &mut SlotVerdicts,
    ) -> bool {
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

/// Whether `kind` is a parameter left open, or a type that failed to check.
fn is_open_kind(kind: &TypeKind) -> bool {
    matches!(kind, TypeKind::Generic(..) | TypeKind::Error)
}

/// Whether `ty` is built from at most [`MAX_FOLLOWED_TYPE_SIZE`] types.
fn is_within_followed_size(ty: &Type) -> bool {
    let counted = Cell::new(0usize);
    !spells_a_type(&ty.kind, &|_| {
        counted.set(counted.get() + 1);
        counted.get() > MAX_FOLLOWED_TYPE_SIZE
    })
}
