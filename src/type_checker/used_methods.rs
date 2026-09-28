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
//!   instance's `compare` or `equals`, which the element sites of
//!   [`PinningSite`] carry;
//! - an instance converted to a trait runs, through its vtable, each method
//!   the program calls through a receiver of that trait.
//!
//! All of them become sites on the one requirement rail in
//! [`super::instantiation_requirements`], answered by the same replay of the
//! body's own checks; nothing here judges a type.

use super::context::{resolve_method_source, Context, TypeDefinition};
use super::implicit_methods::{
    operator_method_name, CONSTRUCTION_METHOD_NAMES, OPERATOR_METHOD_NAMES,
};
use super::instantiation_requirements::{spells_a_type, ElementMethodSite, Pin, PinningSite};
use super::TypeChecker;
use crate::ast::expression::Expression;
use crate::ast::types::{Type, TypeKind};
use crate::ast::BinaryOp;
use crate::error::syntax::Span;
use std::cell::Cell;
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

/// A method called through a receiver whose type is a trait, which runs the
/// vtable slot of whatever instance the receiver holds.
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
        for method in CONSTRUCTION_METHOD_NAMES {
            self.record_method_pinning_sites(class_name, method, &substitution, span, context);
        }
    }

    /// Record the site `op` applied to a left operand of type `receiver`
    /// stands for, when the operand is a class instance whose class answers
    /// the operator with a method.
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
        let Some((class_name, substitution)) = self.class_instance(receiver) else {
            return;
        };
        self.record_method_pinning_sites(&class_name, method, &substitution, span, context);
    }

    /// Record that a value of `actual` is stored where `expected` is declared,
    /// when that converts a class instance to a trait.
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
        let expected = Self::optional_inner(expected);
        let TypeKind::Custom(trait_name, trait_args) = &expected.kind else {
            return;
        };
        if !matches!(
            self.type_table.global_type_definitions.get(trait_name),
            Some(TypeDefinition::Trait(_))
        ) {
            return;
        }
        let Some((class_name, substitution)) = self.class_instance(Self::optional_inner(actual))
        else {
            return;
        };
        let sites = self.method_pinning_sites(&class_name, "", &substitution, span, context, None);
        if sites.is_empty() {
            return;
        }
        let reached = self.traits_reached(trait_name, trait_args.as_deref());
        self.trait_conversions
            .push(TraitConversion { sites, reached });
    }

    /// Record a call to `method` through a receiver of the trait
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

    /// The methods an instance a pin names answers an operation with, each as
    /// the sites its use would record, waiting on the operation.
    ///
    /// Only instances smaller than `bound` are considered, and the sites of
    /// one of size `n` look only at instances smaller than `n`. A class's own
    /// arguments are smaller than the instance, so this reaches every
    /// element nested in what a program writes; what it skips is an argument
    /// a class's `extends` clause builds larger than the instance, which is
    /// the one way the expansion could otherwise go on forever.
    pub(super) fn element_method_sites(
        &self,
        pins: &HashMap<String, Pin>,
        span: Span,
        context: &Context,
        bound: Option<usize>,
    ) -> Vec<ElementMethodSite> {
        let mut parameters: Vec<&String> = pins.keys().collect();
        parameters.sort_unstable();
        let mut element_sites = Vec::new();
        for parameter in parameters {
            let Some(Pin::Concrete(pinned)) = pins.get(parameter) else {
                continue;
            };
            let size = type_size(pinned);
            if bound.is_some_and(|bound| size >= bound) {
                continue;
            }
            let Some((class_name, substitution)) = self.class_instance(pinned) else {
                continue;
            };
            let definitions = &self.type_table.global_type_definitions;
            for method in OPERATOR_METHOD_NAMES {
                if resolve_method_source(definitions, &class_name, method).is_none() {
                    continue;
                }
                let sites = self.method_pinning_sites(
                    &class_name,
                    method,
                    &substitution,
                    span,
                    context,
                    Some(size),
                );
                element_sites.extend(sites.into_iter().map(|site| ElementMethodSite {
                    parameter: parameter.clone(),
                    method,
                    site,
                }));
            }
        }
        element_sites
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
    /// `trait_name` at `type_args` pins its parameters to.
    fn traits_reached(
        &self,
        trait_name: &str,
        type_args: Option<&[Expression]>,
    ) -> Vec<(String, Vec<Option<Type>>)> {
        let own = (
            trait_name.to_string(),
            self.instance_arguments(trait_name, type_args),
        );
        let substitution = self.instance_substitution(trait_name, type_args);
        let above = self
            .declaring_types_above(trait_name, &substitution)
            .into_iter()
            .map(|(name, pins)| {
                let arguments = self
                    .generics_of(&name)
                    .iter()
                    .map(|generic| pins.get(&generic.name).cloned())
                    .collect();
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

    /// The type an optional wraps, or `ty` itself.
    fn optional_inner(ty: &Type) -> &Type {
        if let TypeKind::Option(inner) = &ty.kind {
            return inner;
        }
        ty
    }
}

/// The number of types `ty` spells, itself included.
fn type_size(ty: &Type) -> usize {
    let count = Cell::new(0);
    spells_a_type(&ty.kind, &|_| {
        count.set(count.get() + 1);
        false
    });
    count.get()
}
