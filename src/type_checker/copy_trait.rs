// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The copy trait, found by its attribute.
//!
//! The copy trait is the trait a module declares `@copy_trait`: a class
//! implementing it is copied through its `clone() Self` wherever a
//! collection copies an element, and a resource that does not is refused
//! where a copy of it is made. The compiler learns which trait that is from
//! the marker, never from its name. The marker is checked where the trait is
//! declared: the trait must declare the `clone() Self` a copy calls, and a
//! second marked trait is refused, so one trait is the copy trait.
//!
//! The answer is recorded on the trait's own definition
//! ([`TraitDefinition::is_copy_trait`]), which every later stage receives
//! with the type definitions, and read through
//! [`context::implements_copy_trait`] and [`context::copy_trait`].

use crate::ast::types::{is_trait_self, CLONE_METHOD_NAME};
use crate::ast::{Attribute, COPY_TRAIT_ATTRIBUTE};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::attributes::has_attribute;
use crate::type_checker::context::{self, TraitDefinition};
use crate::type_checker::TypeChecker;

/// Whether `definition` declares the `clone() Self` a copy calls: an
/// instance method taking nothing and answering the implementing type.
///
/// Only the trait's own methods count. A `clone` it inherits from a parent
/// trait does not, so the marked trait must declare `clone` itself.
fn declares_copying_clone(definition: &TraitDefinition) -> bool {
    definition
        .methods
        .get(CLONE_METHOD_NAME)
        .is_some_and(|clone| {
            !clone.is_static && clone.params.is_empty() && is_trait_self(&clone.return_type.kind)
        })
}

impl TypeChecker {
    /// The name of the copy trait, once a module has declared it.
    pub fn copy_trait(&self) -> Option<&str> {
        context::copy_trait(&self.type_table.global_type_definitions)
    }

    /// Takes the trait `definition`, declared at `span` with `attributes`, as
    /// the copy trait when it is marked so: it must declare `clone() Self`,
    /// and a second marked trait is refused.
    pub(crate) fn record_copy_trait(
        &mut self,
        definition: &mut TraitDefinition,
        attributes: &[Attribute],
        span: Span,
    ) {
        if !has_attribute(attributes, COPY_TRAIT_ATTRIBUTE) {
            return;
        }
        let name = definition.name.as_str();
        if !declares_copying_clone(definition) {
            let message = format!(
                "'{name}' is marked `@copy_trait`, but it declares no `clone() Self`, the \
                 method a copy calls"
            );
            let help = format!("declare `fn clone() Self` in '{name}', or remove `@copy_trait`");
            return self.report_error_with_help(
                DiagnosticCode::TypCopyTraitDeclaration,
                message,
                span,
                help,
            );
        }
        match self.copy_trait() {
            Some(existing) if existing != name => {
                let message = format!(
                    "'{name}' is marked `@copy_trait`, and '{existing}' already is: one trait \
                     is the copy trait"
                );
                let help = format!("remove `@copy_trait` from '{name}'");
                self.report_error_with_help(
                    DiagnosticCode::TypCopyTraitDeclaration,
                    message,
                    span,
                    help,
                );
            }
            Some(_) | None => definition.is_copy_trait = true,
        }
    }
}
