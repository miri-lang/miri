// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The rules a list of several bounds obeys where it is declared.
//!
//! `<T implements Comparable and Hashable>` asks for a type meeting every
//! bound, so a list no type can meet, or one that says a thing twice, is
//! refused where it is written rather than at each use: a bound written
//! twice, a type set beside another bound (a set already lists every type
//! the parameter may be), two classes (a type extends at most one), and two
//! bounds that declare one member with different signatures.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::context::{class_ancestry, MethodInfo, TypeDefinition};
use super::TypeChecker;
use crate::ast::bounds::{Bound, Bounds};
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
use crate::ast::{Expression, ExpressionKind};
use crate::diagnostics::DiagnosticCode;

impl TypeChecker {
    /// Refuse what a bound list says twice or what no type can meet. `bounds`
    /// are the resolved bounds of `subject`, parallel to `written`, the bound
    /// types as written. A list of one bound obeys every rule.
    pub(crate) fn validate_bounds(
        &mut self,
        subject: &str,
        bounds: &[Bound],
        written: &[Expression],
    ) {
        if bounds.len() < 2 {
            return;
        }
        self.refuse_repeated_bounds(subject, bounds, written);
        self.refuse_type_set_in_list(subject, bounds, written);
        self.refuse_second_class(subject, bounds, written);
        self.refuse_conflicting_members(subject, bounds, written);
    }

    /// Refuse a bound written a second time: it asks nothing the first did not.
    fn refuse_repeated_bounds(&mut self, subject: &str, bounds: &[Bound], written: &[Expression]) {
        for (at, (bound, expr)) in bounds.iter().zip(written).enumerate() {
            if !bounds[..at]
                .iter()
                .any(|seen| seen.ty.kind == bound.ty.kind)
            {
                continue;
            }
            self.report_error_with_help(
                DiagnosticCode::TypGenericTypeStructure,
                format!("the bound `{}` is written twice for `{subject}`", bound.ty),
                expr.span,
                format!(
                    "remove the repeated `{}`: the first one already requires it",
                    bound.ty
                ),
            );
        }
    }

    /// Refuse a type set in a list of several bounds: the set lists every
    /// type the parameter may be, so it bounds the parameter alone.
    fn refuse_type_set_in_list(&mut self, subject: &str, bounds: &[Bound], written: &[Expression]) {
        for (bound, expr) in bounds.iter().zip(written) {
            let is_set = bound.kind == TypeDeclarationKind::Is
                || matches!(bound.ty.kind, TypeKind::OneOf(_));
            if !is_set {
                continue;
            }
            let set = written_type_name(expr).unwrap_or_else(|| bound.ty.to_string());
            self.report_error_with_help(
                DiagnosticCode::TypGenericTypeStructure,
                format!("a type set (`is {set}`) is not combined with other bounds"),
                expr.span,
                format!(
                    "a type set lists every type `{subject}` may be, so it bounds `{subject}` \
                     alone: write `{subject} is {set}`"
                ),
            );
        }
    }

    /// Refuse a list naming a second class: a type extends at most one.
    fn refuse_second_class(&mut self, subject: &str, bounds: &[Bound], written: &[Expression]) {
        let mut classes = bounds
            .iter()
            .zip(written)
            .filter(|(bound, _)| self.names_a_class(&bound.ty));
        let Some((first, _)) = classes.next() else {
            return;
        };
        let Some((second, expr)) = classes.next() else {
            return;
        };
        self.report_error_with_help(
            DiagnosticCode::TypGenericTypeStructure,
            format!(
                "`{subject}` names two classes, `{}` and `{}`, in its bounds",
                first.ty, second.ty
            ),
            expr.span,
            "a type extends at most one class, so the other bounds must be traits".to_string(),
        );
    }

    fn names_a_class(&self, ty: &Type) -> bool {
        let TypeKind::Custom(name, _) = &ty.kind else {
            return false;
        };
        matches!(
            self.type_definitions().get(name),
            Some(TypeDefinition::Class(_))
        )
    }

    /// Refuse two bounds declaring one member with different signatures: a
    /// type declares a member once, so it cannot meet both.
    fn refuse_conflicting_members(
        &mut self,
        subject: &str,
        bounds: &[Bound],
        written: &[Expression],
    ) {
        let marker = make_type(TypeKind::Generic(subject.to_string(), Bounds::none()));
        let members: Vec<_> = bounds
            .iter()
            .map(|bound| self.bound_methods(&bound.ty, &marker))
            .collect();
        for (later, expr) in written.iter().enumerate() {
            for earlier in 0..later {
                let conflict = members[earlier].iter().find(|(name, method)| {
                    members[later]
                        .get(*name)
                        .is_some_and(|other| !same_signature(method, other))
                });
                let Some((member, _)) = conflict else {
                    continue;
                };
                self.report_error_with_help(
                    DiagnosticCode::TypGenericTypeStructure,
                    format!(
                        "the bounds `{}` and `{}` of `{subject}` both declare `{member}` with \
                         different signatures, so no type can meet both",
                        bounds[earlier].ty, bounds[later].ty
                    ),
                    expr.span,
                    format!(
                        "a type declares `{member}` once; drop one of the two bounds, or \
                         declare `{member}` the same way in both"
                    ),
                );
            }
        }
    }

    /// The methods a value of the bound type `bound` has: its own, those of
    /// the classes it extends and of every trait it reaches, the nearest
    /// first. Each is read with the type's `Self` written as `marker`, so two
    /// bounds' `fn twice() Self` read as one signature.
    fn bound_methods(&self, bound: &Type, marker: &Type) -> BTreeMap<String, MethodInfo> {
        let TypeKind::Custom(name, _) = &bound.kind else {
            return BTreeMap::new();
        };
        let defs = self.type_definitions();
        let mut owners: Vec<&BTreeMap<String, MethodInfo>> = Vec::new();
        let mut selves: Vec<String> = Vec::new();
        let mut traits: Vec<String> = Vec::new();
        match defs.get(name) {
            Some(TypeDefinition::Class(_)) => {
                for (class_name, class_def) in class_ancestry(name, defs) {
                    selves.push(class_name.to_string());
                    owners.push(&class_def.methods);
                    traits.extend(class_def.traits.iter().cloned());
                }
            }
            Some(TypeDefinition::Trait(_)) => traits.push(name.clone()),
            Some(
                TypeDefinition::Struct(_)
                | TypeDefinition::Enum(_)
                | TypeDefinition::Generic(_)
                | TypeDefinition::Alias(_),
            )
            | None => return BTreeMap::new(),
        }
        let mut seen = HashSet::new();
        while let Some(trait_name) = traits.pop() {
            if let Some(TypeDefinition::Trait(trait_def)) = defs.get(&trait_name) {
                if seen.insert(trait_name.clone()) {
                    owners.push(&trait_def.methods);
                    traits.extend(trait_def.parent_traits.iter().cloned());
                    selves.push(trait_name);
                }
            }
        }
        let as_marker: HashMap<String, Type> = selves
            .into_iter()
            .map(|own| (own, marker.clone()))
            .collect();
        let mut methods = BTreeMap::new();
        for (method_name, method) in owners.into_iter().flatten() {
            methods
                .entry(method_name.clone())
                .or_insert_with(|| self.method_read_at(method, &as_marker));
        }
        methods
    }

    /// `method` with each type of its signature rewritten by `mapping`.
    fn method_read_at(&self, method: &MethodInfo, mapping: &HashMap<String, Type>) -> MethodInfo {
        let mut read = method.clone();
        for (_, param) in &mut read.params {
            *param = self.substitute_type(param, mapping);
        }
        read.return_type = self.substitute_type(&method.return_type, mapping);
        read
    }
}

/// Whether two declarations of one member take and give the same types.
fn same_signature(one: &MethodInfo, other: &MethodInfo) -> bool {
    one.params.len() == other.params.len()
        && one
            .params
            .iter()
            .zip(&other.params)
            .all(|((_, a), (_, b))| a.kind == b.kind)
        && one.return_type.kind == other.return_type.kind
}

/// The type a bound expression writes, as written: `Real` rather than the
/// members the name resolves to.
fn written_type_name(expr: &Expression) -> Option<String> {
    let ExpressionKind::Type(written, _) = &expr.node else {
        return None;
    };
    Some(written.to_string())
}
