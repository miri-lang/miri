// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter named after a type already in scope.
//!
//! Inside the declaration the parameter hides that type, so every use of the
//! name means the parameter: `fn big<T implements Comparable, Hashable>`
//! declares a second, unbounded parameter called `Hashable`, not a second
//! bound on `T`, and the trait can no longer be named. Such a parameter is
//! refused where it is declared.

use super::context::{Context, TypeDefinition};
use super::module_loader::PROGRAM_MODULE;
use super::TypeChecker;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::{Expression, ExpressionKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;

/// How a reader calls a trait.
const TRAIT: &str = "trait";

impl TypeChecker {
    /// Refuses the type parameter `name`, declared at `span`, when a type of
    /// that name is in scope. `previous` is the parameter written just before
    /// it: when that one has a bound and the hidden type is a trait, the
    /// reader most likely meant a second bound, and the help spells it.
    pub(crate) fn refuse_type_parameter_hiding_a_type(
        &mut self,
        name: &str,
        span: Span,
        previous: Option<&Expression>,
        context: &Context,
    ) {
        let Some(hidden) = self.resolve_visible_type(name, context) else {
            return;
        };
        let Some(what) = hidden_kind(hidden) else {
            return;
        };
        if !self.declaration_sees(hidden) {
            return;
        }
        let help = match previous.and_then(written_bound) {
            Some(bound) if what == TRAIT => {
                format!("`{name}` is a trait; to require both, write `{bound} and {name}`")
            }
            Some(_) | None => {
                format!("give the type parameter a name no type in scope has, or rename the {what}")
            }
        };
        self.report_error_with_help(
            DiagnosticCode::TypShadowingNotAllowed,
            format!("type parameter `{name}` hides the {what} `{name}`"),
            span,
            help,
        );
    }
}

impl TypeChecker {
    /// Whether the module being checked sees the type `hidden` defines.
    ///
    /// Which type names are visible is tracked once, for the program: a
    /// module the program imports, the standard library's among them, is
    /// checked while the program's own types are visible too. Such a module
    /// sees only the types it declares itself, so a library parameter `E` is
    /// not refused for an `enum E` the program declares.
    fn declaration_sees(&self, hidden: &TypeDefinition) -> bool {
        let current = self.modules.current_module.as_str();
        current == PROGRAM_MODULE || defining_module(hidden).is_some_and(|module| module == current)
    }
}

/// The module that declares the type `definition` describes, where the
/// definition records one.
fn defining_module(definition: &TypeDefinition) -> Option<&str> {
    match definition {
        TypeDefinition::Trait(definition) => Some(&definition.module),
        TypeDefinition::Class(definition) => Some(&definition.module),
        TypeDefinition::Struct(definition) => Some(&definition.module),
        TypeDefinition::Enum(definition) => Some(&definition.module),
        TypeDefinition::Alias(_) | TypeDefinition::Generic(_) => None,
    }
}

/// What a hidden type is, as a reader calls it; `None` for another type
/// parameter, which a parameter of an inner declaration may not reuse either
/// but is refused by its own rule.
fn hidden_kind(definition: &TypeDefinition) -> Option<&'static str> {
    match definition {
        TypeDefinition::Trait(_) => Some(TRAIT),
        TypeDefinition::Class(_) => Some("class"),
        TypeDefinition::Struct(_) => Some("struct"),
        TypeDefinition::Enum(_) => Some("enum"),
        TypeDefinition::Alias(_) => Some("type"),
        TypeDefinition::Generic(_) => None,
    }
}

/// A type parameter with its bounds, as written (`T implements Comparable`);
/// `None` for one with no `extends`, `implements` or `includes` bound.
fn written_bound(generic: &Expression) -> Option<String> {
    let ExpressionKind::GenericType(name, bounds, kind) = &generic.node else {
        return None;
    };
    let ExpressionKind::Identifier(name, _) = &name.node else {
        return None;
    };
    let bounds = bounds
        .iter()
        .map(|bound| match &bound.node {
            ExpressionKind::Type(bound, _) => Some(bound.to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .filter(|bounds| !bounds.is_empty())?
        .join(" and ");
    match kind {
        TypeDeclarationKind::Extends
        | TypeDeclarationKind::Implements
        | TypeDeclarationKind::Includes => Some(format!("{name} {kind} {bounds}")),
        TypeDeclarationKind::Is | TypeDeclarationKind::None => None,
    }
}
