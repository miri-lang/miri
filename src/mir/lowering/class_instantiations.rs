// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Finding the generic-class instantiations a type spells.
//!
//! The pipeline fills the instantiation registry from the types the checker
//! inferred. A body lowered for one instantiation of a generic function or
//! class names further instantiations the checker never saw concretely —
//! `Box<T>` inside `via_box<T>` becomes `Box<String>` only once `via_box` is
//! lowered at `String` — and records them on the body so the pipeline can add
//! them too.

use super::context::LoweringContext;
use super::method_dispatch::resolve_generic_argument;
use crate::ast::types::{Type, TypeKind};
use crate::mir::body::GenericClassInstantiation;
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::TypeChecker;

/// How deep [`collect_generic_instantiations`] descends through a type's own
/// arguments. Matches the depth the symbol mangler names a type to.
const MAX_INSTANTIATION_NESTING: usize = 64;

/// Append every generic-class instantiation written inside `kind`, including
/// the ones nested in its own arguments (`List<List<W>>` yields both).
///
/// The descent stops at [`MAX_INSTANTIATION_NESTING`], past which the symbol
/// mangler has no name for the type anyway, so nothing below it could be given
/// a body.
///
/// For the same reason a class spelled at an argument the mangler has no token
/// for is left out, though its arguments are still searched. Inside a generic
/// class `self` has the class at its own parameters (`Box<T>`): that is the
/// generic definition, not an instantiation any call could reach.
pub(crate) fn collect_generic_instantiations(
    type_checker: &TypeChecker,
    kind: &TypeKind,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    collect_nested_instantiations(type_checker, kind, 0, out);
}

fn collect_nested_instantiations(
    type_checker: &TypeChecker,
    kind: &TypeKind,
    depth: usize,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    if depth >= MAX_INSTANTIATION_NESTING {
        return;
    }
    let TypeKind::Custom(name, Some(args)) = kind else {
        return;
    };
    let Some(TypeDefinition::Class(def)) = type_checker.type_definitions().get(name.as_str())
    else {
        return;
    };
    let Some(generics) = def.generics.as_ref() else {
        return;
    };
    let resolved: Option<Vec<Type>> = args
        .iter()
        .map(|arg| resolve_generic_argument(type_checker, arg))
        .collect();
    let Some(resolved) = resolved else {
        return;
    };
    if resolved.len() != generics.len() {
        return;
    }
    for arg in &resolved {
        collect_nested_instantiations(type_checker, &arg.kind, depth + 1, out);
    }
    if resolved
        .iter()
        .all(|arg| super::has_a_monomorphized_spelling(&arg.kind))
    {
        out.push((name.clone(), resolved));
    }
}

/// Whether the registry already holds `class` at exactly `type_args`.
pub(crate) fn is_registered_instantiation(
    type_checker: &TypeChecker,
    class: &str,
    type_args: &[Type],
) -> bool {
    type_checker
        .generic_class_instantiations
        .get(class)
        .is_some_and(|tuples| {
            tuples.iter().any(|tuple| {
                tuple.len() == type_args.len()
                    && tuple.iter().zip(type_args).all(|(a, b)| a.kind == b.kind)
            })
        })
}

impl LoweringContext<'_> {
    /// Record on the body each generic-class instantiation `ty` spells that the
    /// registry does not hold yet.
    ///
    /// Only a body lowered under an instantiation substitution records anything:
    /// everywhere else a type is what the checker inferred, and the registry was
    /// filled from exactly those. An instantiation with an argument that still
    /// has no concrete spelling names no body a call could reach, so it is left
    /// out.
    pub fn record_class_instantiations(&mut self, ty: &Type) {
        if self.generic_subs.is_empty() {
            return;
        }
        let found = self.unregistered_class_instantiations(std::iter::once(ty));
        self.push_class_instantiations(found);
    }

    /// Record the instantiations every local of the body is declared at.
    ///
    /// A binding, a parameter and a temporary are each dropped through the
    /// drop function of their own type, and a per-instantiation drop function
    /// exists only for a registered instantiation.
    pub fn record_local_class_instantiations(&mut self) {
        if self.generic_subs.is_empty() {
            return;
        }
        let found =
            self.unregistered_class_instantiations(self.body.local_decls.iter().map(|d| &d.ty));
        self.push_class_instantiations(found);
    }

    fn unregistered_class_instantiations<'t>(
        &self,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<GenericClassInstantiation> {
        let mut found = Vec::new();
        for ty in types {
            collect_generic_instantiations(self.type_checker, &ty.kind, &mut found);
        }
        let definitions = self.type_checker.type_definitions();
        found
            .into_iter()
            .filter(|(class, type_args)| {
                type_args
                    .iter()
                    .all(|arg| super::is_monomorphizable_type_argument(&arg.kind, definitions))
                    && !is_registered_instantiation(self.type_checker, class, type_args)
            })
            .map(|(class, type_args)| GenericClassInstantiation { class, type_args })
            .collect()
    }

    fn push_class_instantiations(&mut self, found: Vec<GenericClassInstantiation>) {
        for instantiation in found {
            if !self
                .body
                .generic_class_instantiations
                .contains(&instantiation)
            {
                self.body.generic_class_instantiations.push(instantiation);
            }
        }
    }
}
