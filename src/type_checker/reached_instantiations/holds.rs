// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The class instances a body holds without building them. A collection
//! literal, a typed local, a value a call or a runtime entry returns: each
//! has a type, and every generic class instance that type spells, nested
//! inside another class's arguments included, is one lowering compiles a
//! body for, its `equals` among them, though no construction names it.
//!
//! A type spelling no parameter of the body holding it is the instance it
//! names; one spelling the body's own parameters names an instance at each
//! type the body is reached at, so it is recorded as a class that body
//! builds ([`TypeChecker::record_class_built_in_body`]).

use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::instantiation_limits::{instantiation_argument, MAX_INSTANCE_TYPE_DEPTH};
use crate::type_checker::instantiation_requirements::spells_a_type;
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};

/// A generic class at the arguments a held type spells it at.
type HeldClass = (String, Vec<Type>);

impl TypeChecker {
    /// Record each generic class instance `ty`, the type an expression the
    /// body being checked writes at `span` has, spells at that body's own
    /// parameters.
    pub(crate) fn record_classes_held_in_body(&mut self, ty: &Type, span: Span, context: &Context) {
        if self.suppress_diagnostics || context.current_function.is_none() {
            return;
        }
        let mut held = Vec::new();
        collect_held_classes(self.type_definitions(), &ty.kind, 0, &mut held);
        for (class, arguments) in held {
            self.record_class_built_in_body(&class, &arguments, span, context);
        }
    }

    /// Record each generic class instance at closed arguments a type the
    /// checker inferred spells, with the instances above it, in
    /// [`TypeChecker::reached_class_instantiations`]. Lowering compiles every
    /// one of them, whichever body holds it.
    pub(super) fn record_held_instances(&mut self) {
        let definitions = self.type_definitions();
        let mut held = Vec::new();
        for ty in self.type_table.types.values() {
            collect_held_classes(definitions, &ty.kind, 0, &mut held);
        }
        let mut seen = HashSet::new();
        held.retain(|(class, arguments)| {
            (arguments.iter()).all(|ty| is_closed(&ty.kind, definitions))
                && seen.insert((
                    class.clone(),
                    arguments
                        .iter()
                        .map(|ty| ty.kind.clone())
                        .collect::<Vec<_>>(),
                ))
        });
        held.sort_by(|(lhs, _), (rhs, _)| lhs.cmp(rhs));
        for (class, arguments) in held {
            self.record_with_bases(&class, arguments);
        }
    }
}

/// Append every generic class instance `kind` spells, each nested one
/// before the one holding it, down to [`MAX_INSTANCE_TYPE_DEPTH`] levels, as
/// lowering lists the instances a type it compiles holds.
fn collect_held_classes(
    definitions: &HashMap<String, TypeDefinition>,
    kind: &TypeKind,
    depth: usize,
    out: &mut Vec<HeldClass>,
) {
    if depth >= MAX_INSTANCE_TYPE_DEPTH {
        return;
    }
    let TypeKind::Custom(name, Some(arguments)) = kind else {
        return;
    };
    let Some(TypeDefinition::Class(class)) = definitions.get(name) else {
        return;
    };
    let declared = class.generics.as_deref().unwrap_or_default();
    let Some(arguments) = arguments
        .iter()
        .map(instantiation_argument)
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    if declared.is_empty() || arguments.len() != declared.len() {
        return;
    }
    for argument in &arguments {
        collect_held_classes(definitions, &argument.kind, depth + 1, out);
    }
    out.push((name.clone(), arguments));
}

/// Whether `kind` names a type an instance can be built at: it spells no
/// parameter left open, no type that failed to check, and no name the type
/// table holds no closed type for.
fn is_closed(kind: &TypeKind, definitions: &HashMap<String, TypeDefinition>) -> bool {
    !spells_a_type(kind, &|component| {
        if let TypeKind::Custom(name, None) = component {
            return !names_a_closed_type(name, definitions);
        }
        matches!(component, TypeKind::Generic(..) | TypeKind::Error)
    })
}

/// Whether the bare name `name` is a type with no parameters left to bind:
/// a class, struct or enum declaring none, or a trait.
fn names_a_closed_type(name: &str, definitions: &HashMap<String, TypeDefinition>) -> bool {
    match definitions.get(name) {
        Some(
            definition @ (TypeDefinition::Class(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)),
        ) => definition.generics().is_none_or(<[_]>::is_empty),
        Some(TypeDefinition::Trait(_)) => true,
        Some(TypeDefinition::Generic(_) | TypeDefinition::Alias(_)) | None => false,
    }
}
