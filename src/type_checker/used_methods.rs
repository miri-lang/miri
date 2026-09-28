// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The pinning sites a program creates by using a method of a generic class
//! without writing a call to it.
//!
//! A method's requirements on its class's parameters are answered where the
//! method is used at an instantiation. A written call is one use, and is
//! recorded where member access resolves it. The others are recorded here:
//!
//! - every construction runs `init` and, once the instance is released, the
//!   drop hook;
//! - an operator on an instance runs the method its class answers it with;
//! - a body ordering or comparing a parameter pinned to an instance runs the
//!   instance's `compare` or `equals`, and a trait default runs the methods
//!   it calls on `self` at the class it is compiled for; both are derived
//!   while answering, from the obligations the pinned body is known to state;
//! - an instance converted to a trait, or to a class it extends, runs,
//!   through its vtable, each method the program calls through a receiver of
//!   that type.
//!
//! All of them become sites on the one requirement rail in
//! [`super::instantiation_requirements`], answered by the same replay of the
//! body's own checks; nothing here judges a type.

use super::context::{resolve_method_source, Context, TypeDefinition};
use super::instantiation_requirements::{
    spells_a_type, written_type, GenericBodyId, InstantiationRequirements, Obligation, Pin,
    PinningSite, SELF_PIN,
};
use super::TypeChecker;
use crate::ast::expression::Expression;
use crate::ast::implicit_methods::{
    operator_method_name, operator_receiver, CLONE_METHOD_NAME, CONSTRUCTION_METHOD_NAMES,
    EQUALS_METHOD_NAME,
};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::BinaryOp;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use std::collections::HashMap;

/// An instance of a class converted to a trait: the sites each method called
/// through that trait would pin, and the traits a receiver holding it can be.
#[derive(Debug)]
pub(crate) struct TraitConversion {
    /// The sites the class's copy of a method pins, one per type declaring
    /// it, with the method left for the call through the trait to name.
    sites: Vec<PinningSite>,
    /// The trait converted to and each trait above it, with the arguments
    /// the conversion pins each one's parameters to, in declaration order.
    reached: Vec<(String, Vec<Option<Type>>)>,
}

/// A class instance converted to a trait, as the conversion is recorded.
struct InstanceConversion<'c> {
    class_name: &'c str,
    /// The instance's own type, which a trait default reads `self` at.
    instance: &'c Type,
    /// What the instance pins its class's parameters to.
    substitution: &'c HashMap<String, Type>,
    trait_name: &'c str,
    /// What the conversion pins the trait's parameters to.
    trait_substitution: &'c HashMap<String, Type>,
}

/// A method called through a receiver whose type is a trait or a class,
/// which runs the vtable slot of whatever instance converted to that type the
/// receiver holds.
#[derive(Debug, PartialEq)]
pub(crate) struct TraitMethodCall {
    trait_name: String,
    method: String,
    arguments: Vec<Option<Type>>,
}

impl TypeChecker {
    /// Record the sites a construction of `class_name` at `type_args` stands
    /// for: its `init` runs now, and its drop hook once the instance is
    /// released, which every instance is.
    pub(crate) fn record_construction_sites(
        &mut self,
        class_name: &str,
        type_args: Option<&[Expression]>,
        span: Span,
        context: &Context,
    ) {
        let substitution = self.instance_substitution(class_name, type_args);
        let receiver = Type::new(
            TypeKind::Custom(class_name.to_string(), type_args.map(<[_]>::to_vec)),
            span,
        );
        for method in CONSTRUCTION_METHOD_NAMES {
            self.record_receiver_method_sites(
                class_name,
                method,
                &substitution,
                &receiver,
                span,
                context,
            );
        }
    }

    /// Record the site `op` applied to a left operand of type `receiver`
    /// stands for, when the operand — or, for an equality, the payload of an
    /// optional operand — is a class instance whose class answers the
    /// operator with a method.
    pub(crate) fn record_operator_method_sites(
        &mut self,
        receiver: &Type,
        op: &BinaryOp,
        span: Span,
        context: &Context,
    ) {
        let Some(method) = operator_method_name(op) else {
            return;
        };
        let receiver = operator_receiver(op, receiver);
        let Some((class_name, substitution)) = self.class_instance(receiver) else {
            return;
        };
        self.record_receiver_method_sites(
            &class_name,
            method,
            &substitution,
            receiver,
            span,
            context,
        );
    }

    /// Record the `equals` a set or map built from, or asked about, values of
    /// `element` runs: the runtime matches two class elements, or keys, only
    /// through the class's own `equals`, and an optional one through the
    /// `equals` of the value it holds.
    pub(crate) fn record_element_matching(
        &mut self,
        element: &Type,
        span: Span,
        context: &Context,
    ) {
        self.record_element_method_use(held_value(element), EQUALS_METHOD_NAME, span, context);
    }

    /// Record the `clone` a collection of type `container` runs on its
    /// elements — a map's values — when the collection is built: a copy of it,
    /// explicit or the one a write to a shared collection makes first, copies
    /// each `Cloneable` element through its own `clone`.
    pub(crate) fn record_elements_cloned(
        &mut self,
        container: &Type,
        span: Span,
        context: &Context,
    ) {
        let TypeKind::Custom(name, Some(arguments)) = &container.kind else {
            return;
        };
        let position = match BuiltinCollectionKind::from_name(name) {
            Some(
                BuiltinCollectionKind::List
                | BuiltinCollectionKind::Array
                | BuiltinCollectionKind::Set,
            ) => 0,
            Some(BuiltinCollectionKind::Map) => 1,
            None => return,
        };
        let Some(element) = arguments.get(position).and_then(written_type) else {
            return;
        };
        self.record_element_method_use(held_value(&element), CLONE_METHOD_NAME, span, context);
    }

    /// Record that `method` runs on values of `element`: a requirement of the
    /// body being checked when `element` is one of its parameters, and the
    /// sites of the class's `method` when it is a class instance.
    fn record_element_method_use(
        &mut self,
        element: &Type,
        method: &str,
        span: Span,
        context: &Context,
    ) {
        self.record_element_method_requirement(element, method, context);
        let Some((class_name, substitution)) = self.class_instance(element) else {
            return;
        };
        self.record_receiver_method_sites(
            &class_name,
            method,
            &substitution,
            element,
            span,
            context,
        );
    }

    /// Record the `equals` a membership test `op` on `container` runs, when
    /// the container is a set or a map: its elements, or keys, are matched
    /// against the value tested.
    pub(crate) fn record_membership_sites(
        &mut self,
        op: &BinaryOp,
        container: &Type,
        span: Span,
        context: &Context,
    ) {
        if matches!(op, BinaryOp::In) {
            self.record_keyed_lookup(container, span, context);
        }
    }

    /// Record the `equals` a lookup in `container` runs, when the container
    /// is a set or a map: the value looked up is matched against its
    /// elements, or keys.
    pub(crate) fn record_keyed_lookup(&mut self, container: &Type, span: Span, context: &Context) {
        let keyed = [BuiltinCollectionKind::Set, BuiltinCollectionKind::Map];
        let Some(element) = self.container_element_type(container, &keyed) else {
            return;
        };
        self.record_element_matching(&element, span, context);
    }

    /// Record that a value of `actual` is stored where `expected` is declared,
    /// when that converts a class instance to a trait or to a class it
    /// extends, whose methods then reach the instance's own by virtual
    /// dispatch.
    ///
    /// Called where the conversion is committed, never where compatibility is
    /// only asked about, so a conversion recorded here is one the program
    /// makes.
    pub(crate) fn record_trait_conversion(
        &mut self,
        expected: &Type,
        actual: &Type,
        span: Span,
        context: &Context,
    ) {
        if self.suppress_diagnostics {
            return;
        }
        let (expected, actual) = Self::optional_payloads(expected, actual);
        let TypeKind::Custom(trait_name, trait_args) = &expected.kind else {
            return;
        };
        self.record_trait_value_handed_on(trait_name, actual, context);
        let Some((class_name, substitution)) = self.class_instance(actual) else {
            return;
        };
        if !self.dispatches_virtually(trait_name, &class_name) {
            return;
        }
        let trait_substitution = self.instance_substitution(trait_name, trait_args.as_deref());
        let conversion = InstanceConversion {
            class_name: &class_name,
            instance: actual,
            substitution: &substitution,
            trait_name,
            trait_substitution: &trait_substitution,
        };
        self.push_trait_conversion(&conversion, span, context);
    }

    /// Record that a trait default stores a value typed as its own trait, or
    /// one above it, where the trait `declared` is declared.
    ///
    /// Inside a default such a value may be the instance the default runs
    /// for, however it was produced — `self`, an alias of it, what a method
    /// declared to return `Self` returned on it — and storing it as a trait
    /// hands that instance on, as [`Obligation::SelfConversion`] states. A
    /// value that is not the instance is pinned where it was converted, so
    /// counting it too can pin a method nothing reaches, never leave one
    /// reached unpinned.
    fn record_trait_value_handed_on(&mut self, declared: &str, actual: &Type, context: &Context) {
        let definitions = &self.type_table.global_type_definitions;
        if !matches!(definitions.get(declared), Some(TypeDefinition::Trait(_))) {
            return;
        }
        let TypeKind::Custom(actual_name, _) = &actual.kind else {
            return;
        };
        if matches!(definitions.get(actual_name), Some(TypeDefinition::Trait(_))) {
            self.record_self_conversion_of(actual_name, context);
        }
    }

    /// Whether a value of `class_name` stored where `declared` is declared
    /// is reached through `declared`'s methods by virtual dispatch: the
    /// declared type is a trait, or a class `class_name` extends.
    fn dispatches_virtually(&self, declared: &str, class_name: &str) -> bool {
        let definitions = &self.type_table.global_type_definitions;
        match definitions.get(declared) {
            Some(TypeDefinition::Trait(_)) => true,
            Some(TypeDefinition::Class(_)) => {
                declared != class_name
                    && super::context::class_ancestry(class_name, definitions)
                        .any(|(ancestor, _)| ancestor == declared)
            }
            Some(
                TypeDefinition::Struct(_)
                | TypeDefinition::Enum(_)
                | TypeDefinition::Generic(_)
                | TypeDefinition::Alias(_),
            )
            | None => false,
        }
    }

    /// Record the sites a call to `method` on an instance of `class_name` at
    /// `receiver` stands for: the class's copy of the method, one site per
    /// type declaring it, with a trait's default reading `self` at the
    /// instance.
    pub(crate) fn record_receiver_method_sites(
        &mut self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        receiver: &Type,
        span: Span,
        context: &Context,
    ) {
        if self.suppress_diagnostics {
            return;
        }
        let sites = self.receiver_sites(class_name, method, substitution, receiver, span, context);
        self.pinning_sites.extend(sites);
    }

    /// The sites [`record_receiver_method_sites`](Self::record_receiver_method_sites)
    /// records.
    fn receiver_sites(
        &self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        receiver: &Type,
        span: Span,
        context: &Context,
    ) -> Vec<PinningSite> {
        self.bodies_run_for(class_name, method, substitution, receiver)
            .into_iter()
            .filter_map(|(body, pins)| self.pinning_site(body, &pins, span, context))
            .collect()
    }

    /// Every body a call to `method` on an instance of `class_name` at
    /// `receiver` may run, with what it pins that body's parameters to: the
    /// class's own and each type above it, a trait's with [`SELF_PIN`] at
    /// the instance.
    pub(super) fn bodies_run_for(
        &self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        receiver: &Type,
    ) -> Vec<(GenericBodyId, HashMap<String, Type>)> {
        let own = (class_name.to_string(), substitution.clone());
        let above = self.declaring_types_above(class_name, substitution);
        std::iter::once(own)
            .chain(above)
            .map(|(declaring, mut pins)| {
                if matches!(
                    self.type_table.global_type_definitions.get(&declaring),
                    Some(TypeDefinition::Trait(_))
                ) {
                    pins.insert(SELF_PIN.to_string(), receiver.clone());
                }
                ((declaring, method.to_string()), pins)
            })
            .collect()
    }

    /// Record `conversion`, with the sites each method called through its
    /// trait would pin.
    fn push_trait_conversion(
        &mut self,
        conversion: &InstanceConversion<'_>,
        span: Span,
        context: &Context,
    ) {
        let sites = self.receiver_sites(
            conversion.class_name,
            "",
            conversion.substitution,
            conversion.instance,
            span,
            context,
        );
        if sites.is_empty() {
            return;
        }
        let reached = self.traits_reached(conversion.trait_name, conversion.trait_substitution);
        self.trait_conversions
            .push(TraitConversion { sites, reached });
    }

    /// Record a call to `method` through a receiver of the trait or class
    /// `trait_name` at `type_args`.
    pub(crate) fn record_trait_method_call(
        &mut self,
        trait_name: &str,
        method: &str,
        type_args: Option<&[Expression]>,
    ) {
        if self.suppress_diagnostics {
            return;
        }
        let call = TraitMethodCall {
            trait_name: trait_name.to_string(),
            method: method.to_string(),
            arguments: self.instance_arguments(trait_name, type_args),
        };
        if !self.trait_method_calls.contains(&call) {
            self.trait_method_calls.push(call);
        }
    }

    /// The sites every recorded conversion to a trait stands for: for each
    /// method called through a receiver of that trait, or of one above it,
    /// at arguments that agree with the conversion's, the class's copy of the
    /// method is used at the converted instance's arguments.
    ///
    /// A call through a receiver whose arguments are still parameters agrees
    /// with every conversion, which can pin a slot nothing reaches at run
    /// time; it never leaves one reached unpinned.
    pub(super) fn sites_reached_through_trait_slots(&self) -> Vec<PinningSite> {
        let mut sites = Vec::new();
        for conversion in &self.trait_conversions {
            for call in &self.trait_method_calls {
                let agrees = conversion.reached.iter().any(|(name, arguments)| {
                    *name == call.trait_name && Self::arguments_agree(arguments, &call.arguments)
                });
                if agrees {
                    sites.extend(
                        conversion
                            .sites
                            .iter()
                            .map(|site| site.for_method(&call.method)),
                    );
                }
            }
        }
        sites
    }

    /// Add the sites the settled `requirements` show a site in `sites` to
    /// reach without a call written to them: for each parameter a site pins
    /// to a class instance, each method its body is known to run on that
    /// parameter — an ordering's `compare`, an operator's method, a method a
    /// trait default calls on `self`. Returns whether anything was added.
    ///
    /// A derivation is made once per caller, class, method and instance, so
    /// the sites grow by at most one set per distinct instance a program
    /// reaches rather than once per path to it. One that would enter a class
    /// at an instance an earlier instance of that class on its own path
    /// embeds in is refused instead, as [`SiteDerivation`] states.
    pub(super) fn derive_used_method_sites(
        &mut self,
        requirements: &InstantiationRequirements,
        sites: &mut Vec<PinningSite>,
        derivation: &mut SiteDerivation,
    ) -> bool {
        let mut derived: Vec<(PinningSite, Origin)> = Vec::new();
        for (parent, site) in sites.iter().enumerate() {
            let Some(stated) = requirements.get(&site.callee) else {
                continue;
            };
            let mut parameters: Vec<&String> = site.pins.keys().collect();
            parameters.sort_unstable();
            for parameter in parameters {
                let Some(Pin::Concrete(pinned)) = site.pins.get(parameter) else {
                    continue;
                };
                // An optional pinned where the body compares or matches the
                // parameter runs the `equals` of the value it holds.
                let pinned = held_value(pinned);
                let Some((class_name, substitution)) = self.class_instance(pinned) else {
                    continue;
                };
                let mut methods: Vec<String> = stated
                    .iter()
                    .filter_map(|obligation| obligation.method_run_on(parameter))
                    .map(str::to_string)
                    .collect();
                if parameter == SELF_PIN {
                    methods.extend(self.methods_called_through_self(stated, site));
                }
                methods.sort_unstable();
                methods.dedup();
                for method in &methods {
                    let use_of = MethodUse {
                        class_name: &class_name,
                        method,
                        instance: pinned,
                        substitution: &substitution,
                    };
                    derived.extend(self.derive_method_use(parent, site, &use_of, derivation));
                }
            }
        }
        let grew = !derived.is_empty();
        for (site, origin) in derived {
            sites.push(site);
            derivation.origins.push(Some(origin));
        }
        grew
    }

    /// Every method a default pinned at `site` runs on its `self` by handing
    /// it on as a trait value: each one the program calls through a receiver
    /// of that trait, or of one above it, at arguments that agree with what
    /// the site pins the trait's own parameters to.
    fn methods_called_through_self(
        &self,
        stated: &[Obligation],
        site: &PinningSite,
    ) -> Vec<String> {
        let pinned: HashMap<String, Type> = site
            .pins
            .iter()
            .filter_map(|(parameter, pin)| match pin {
                Pin::Concrete(ty) => Some((parameter.clone(), ty.clone())),
                Pin::CallerParameter(_) => None,
            })
            .collect();
        let mut methods = Vec::new();
        for obligation in stated {
            let Obligation::SelfConversion { trait_name } = obligation else {
                continue;
            };
            let reached = self.traits_reached(trait_name, &pinned);
            let called = self.trait_method_calls.iter().filter(|call| {
                reached.iter().any(|(name, arguments)| {
                    *name == call.trait_name && Self::arguments_agree(arguments, &call.arguments)
                })
            });
            methods.extend(called.map(|call| call.method.clone()));
        }
        methods
    }

    /// The sites `use_of`, reached from the site at index `parent`, derives,
    /// unless the derivation was made already or the method has no body.
    fn derive_method_use(
        &mut self,
        parent: usize,
        site: &PinningSite,
        use_of: &MethodUse<'_>,
        derivation: &mut SiteDerivation,
    ) -> Vec<(PinningSite, Origin)> {
        let definitions = &self.type_table.global_type_definitions;
        if resolve_method_source(definitions, use_of.class_name, use_of.method).is_none() {
            return Vec::new();
        }
        if !derivation.claim(&site.caller, use_of) {
            return Vec::new();
        }
        if let Some(earlier) = derivation.embedded_ancestor(parent, use_of) {
            self.report_unbounded_use(use_of, &earlier, site.span);
            return Vec::new();
        }
        let origin = Origin {
            parent,
            class_name: use_of.class_name.to_string(),
            instance: use_of.instance.clone(),
        };
        self.bodies_run_for(
            use_of.class_name,
            use_of.method,
            use_of.substitution,
            use_of.instance,
        )
        .into_iter()
        .filter(|(_, pins)| !pins.is_empty())
        .map(|(body, pins)| (derived_site(site, body, &pins), origin.clone()))
        .collect()
    }

    /// Report that checking `use_of` would reach its class again at an ever
    /// larger instance, so no finite set of checks covers it.
    fn report_unbounded_use(&mut self, use_of: &MethodUse<'_>, earlier: &Type, span: Span) {
        self.report_error_with_help(
            DiagnosticCode::MirPolymorphicRecursion,
            format!(
                "'{}' of '{}' is used at {}, which grows from {} on the way to it; \
                 the instantiation recurses without bound",
                use_of.method, use_of.class_name, use_of.instance, earlier
            ),
            span,
            "bound the recursion by type, or keep one type (e.g. store the depth as a value \
             instead of in the type)"
                .to_string(),
        );
    }

    /// The class `ty` is an instance of, with what its parameters are pinned
    /// to, or `None` when `ty` names no class.
    fn class_instance(&self, ty: &Type) -> Option<(String, HashMap<String, Type>)> {
        let TypeKind::Custom(name, arguments) = &ty.kind else {
            return None;
        };
        let Some(TypeDefinition::Class(_)) = self.type_table.global_type_definitions.get(name)
        else {
            return None;
        };
        Some((
            name.clone(),
            self.instance_substitution(name, arguments.as_deref()),
        ))
    }

    /// What the arguments written after `type_name` pin its own parameters
    /// to, by name.
    fn instance_substitution(
        &self,
        type_name: &str,
        type_args: Option<&[Expression]>,
    ) -> HashMap<String, Type> {
        self.generics_of(type_name)
            .iter()
            .zip(type_args.unwrap_or_default())
            .map(|(generic, argument)| (generic.name.clone(), self.pinned_argument(argument)))
            .collect()
    }

    /// The arguments written after `type_name`, one per parameter it declares,
    /// in declaration order.
    fn instance_arguments(
        &self,
        type_name: &str,
        type_args: Option<&[Expression]>,
    ) -> Vec<Option<Type>> {
        let written = type_args.unwrap_or_default();
        (0..self.generics_of(type_name).len())
            .map(|position| written.get(position).map(|arg| self.pinned_argument(arg)))
            .collect()
    }

    /// One written type argument as a substitution holds it: a type as
    /// itself, a value argument wrapped so the value survives substitution.
    fn pinned_argument(&self, argument: &Expression) -> Type {
        self.extract_type_from_expression(argument)
            .unwrap_or_else(|_| super::generics::value_generic_marker_type(argument.clone()))
    }

    /// `trait_name` and every trait above it, each with what a receiver of
    /// `trait_name` whose parameters `substitution` pins pins its own to.
    fn traits_reached(
        &self,
        trait_name: &str,
        substitution: &HashMap<String, Type>,
    ) -> Vec<(String, Vec<Option<Type>>)> {
        let arguments_of = |name: &str, pins: &HashMap<String, Type>| -> Vec<Option<Type>> {
            self.generics_of(name)
                .iter()
                .map(|generic| pins.get(&generic.name).cloned())
                .collect()
        };
        let own = (
            trait_name.to_string(),
            arguments_of(trait_name, substitution),
        );
        let above = self
            .declaring_types_above(trait_name, substitution)
            .into_iter()
            .map(|(name, pins)| {
                let arguments = arguments_of(&name, &pins);
                (name, arguments)
            });
        std::iter::once(own).chain(above).collect()
    }

    /// Whether a conversion's trait arguments and a call's agree: every
    /// position where both name a type that spells no parameter names the
    /// same one.
    fn arguments_agree(converted: &[Option<Type>], called: &[Option<Type>]) -> bool {
        let is_open = |ty: &Type| {
            spells_a_type(&ty.kind, &|kind| {
                matches!(kind, TypeKind::Generic(..) | TypeKind::Error)
            })
        };
        converted.iter().zip(called).all(|pair| match pair {
            (Some(converted), Some(called)) => {
                is_open(converted) || is_open(called) || converted.kind == called.kind
            }
            (None, _) | (_, None) => true,
        })
    }

    /// The payloads of two optionals, or the two types themselves.
    ///
    /// A value stored where an optional is declared without being one is
    /// settled at the payload type on its own, so only two optionals are
    /// looked through here.
    fn optional_payloads<'t>(expected: &'t Type, actual: &'t Type) -> (&'t Type, &'t Type) {
        if let (TypeKind::Option(expected), TypeKind::Option(actual)) =
            (&expected.kind, &actual.kind)
        {
            return (expected, actual);
        }
        (expected, actual)
    }
}

/// A method of a class instance a pinned body runs without a call written to
/// it.
struct MethodUse<'u> {
    class_name: &'u str,
    method: &'u str,
    instance: &'u Type,
    /// What `instance` pins its class's parameters to.
    substitution: &'u HashMap<String, Type>,
}

/// A derivation already made in one answering pass.
struct MadeDerivation {
    caller: Option<GenericBodyId>,
    class_name: String,
    method: String,
    instance: Type,
}

/// Where a derived site came from: the site whose body runs the method, and
/// the instance it runs it on.
#[derive(Debug, Clone)]
struct Origin {
    parent: usize,
    class_name: String,
    instance: Type,
}

/// The derivations one answering pass has made, and where each derived site
/// came from.
///
/// Derivation ends. Each is made at most once per caller, class, method and
/// instance, so an unending derivation would need an unending path of derived
/// sites, and along it finitely many repeat an instance of a class. Along a path, an instance of a class is refused where an earlier
/// instance of that class embeds in it — its constructors, in order, found in
/// the new one's. Types are finite trees over the names a program writes, with
/// every value argument counted as one label, so by Kruskal's tree theorem
/// every unending sequence of them has an earlier element embedded in a later
/// one: no path can go on forever. What is refused is exactly polymorphic
/// recursion — a class reaching itself at an instance grown from an earlier
/// one — and it is reported, never skipped in silence.
pub(crate) struct SiteDerivation {
    made: Vec<MadeDerivation>,
    /// One entry per site, `None` for a site the program records itself.
    origins: Vec<Option<Origin>>,
}

impl SiteDerivation {
    /// No derivation made yet over `recorded` sites the program records.
    pub(super) fn rooted_at(recorded: usize) -> Self {
        SiteDerivation {
            made: Vec::new(),
            origins: vec![None; recorded],
        }
    }

    /// Claim the derivation of `use_of` for `caller`, unless it was made
    /// already. Reports whether it was claimed.
    ///
    /// Instances are compared by structure, not by where they were written,
    /// so one instance reached along two paths is derived once.
    fn claim(&mut self, caller: &Option<GenericBodyId>, use_of: &MethodUse<'_>) -> bool {
        let made = self.made.iter().any(|done| {
            done.caller == *caller
                && done.class_name == use_of.class_name
                && done.method == use_of.method
                && same_structure(&done.instance.kind, &use_of.instance.kind)
        });
        if made {
            return false;
        }
        self.made.push(MadeDerivation {
            caller: caller.clone(),
            class_name: use_of.class_name.to_string(),
            method: use_of.method.to_string(),
            instance: use_of.instance.clone(),
        });
        true
    }

    /// The earlier instance of `use_of`'s class, on the path to the site at
    /// index `parent`, that embeds in `use_of`'s instance.
    fn embedded_ancestor(&self, parent: usize, use_of: &MethodUse<'_>) -> Option<Type> {
        let mut current = Some(parent);
        while let Some(index) = current {
            let origin = self.origins.get(index)?.as_ref()?;
            let grown = embeds(&origin.instance.kind, &use_of.instance.kind)
                && !embeds(&use_of.instance.kind, &origin.instance.kind);
            if origin.class_name == use_of.class_name && grown {
                return Some(origin.instance.clone());
            }
            current = Some(origin.parent);
        }
        None
    }
}

/// The site a method use derives from `parent`, pinning `body`: written where
/// `parent` is, and reading a type argument that is one of `parent`'s caller's
/// parameters as that parameter.
fn derived_site(
    parent: &PinningSite,
    body: GenericBodyId,
    pinned: &HashMap<String, Type>,
) -> PinningSite {
    let pins = pinned
        .iter()
        .map(|(parameter, ty)| {
            let own = super::generics::generic_parameter_name(&ty.kind)
                .filter(|name| parent.caller_parameters.iter().any(|known| known == name));
            let pin = match own {
                Some(name) => Pin::CallerParameter(name.to_string()),
                None => Pin::Concrete(ty.clone()),
            };
            (parameter.clone(), pin)
        })
        .collect();
    PinningSite {
        caller: parent.caller.clone(),
        callee: body,
        pins,
        span: parent.span,
        caller_parameters: parent.caller_parameters.clone(),
    }
}

/// The value an optional of `ty`, at any depth of nesting, holds, or `ty`
/// itself when it is no optional.
fn held_value(ty: &Type) -> &Type {
    let mut held = ty;
    while let TypeKind::Option(inner) = &held.kind {
        held = inner;
    }
    held
}

/// The constructor at the top of a type: a name for a named type, the kind
/// itself for every other.
#[derive(PartialEq)]
enum TypeHead<'k> {
    Named(&'k str),
    Other(std::mem::Discriminant<TypeKind>),
}

/// The head of `kind`.
fn type_head(kind: &TypeKind) -> TypeHead<'_> {
    match kind {
        TypeKind::Custom(name, _) | TypeKind::Generic(name, _, _) => TypeHead::Named(name),
        TypeKind::List(_)
        | TypeKind::Set(_)
        | TypeKind::Future(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Result(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Option(_)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_)
        | TypeKind::Function(_)
        | TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => TypeHead::Other(std::mem::discriminant(kind)),
    }
}

/// The types directly inside `kind`, in order. A value argument is not a
/// type and contributes none, so every value reads as the same leaf.
fn type_parts(kind: &TypeKind) -> Vec<&TypeKind> {
    match kind {
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
            spelled_type(element).into_iter().collect()
        }
        TypeKind::Array(element, size) => spelled_type(element)
            .into_iter()
            .chain(spelled_type(size))
            .collect(),
        TypeKind::Map(key, value) | TypeKind::Result(key, value) => spelled_type(key)
            .into_iter()
            .chain(spelled_type(value))
            .collect(),
        TypeKind::Tuple(elements) | TypeKind::Custom(_, Some(elements)) => {
            elements.iter().filter_map(spelled_type).collect()
        }
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            vec![&inner.kind]
        }
        TypeKind::Function(signature) => signature
            .params
            .iter()
            .filter_map(|param| spelled_type(&param.typ))
            .chain(signature.return_type.as_deref().and_then(spelled_type))
            .collect(),
        TypeKind::Custom(_, None)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => Vec::new(),
    }
}

/// The type an argument expression spells, or `None` for a value argument.
fn spelled_type(expr: &Expression) -> Option<&TypeKind> {
    if let crate::ast::expression::ExpressionKind::Type(ty, _) = &expr.node {
        return Some(&ty.kind);
    }
    None
}

/// Whether two types are the same type, wherever each was written.
fn same_structure(left: &TypeKind, right: &TypeKind) -> bool {
    embeds(left, right) && embeds(right, left)
}

/// Whether `small` embeds in `big`: `big` is `small` with types wrapped
/// around some of its parts — the same heads, in the same order, with
/// `small`'s parts each embedded in a distinct later part of `big`'s.
fn embeds(small: &TypeKind, big: &TypeKind) -> bool {
    if type_parts(big).into_iter().any(|part| embeds(small, part)) {
        return true;
    }
    if type_head(small) != type_head(big) {
        return false;
    }
    let small_parts = type_parts(small);
    let mut big_parts = type_parts(big).into_iter();
    small_parts
        .into_iter()
        .all(|part| big_parts.any(|candidate| embeds(part, candidate)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str, arguments: Option<Vec<TypeKind>>) -> TypeKind {
        let arguments = arguments.map(|arguments| {
            arguments
                .into_iter()
                .map(|argument| {
                    crate::ast::factory::type_expression(
                        Type::new(argument, Span::new(0, 0)),
                        false,
                    )
                })
                .collect()
        });
        TypeKind::Custom(name.to_string(), arguments)
    }

    #[test]
    fn a_type_embeds_in_itself_and_in_anything_wrapped_around_it() {
        let boxed = named("Box", Some(vec![TypeKind::String]));
        let wrapped = named("Holder", Some(vec![boxed.clone()]));
        assert!(embeds(&boxed, &boxed));
        assert!(embeds(&boxed, &wrapped));
        assert!(!embeds(&wrapped, &boxed));
    }

    #[test]
    fn a_class_at_a_grown_argument_embeds_its_smaller_instance() {
        let small = named("K", Some(vec![TypeKind::Int]));
        let grown = named("K", Some(vec![named("K", Some(vec![TypeKind::Int]))]));
        assert!(embeds(&small, &grown));
    }

    #[test]
    fn instances_of_one_class_at_unrelated_arguments_do_not_embed() {
        let first = named(
            "Holder",
            Some(vec![named("W", Some(vec![TypeKind::String]))]),
        );
        let second = named(
            "Holder",
            Some(vec![named("Box", Some(vec![TypeKind::String]))]),
        );
        assert!(!embeds(&first, &second));
    }

    #[test]
    fn two_spellings_of_one_type_have_the_same_structure() {
        let left = named("Box", Some(vec![TypeKind::String]));
        let right = named("Box", Some(vec![TypeKind::String]));
        assert!(same_structure(&left, &right));
    }
}
