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
//! many each step. The walk applies the bounds lowering compiles within
//! ([`super::instantiation_limits`]) to every body it reaches and every class
//! a reached body builds, and refuses the program at the first one past a
//! bound that a body reached from itself leads to, as polymorphic recursion,
//! naming the earlier instances of its class that led to it. A type the
//! program writes past a bound, with no recursion leading to it, is left to
//! lowering, which refuses it in its own words. The walk follows the most
//! deeply nested body it has reached first, so a type growing on every call
//! is refused once one chain of calls passes the depth bound, before the
//! bodies beside that chain multiply. Nothing else stops it, so ownership is
//! decided over every instantiation lowering compiles from the bodies
//! followed.

mod limits;

use super::compiled_instances::CompiledInstance;
use super::context::{class_needs_vtable, Context};
use super::instantiation_limits::deepest_argument_depth;
use super::instantiation_requirements::{
    parameters_spelled, spells_a_type, GenericBodyId, Pin, PinningSite, SlotRead,
};
use super::used_methods::reads_converted_slot;
use super::utils::add_instantiation;
use super::TypeChecker;
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use limits::Refused;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::rc::Rc;

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
    /// The construction building it.
    span: Span,
}

/// Whether lowering compiles a slot's body at an instance, by the class, the
/// method and the instance's type.
type SlotVerdicts = HashMap<(String, String, TypeKind), bool>;

/// Each step the walk has reached a body at so far, by body.
type ReachedBindings = HashMap<GenericBodyId, Vec<Rc<Step>>>;

/// How a body was reached: the step whose body wrote the site reaching it,
/// `None` for a site outside every generic body.
type Lineage = Option<Rc<Step>>;

/// A trait-slot site waiting on the call reading its slot, with the step the
/// body writing the conversion was reached at.
type WaitingSite<'s> = (&'s PinningSite, Lineage);

/// A generic body the program reaches, with the type each of its parameters
/// is bound to there, and how it was reached.
#[derive(Debug)]
struct Step {
    body: GenericBodyId,
    bound: HashMap<String, Type>,
    /// Whether a call through a trait reached the body by reading its slot.
    through_slot: bool,
    outer: Lineage,
}

impl Step {
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

/// A reached body waiting to be followed. The most deeply nested is followed
/// first, the one reached first among equally nested ones.
struct Pending {
    step: Rc<Step>,
    depth: usize,
    order: usize,
}

impl Ord for Pending {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.depth.cmp(&other.depth)).then_with(|| other.order.cmp(&self.order))
    }
}

impl PartialOrd for Pending {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Pending {}

/// What the walk over reached bodies knows as it goes.
#[derive(Default)]
struct ReachWalk<'s> {
    /// Whether lowering compiles a slot's body at an instance.
    verdicts: SlotVerdicts,
    /// Each step each body is reached at so far.
    reached: ReachedBindings,
    /// The trait-slot sites whose call reads their slot at no types reached
    /// so far, by the body writing the call.
    waiting: HashMap<GenericBodyId, Vec<WaitingSite<'s>>>,
    /// The reached bodies not yet followed.
    pending: BinaryHeap<Pending>,
    /// How many bodies have been reached so far.
    admitted: usize,
    /// The instances at value arguments found so far, by class.
    value_instances: HashMap<String, HashSet<Vec<TypeKind>>>,
}

impl TypeChecker {
    /// Record that the body being checked builds the generic class `class`
    /// at `arguments`, at `span`, when they spell that body's own parameters.
    pub(crate) fn record_class_built_in_body(
        &mut self,
        class: &str,
        arguments: &[Type],
        span: Span,
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
                span,
            });
        }
    }

    /// Record every class instantiation a body the program reaches through
    /// `sites` builds, at the types that body is reached at, in
    /// [`TypeChecker::reached_class_instantiations`], refusing the program
    /// where one passes a bound of [`super::instantiation_limits`].
    /// `context` is the program's global scope, which a slot's obligations
    /// are answered in.
    pub(super) fn record_reached_instantiations(
        &mut self,
        sites: &[PinningSite],
        context: &Context,
    ) {
        let built = std::mem::take(&mut self.classes_built_in_bodies);
        let refusal = self.walk_reached_bodies(sites, &built, context).err();
        self.classes_built_in_bodies = built;
        self.resource_types.take();
        if let Some(refusal) = refusal {
            self.diagnostics.push_error(*refusal);
        }
    }

    /// Follow every body `sites` reach, deepest first, recording what each
    /// builds; the refusal of the first instance past a bound, if any.
    fn walk_reached_bodies(
        &mut self,
        sites: &[PinningSite],
        built: &BuiltInBodies,
        context: &Context,
    ) -> Result<(), Refused> {
        let mut written_in: HashMap<&GenericBodyId, Vec<&PinningSite>> = HashMap::new();
        for site in sites {
            if let Some(caller) = &site.caller {
                written_in.entry(caller).or_default().push(site);
            }
        }
        let mut walk = ReachWalk::default();
        for site in sites
            .iter()
            .filter(|site| site.caller_parameters.is_empty())
        {
            self.reach_site(site, &None, context, &mut walk)?;
        }
        let mut seen = HashSet::new();
        while let Some(Pending { step, .. }) = walk.pending.pop() {
            if !seen.insert(step.identity()) {
                continue;
            }
            for class in built.get(&step.body).into_iter().flatten() {
                self.record_built_at(class, &step, context, &mut walk)?;
            }
            self.resume_waiting_slots(&step, context, &mut walk)?;
            let writer = Some(Rc::clone(&step));
            for site in written_in.get(&step.body).into_iter().flatten() {
                self.reach_site(site, &writer, context, &mut walk)?;
            }
        }
        Ok(())
    }

    /// Follow the body `site`, written in the body `writer` reached, reaches.
    /// A site a trait slot reaches waits, when the call reading the slot
    /// reads it at no types reached so far, until the body writing the call
    /// is reached at types it reads it at ([`Self::resume_waiting_slots`]).
    fn reach_site<'s>(
        &mut self,
        site: &'s PinningSite,
        writer: &Lineage,
        context: &Context,
        walk: &mut ReachWalk<'s>,
    ) -> Result<(), Refused> {
        if let Some(read) = &site.through_slot {
            let no_bound = HashMap::new();
            let bound = writer.as_ref().map_or(&no_bound, |step| &step.bound);
            if !self.is_slot_read_at(read, bound, &walk.reached) {
                let call_writer = &read.writer;
                if let (Some(body), false) = (&call_writer.body, call_writer.parameters.is_empty())
                {
                    let waiting = walk.waiting.entry(body.clone()).or_default();
                    waiting.push((site, writer.clone()));
                }
                return Ok(());
            }
        }
        self.follow(site, writer, context, walk)
    }

    /// Record `step` as a step its body is reached at, and follow the bodies
    /// the sites waiting on a call it writes reach now that the call reads
    /// their slot at its types.
    fn resume_waiting_slots<'s>(
        &mut self,
        step: &Rc<Step>,
        context: &Context,
        walk: &mut ReachWalk<'s>,
    ) -> Result<(), Refused> {
        let steps = walk.reached.entry(step.body.clone()).or_default();
        steps.push(Rc::clone(step));
        let Some(waiting) = walk.waiting.remove(&step.body) else {
            return Ok(());
        };
        let no_bound = HashMap::new();
        let (resumed, still): (Vec<_>, Vec<_>) = waiting.into_iter().partition(|(site, writer)| {
            let bound = writer.as_ref().map_or(&no_bound, |writer| &writer.bound);
            (site.through_slot.as_ref()).is_some_and(|read| {
                let converted = self.restated_conversion(read, bound);
                self.is_call_read_at(read, &converted, &step.bound)
            })
        });
        if !still.is_empty() {
            walk.waiting.insert(step.body.clone(), still);
        }
        for (site, writer) in resumed {
            self.follow(site, &writer, context, walk)?;
        }
        Ok(())
    }

    /// Reach the body `site` pins at its pins restated at the types of the
    /// body `writer` reached, which writes the site. Nothing is reached when
    /// a pin spells a parameter those types leave open, or when the site is a
    /// trait slot whose body lowering withholds there
    /// ([`Self::compiles_slot_at`]).
    fn follow(
        &mut self,
        site: &PinningSite,
        writer: &Lineage,
        context: &Context,
        walk: &mut ReachWalk<'_>,
    ) -> Result<(), Refused> {
        let no_bound = HashMap::new();
        let bound = writer.as_ref().map_or(&no_bound, |step| &step.bound);
        if !(site.caller_parameters.iter()).all(|parameter| bound.contains_key(parameter)) {
            return Ok(());
        }
        let mut restated = HashMap::with_capacity(site.pins.len());
        for (parameter, pin) in &site.pins {
            let ty = match pin {
                Pin::CallerParameter(name) => match bound.get(name) {
                    Some(ty) => ty.clone(),
                    None => return Ok(()),
                },
                Pin::Concrete(ty) => self.substitute_type(ty, bound),
            };
            restated.insert(parameter.clone(), ty);
        }
        if let Some(read) = &site.through_slot {
            if !self.compiles_slot_at(read, bound, context, &mut walk.verdicts) {
                return Ok(());
            }
        }
        let step = Rc::new(Step {
            body: site.callee.clone(),
            bound: restated,
            through_slot: site.through_slot.is_some(),
            outer: writer.clone(),
        });
        self.refuse_body_past_limits(&step, site.span, context, walk)?;
        walk.admitted += 1;
        let depth = 1 + deepest_argument_depth(step.bound.values());
        let order = walk.admitted;
        walk.pending.push(Pending { step, depth, order });
        Ok(())
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
        let steps = writer.body.as_ref().and_then(|body| reached.get(body));
        (steps.into_iter().flatten())
            .any(|step| self.is_call_read_at(read, &converted, &step.bound))
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

    /// Record `class`, built in the body `step` reached, at the types `step`
    /// gives its arguments, when it binds every parameter they spell.
    /// Refuses it past a bound of [`super::instantiation_limits`].
    fn record_built_at(
        &mut self,
        class: &BuiltClass,
        step: &Rc<Step>,
        context: &Context,
        walk: &mut ReachWalk<'_>,
    ) -> Result<(), Refused> {
        let bound = &step.bound;
        if !(class.parameters.iter()).all(|parameter| bound.contains_key(parameter)) {
            return Ok(());
        }
        let arguments: Vec<Type> = (class.arguments.iter())
            .map(|ty| self.substitute_type(ty, bound))
            .collect();
        let builder = Some(Rc::clone(step));
        let growth = || {
            if class_needs_vtable(&class.class, self.type_definitions()) {
                self.growth_through_slots(&class.class, &builder, context)
            } else {
                self.growth_through_calls(&class.class, &arguments, &builder, context)
            }
        };
        let within = (class.span, step.as_ref());
        self.refuse_instance_past_limits(&class.class, &arguments, growth, within, walk)?;
        if let Some(arguments) = self.instantiation_arguments(&arguments) {
            add_instantiation(
                &mut self.reached_class_instantiations,
                &class.class,
                arguments,
            );
        }
        Ok(())
    }
}

/// Whether `kind` is a parameter left open, or a type that failed to check.
fn is_open_kind(kind: &TypeKind) -> bool {
    matches!(kind, TypeKind::Generic(..) | TypeKind::Error)
}
