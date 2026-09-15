// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which generic parameters a body needs to carry an ordering, and the check
//! that applies that need where a parameter is pinned to a concrete type.
//!
//! A generic body is checked once, against its own parameters. An ordering
//! operator written there has no type to ask yet, so the check is deferred:
//! the parameter is recorded as needing an ordering, and every site that pins
//! it — a call to a generic function, a method reached through a generic
//! receiver — answers for the type it pins it to. Without the deferral the
//! operator would reach code generation with nothing to compare but the two
//! operands' addresses.
//!
//! Bodies are checked in source order, so a site can be checked before the body
//! it pins has stated anything. Sites are therefore only recorded during the
//! body pass. After it, requirements are settled — a body that pins another
//! body's ordering parameter to its own parameter orders that parameter too —
//! and every site is answered against the settled set.

use super::context::{Context, TypeDefinition};
use super::operators::missing_ordering_at_instantiation_message;
use super::TypeChecker;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeDeclarationKind, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use std::collections::{BTreeSet, HashMap};

/// The declaration a requirement was recorded against: the type that declares
/// the method, or [`FREE_FUNCTION_OWNER`] for a plain function, paired with the
/// function's own name.
pub(crate) type GenericBodyId = (String, String);

/// The owner half of a [`GenericBodyId`] for a function that no type declares.
pub(crate) const FREE_FUNCTION_OWNER: &str = "";

/// Every requirement recorded across the program, keyed by the body that stated
/// it and valued by the parameter names that body orders.
pub(crate) type OrderingRequirements = HashMap<GenericBodyId, BTreeSet<String>>;

/// What a site pins one generic parameter to.
#[derive(Debug)]
pub(crate) enum Pin {
    /// One of the pinning body's own generic parameters, named as that body
    /// declares it. Such a pin names no type to judge: it hands the requirement
    /// on to the sites that pin the pinning body.
    CallerParameter(String),
    /// A type the site names directly, judged against the requirement.
    Concrete(Type),
}

/// One place the program pins a generic body's parameters: a call to a generic
/// function, or a method reached through a receiver whose type arguments are
/// known.
#[derive(Debug)]
pub(crate) struct PinningSite {
    /// The body the site is written in, or `None` outside a function body.
    caller: Option<GenericBodyId>,
    /// The body whose parameters the site pins.
    callee: GenericBodyId,
    /// Each pinned parameter of `callee`, by the name `callee` declares it with.
    pins: HashMap<String, Pin>,
    span: Span,
}

/// Grow `requirements` until every body that pins another body's ordering
/// parameter to one of its own parameters orders that parameter too.
///
/// A requirement can only be added, and there are finitely many parameters to
/// add, so the loop ends — including when bodies delegate to each other in a
/// cycle.
fn settle_requirements(requirements: &mut OrderingRequirements, sites: &[PinningSite]) {
    loop {
        let mut inherited: Vec<(GenericBodyId, String)> = Vec::new();
        for site in sites {
            let (Some(caller), Some(required)) = (&site.caller, requirements.get(&site.callee))
            else {
                continue;
            };
            for parameter in required {
                if let Some(Pin::CallerParameter(own)) = site.pins.get(parameter) {
                    inherited.push((caller.clone(), own.clone()));
                }
            }
        }
        let mut grew = false;
        for (caller, own) in inherited {
            grew |= requirements.entry(caller).or_default().insert(own);
        }
        if !grew {
            return;
        }
    }
}

/// The body identifier for the declaration currently being checked, or `None`
/// outside a function body.
fn current_body(context: &Context) -> Option<GenericBodyId> {
    let function = context.current_function.clone()?;
    let owner = context
        .current_class
        .clone()
        .unwrap_or_else(|| FREE_FUNCTION_OWNER.to_string());
    Some((owner, function))
}

/// The generic-parameter name `ty` spells, when that name is in scope as a
/// parameter rather than as a declared type.
fn generic_parameter_in_scope<'t>(ty: &'t Type, context: &Context) -> Option<&'t str> {
    let name = super::generics::generic_parameter_name(&ty.kind)?;
    let is_parameter = matches!(
        context.resolve_type_definition(name),
        Some(TypeDefinition::Generic(_))
    );
    is_parameter.then_some(name)
}

impl TypeChecker {
    /// Record that the body being checked orders values of `ty`, when `ty` is
    /// one of that body's own generic parameters.
    ///
    /// Called for the left operand of every ordering operator, so a body that
    /// never orders a parameter records nothing and every one of its
    /// instantiations stays unconstrained.
    pub(crate) fn record_ordering_requirement(&mut self, ty: &Type, context: &Context) {
        let Some(parameter) = generic_parameter_in_scope(ty, context) else {
            return;
        };
        let Some(body) = current_body(context) else {
            return;
        };
        self.ordering_requirements
            .entry(body)
            .or_default()
            .insert(parameter.to_string());
    }

    /// Record that the body being checked hands a container to a call that
    /// orders that container's elements, when the element type is one of the
    /// body's own generic parameters.
    ///
    /// The runtime knows an element only by its size, so a sort it performs has
    /// nothing but the element's bytes to order by — which for a reference is
    /// where the value lives rather than what it is. A body written against a
    /// parameter states the need here, and the sites that pin the parameter
    /// answer it, the same way an ordering operator written in Miri does.
    pub(crate) fn record_elements_a_call_orders(
        &mut self,
        callee: &str,
        container: &Type,
        context: &Context,
    ) {
        if !crate::runtime_fns::orders_its_elements(callee) {
            return;
        }
        let Some(element) = self.sorted_element_type(container) else {
            return;
        };
        self.record_ordering_requirement(&element, context);
    }

    /// The element type of a container handed to such a call.
    ///
    /// A call written inside the container's own class names the receiver
    /// without type arguments — `List`, not `List<T>` — so the element is read
    /// from the class's own parameter list, in the position a written argument
    /// would occupy.
    fn sorted_element_type(&self, container: &Type) -> Option<Type> {
        if let Some(element) = container.kind.sequence_element_kind() {
            return Some(Type::new(element.clone(), container.span));
        }
        let TypeKind::Custom(name, None) = &container.kind else {
            return None;
        };
        if !matches!(
            BuiltinCollectionKind::from_name(name),
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        ) {
            return None;
        }
        let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(name.as_str())
        else {
            return None;
        };
        let parameter = class_def.generics.as_ref()?.first()?;
        Some(Type::new(
            TypeKind::Generic(parameter.name.clone(), None, TypeDeclarationKind::None),
            container.span,
        ))
    }

    /// Record that the body being checked pins `body`'s generic parameters as
    /// `substitution` spells them, to be answered by
    /// [`answer_pinning_sites`](Self::answer_pinning_sites).
    ///
    /// Nothing is judged here, because the body being pinned may be declared
    /// further down the source and not yet have stated what it orders. Whether
    /// each pin names the checking body's own parameter is decided now, while
    /// that body's scope is the one in `context`.
    pub(crate) fn record_pinning_site(
        &mut self,
        body: GenericBodyId,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) {
        if substitution.is_empty() || self.suppress_diagnostics {
            return;
        }
        let pins = substitution
            .iter()
            .map(|(parameter, pinned)| {
                let pin = match generic_parameter_in_scope(pinned, context) {
                    Some(name) => Pin::CallerParameter(name.to_string()),
                    None => Pin::Concrete(pinned.clone()),
                };
                (parameter.clone(), pin)
            })
            .collect();
        self.pinning_sites.push(PinningSite {
            caller: current_body(context),
            callee: body,
            pins,
            span,
        });
    }

    /// Record the sites a call to `method`, reached through a receiver of type
    /// `class_name` whose generic parameters `substitution` pins, stands for.
    ///
    /// A method's requirement is recorded against whichever type declares it,
    /// which for an inherited default method is a trait rather than the
    /// receiver's own class. The receiver's substitution is therefore re-keyed
    /// into each declaring type's parameter names, one site per declaring type.
    pub(crate) fn record_method_pinning_sites(
        &mut self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) {
        if substitution.is_empty() {
            return;
        }
        self.record_pinning_site(
            (class_name.to_string(), method.to_string()),
            substitution,
            span,
            context,
        );
        let bindings = self.class_trait_param_bindings(class_name);
        for declaring in self.declaring_types_above(class_name) {
            let rekeyed = self.rekeyed_into(&declaring, substitution, &bindings);
            self.record_pinning_site((declaring, method.to_string()), &rekeyed, span, context);
        }
    }

    /// Settle every requirement, then report each site that pins an ordering
    /// parameter to a type carrying no ordering.
    ///
    /// Runs once the body pass has recorded every requirement and every site,
    /// so a site is answered the same wherever it is written relative to the
    /// body it pins.
    pub(crate) fn answer_pinning_sites(&mut self) {
        let sites = std::mem::take(&mut self.pinning_sites);
        settle_requirements(&mut self.ordering_requirements, &sites);
        for site in &sites {
            self.answer_pinning_site(site);
        }
    }

    /// Report each ordering parameter `site` pins to a type carrying no ordering.
    fn answer_pinning_site(&mut self, site: &PinningSite) {
        let Some(parameters) = self.ordering_requirements.get(&site.callee).cloned() else {
            return;
        };
        for parameter in parameters {
            let Some(Pin::Concrete(pinned)) = site.pins.get(&parameter) else {
                continue;
            };
            if self.orders_its_values(pinned) {
                continue;
            }
            self.report_error_with_help(
                DiagnosticCode::TypOrderingNotSupported,
                missing_ordering_at_instantiation_message(pinned),
                site.span,
                format!(
                    "'{}' orders its '{}' parameter, so the type it is instantiated with has to \
                     define 'compare'",
                    site.callee.1, parameter
                ),
            );
        }
    }

    /// Re-key a receiver's substitution into `declaring`'s own parameter names.
    ///
    /// `bindings` maps each directly-implemented trait's parameter to the
    /// argument the class binds it to, written in the class's parameter terms;
    /// resolving that argument through the receiver's substitution yields the
    /// concrete type. A parameter with no binding keeps its own name, which is
    /// how a trait whose parameter the class names identically still resolves.
    fn rekeyed_into(
        &self,
        declaring: &str,
        substitution: &HashMap<String, Type>,
        bindings: &HashMap<String, Type>,
    ) -> HashMap<String, Type> {
        let Some(TypeDefinition::Trait(trait_def)) =
            self.type_table.global_type_definitions.get(declaring)
        else {
            return substitution.clone();
        };
        let Some(generics) = trait_def.generics.as_ref() else {
            return HashMap::new();
        };
        let mut rekeyed = HashMap::new();
        for generic in generics {
            let class_side = bindings.get(&generic.name).cloned().unwrap_or_else(|| {
                Type::new(
                    TypeKind::Generic(generic.name.clone(), None, TypeDeclarationKind::None),
                    Span::new(0, 0),
                )
            });
            let resolved = self.substitute_type(&class_side, substitution);
            rekeyed.insert(generic.name.clone(), resolved);
        }
        rekeyed
    }

    /// The traits a receiver of type `class_name` inherits methods from,
    /// each named once.
    fn declaring_types_above(&self, class_name: &str) -> Vec<String> {
        let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(class_name)
        else {
            return Vec::new();
        };
        let mut pending: Vec<String> = class_def.traits.clone();
        let mut seen: Vec<String> = Vec::new();
        while let Some(name) = pending.pop() {
            if seen.contains(&name) {
                continue;
            }
            if let Some(TypeDefinition::Trait(trait_def)) =
                self.type_table.global_type_definitions.get(&name)
            {
                pending.extend(trait_def.parent_traits.iter().cloned());
            }
            seen.push(name);
        }
        seen
    }
}
