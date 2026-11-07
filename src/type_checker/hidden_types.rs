// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter named after a type already in scope.
//!
//! Inside the declaration the parameter hides that type, so every use of the
//! name means the parameter: `fn big<T implements Comparable, Hashable>`
//! declares a second, unbounded parameter called `Hashable`, not a second
//! bound on `T`, and the trait can no longer be named. Such a parameter is
//! refused where it is declared, when the module declaring it sees the type:
//! a type declared somewhere out of the module's reach hides nothing.

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::types::{TypeDeclarationKind, LINEAR_TYPE_NAME, OPTION_TYPE_NAME};
use crate::ast::{Expression, ExpressionKind};
use crate::diagnostics::DiagnosticCode;

/// How a reader calls a trait.
const TRAIT: &str = "trait";

/// How a reader calls a built-in type such as `Option`.
const BUILT_IN: &str = "type";

impl TypeChecker {
    /// Refuses each type parameter of `generics` named after a type in
    /// scope, where it is declared.
    pub(crate) fn refuse_type_parameters_hiding_types(
        &mut self,
        generics: &[Expression],
        context: &Context,
    ) {
        for (position, generic) in generics.iter().enumerate() {
            let Some(name) = generic_name(generic) else {
                continue;
            };
            let previous = position.checked_sub(1).and_then(|at| generics.get(at));
            self.refuse_type_parameter_hiding_a_type(name, previous, context);
        }
    }

    /// Refuses the type parameter `name`, written as `name.span`, when the
    /// module declaring it sees a type of that name. `previous` is the
    /// parameter written just before it: when that one has a bound and the
    /// hidden type is a trait, the reader most likely meant a second bound,
    /// and the help spells it.
    fn refuse_type_parameter_hiding_a_type(
        &mut self,
        name: &Expression,
        previous: Option<&Expression>,
        context: &Context,
    ) {
        let ExpressionKind::Identifier(name_text, _) = &name.node else {
            return;
        };
        let Some(what) = self.hidden_kind_in_scope(name_text, context) else {
            return;
        };
        let help = match previous.and_then(written_bound) {
            Some(bound) if what == TRAIT => {
                format!(
                    "`{name_text}` is a trait; to require both, write `{bound} and {name_text}`"
                )
            }
            Some(_) | None => {
                format!("give the type parameter a name no type in scope has, or rename the {what}")
            }
        };
        self.report_error_with_help(
            DiagnosticCode::TypShadowingNotAllowed,
            format!("type parameter `{name_text}` hides the {what} `{name_text}`"),
            name.span,
            help,
        );
    }

    /// What the type the module being checked sees under `name` is, as a
    /// reader calls it; `None` when it sees no type of that name, or when the
    /// name is another type parameter's.
    ///
    /// A module sees the types it declares, those its own `use` lines and the
    /// prelude bring in, and the built-in types written by name everywhere.
    /// Which names are registered as visible is tracked once, for the
    /// program, so a library module is checked while the program's types are
    /// registered too: its parameter `E` hides nothing the program declares.
    fn hidden_kind_in_scope(&self, name: &str, context: &Context) -> Option<&'static str> {
        match self.resolve_visible_type(name, context) {
            Some(definition) => hidden_kind(definition).filter(|_| {
                let identity = self.written_type_identity_in(name, context);
                self.modules.current_module_sees(identity)
            }),
            None => is_builtin_type_spelling(name).then_some(BUILT_IN),
        }
    }
}

/// The name a type parameter declaration writes, with its span.
fn generic_name(generic: &Expression) -> Option<&Expression> {
    let ExpressionKind::GenericType(name, _, _) = &generic.node else {
        return None;
    };
    Some(name)
}

/// Whether `name` spells a built-in type the checker resolves by its name
/// alone, with no declaration any module imports: it is in scope in every
/// module.
fn is_builtin_type_spelling(name: &str) -> bool {
    name == OPTION_TYPE_NAME || name == LINEAR_TYPE_NAME
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
