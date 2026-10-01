// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A local binding written without a type takes the type of its initializer,
//! and a variant constructor leaves open every type argument its payload does
//! not name: `var e = E.L(s)` is an `E<String, B>`. A value of that type is laid
//! out without `B`. When a later store into the binding binds `B` — `e = make()`
//! with an `E<String, i128>`, or `xs.push(make())` into a list of them — the
//! binding has to hold the wider layout from its declaration on, or the stored
//! value and the one already there disagree about where their fields are.
//!
//! Types are settled in one forward pass, so the store is reached after the
//! declaration was recorded. Rather than patching what that pass recorded, the
//! store records the binding's refined type and the function body is checked
//! again, with the declaration taking the refined type as if it were written
//! there. Every expression the body records — the initializer, each read of the
//! binding, each value built from one — is then recorded at the refined type
//! by the same rules a written type gets.
//!
//! A binding at module scope is read by every function, so no body's check
//! settles it; a store into one is still refused, as is one that a body fails to
//! settle within [`MAX_BODY_PASSES`].

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::Type;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::diagnostics::DiagnosticCollector;
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

/// How many times one function body is checked at most: once as written, and
/// once more for each round of refinements its stores ask for. Each round binds
/// at least one argument nothing bound before, so a body settles in a few; the
/// last pass refuses what is still left open rather than checking again.
const MAX_BODY_PASSES: usize = 4;

/// The refined types of local bindings, and whether the body being checked has
/// asked for another pass.
#[derive(Debug, Default)]
pub(crate) struct BindingRefinements {
    /// The refined type of each refined binding, keyed by the id of the
    /// initializer it was inferred from.
    refined: HashMap<usize, Type>,
    /// Whether a store in the current pass refined a binding, so the body
    /// must be checked again before what it recorded can be read.
    pending: bool,
    /// Whether a store may refine its binding: inside a function body that
    /// can still be checked again.
    open: bool,
}

/// What a pass over a function body may change that the next pass must find as
/// it was: the diagnostics, and the function's own scope (a parameter a pass
/// moves is marked consumed there).
struct BodyCheckSnapshot {
    errors: usize,
    warnings: usize,
    reported: std::collections::HashSet<(String, crate::error::syntax::Span)>,
    function_scope: Option<HashMap<String, SymbolInfo>>,
    inferred_return: Option<Option<Vec<(Type, crate::error::syntax::Span)>>>,
}

impl BodyCheckSnapshot {
    fn take(diagnostics: &DiagnosticCollector, context: &Context) -> Self {
        BodyCheckSnapshot {
            errors: diagnostics.errors.len(),
            warnings: diagnostics.warnings.len(),
            reported: diagnostics.reported_errors.clone(),
            function_scope: context.scopes.last().cloned(),
            inferred_return: context.inferred_return_types.last().cloned(),
        }
    }

    fn restore(&self, diagnostics: &mut DiagnosticCollector, context: &mut Context) {
        diagnostics.errors.truncate(self.errors);
        diagnostics.warnings.truncate(self.warnings);
        diagnostics.reported_errors = self.reported.clone();
        if let (Some(scope), Some(saved)) = (context.scopes.last_mut(), &self.function_scope) {
            *scope = saved.clone();
        }
        if let (Some(inferred), Some(saved)) = (
            context.inferred_return_types.last_mut(),
            &self.inferred_return,
        ) {
            *inferred = saved.clone();
        }
    }
}

impl TypeChecker {
    /// Check a function body with `check`, again for as long as a pass refines
    /// a binding the body declares, and return what the last pass returned.
    pub(crate) fn check_body_refining_bindings<R>(
        &mut self,
        context: &mut Context,
        mut check: impl FnMut(&mut TypeChecker, &mut Context) -> R,
    ) -> R {
        let outer_pending = std::mem::replace(&mut self.binding_refinements.pending, false);
        let outer_open = std::mem::replace(&mut self.binding_refinements.open, true);
        let snapshot = BodyCheckSnapshot::take(&self.diagnostics, context);
        let mut passes = 1;
        let mut result = check(self, context);
        while self.binding_refinements.pending {
            passes += 1;
            snapshot.restore(&mut self.diagnostics, context);
            self.binding_refinements.pending = false;
            self.binding_refinements.open = passes < MAX_BODY_PASSES;
            result = check(self, context);
        }
        self.binding_refinements.pending = outer_pending;
        self.binding_refinements.open = outer_open;
        result
    }

    /// The type a local binding written without a type takes, when a store
    /// into it refined the type its initializer `init` was inferred at.
    pub(crate) fn refined_binding_type(&self, init: &Expression) -> Option<Type> {
        self.binding_refinements.refined.get(&init.id).cloned()
    }

    /// Refine the local binding `place` is rooted in so that it can hold
    /// `source`, a value that binds the inference slots of `target`, the type
    /// the store reads `place` at. Returns whether it was refined, in which
    /// case the body is checked again and the store needs no refusal.
    pub(crate) fn refine_binding_for_store(
        &mut self,
        place: &Expression,
        target: &Type,
        source: &Type,
        context: &Context,
    ) -> bool {
        if !self.binding_refinements.open {
            return false;
        }
        let Some(info) = root_binding(place).and_then(|name| context.resolve_info(name)) else {
            return false;
        };
        let Some(init_id) = info.inferred_from else {
            return false;
        };
        let bound: HashMap<String, Type> = self
            .inference_slot_bindings(target, source, context)
            .into_iter()
            .collect();
        if bound.is_empty() {
            return false;
        }
        let refined = self.substitute_type(&info.ty, &bound);
        if refined == info.ty {
            return false;
        }
        self.binding_refinements.refined.insert(init_id, refined);
        self.binding_refinements.pending = true;
        true
    }
}

/// The name of the binding a stored-into place is part of: the binding itself,
/// or the one an index or field store reaches into.
fn root_binding(place: &Expression) -> Option<&str> {
    match &place.node {
        ExpressionKind::Identifier(name, _) => Some(name),
        ExpressionKind::Index(base, _) | ExpressionKind::Member(base, _) => root_binding(base),
        _ => None,
    }
}
