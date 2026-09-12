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

use super::context::{Context, TypeDefinition};
use super::operators::missing_ordering_at_instantiation_message;
use super::TypeChecker;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
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

    /// Report every parameter of `body` that `substitution` pins to a type
    /// carrying no ordering.
    ///
    /// A parameter the substitution leaves open is skipped: one generic body
    /// calling another passes its own parameter, which names no type to judge.
    pub(crate) fn check_pinned_ordering(
        &mut self,
        body: &GenericBodyId,
        substitution: &HashMap<String, Type>,
        span: Span,
    ) {
        let Some(parameters) = self.ordering_requirements.get(body).cloned() else {
            return;
        };
        for parameter in parameters {
            let Some(pinned) = substitution.get(&parameter) else {
                continue;
            };
            if self.orders_its_values(pinned) {
                continue;
            }
            self.report_error_with_help(
                DiagnosticCode::TypOrderingNotSupported,
                missing_ordering_at_instantiation_message(pinned),
                span,
                format!(
                    "'{}' orders its '{}' parameter, so the type it is instantiated with has to \
                     define 'compare'",
                    body.1, parameter
                ),
            );
        }
    }

    /// True when any body declaring a method of this name orders one of its own
    /// generic parameters.
    ///
    /// Read before a member access rebuilds the receiver's substitution, so a
    /// receiver whose methods order nothing costs a scan of a map that holds one
    /// entry per ordering body in the program.
    pub(crate) fn orders_a_parameter_of(&self, method: &str) -> bool {
        self.ordering_requirements
            .keys()
            .any(|(_, declared)| declared == method)
    }

    /// Apply the requirements of `method` as reached through a receiver of type
    /// `class_name`, whose generic parameters `substitution` pins.
    ///
    /// A method's requirement is recorded against whichever type declares it,
    /// which for an inherited default method is a trait rather than the
    /// receiver's own class. The receiver's substitution is therefore re-keyed
    /// into each declaring type's parameter names before the requirement is
    /// answered.
    pub(crate) fn check_pinned_ordering_for_method(
        &mut self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        span: Span,
    ) {
        if substitution.is_empty() || self.ordering_requirements.is_empty() {
            return;
        }
        self.check_pinned_ordering(
            &(class_name.to_string(), method.to_string()),
            substitution,
            span,
        );
        let bindings = self.class_trait_param_bindings(class_name);
        for declaring in self.declaring_types_above(class_name) {
            let rekeyed = self.rekeyed_into(&declaring, substitution, &bindings);
            self.check_pinned_ordering(&(declaring, method.to_string()), &rekeyed, span);
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
