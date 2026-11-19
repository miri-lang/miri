// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Records each copy a call to a `runtime` entry point makes of an element
//! ([`crate::type_checker::copy_judgement`]): one declared `@clones` copies
//! the element it is handed, one declared `@copies` every element of the
//! collection it copies.
//!
//! In a body compiled once per copy the element may be a value of an open
//! type parameter, and the copy is recorded for judging at each copy of the
//! body the type checker knows. Anywhere else its type is known, and a copy
//! of a resource the collection has no `clone` for is refused where it is
//! made.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::TypeKind;
use crate::error::syntax::Span;
use crate::runtime_effects::{ElementRole, ParamUse, ResultUse};

use super::super::copy_judgement::{copied_element_kinds, copy_refusal, refuses_copy_of, OpenCopy};
use super::{argument_value, UseAfterMoveChecker};

impl UseAfterMoveChecker<'_> {
    /// Records each element the call of `callee` with `args` copies, when
    /// `callee` names an entry point declared `@clones` or `@copies`.
    pub(super) fn record_element_copies(&mut self, callee: &Expression, args: &[Expression]) {
        let ExpressionKind::Identifier(name, _) = &callee.node else {
            return;
        };
        let Some(effect) = self.runtime_effects.get(name) else {
            return;
        };
        let types = self.types;
        for &position in &effect.clones {
            let (Some(arg), ParamUse::LooksUp(role)) = (args.get(position), effect.param(position))
            else {
                continue;
            };
            let copied = argument_value(arg);
            if let Some(ty) = types.get(&copied.id) {
                self.record_copy_at(&ty.kind, role, copied.span);
            }
        }
        let ResultUse::Copies(position) = effect.result else {
            return;
        };
        let Some(collection) = args.get(position).map(argument_value) else {
            return;
        };
        let Some(ty) = types.get(&collection.id) else {
            return;
        };
        for element in copied_element_kinds(&ty.kind, self.type_definitions) {
            self.record_copy_at(&element, ElementRole::Element, collection.span);
        }
    }

    /// Records the copy, made at `span`, of a value of `kind` stored as
    /// `role`: for judging at each copy of the body when `kind` is open in
    /// it, refused here when it is a resource the collection has no `clone`
    /// for.
    fn record_copy_at(&mut self, kind: &TypeKind, role: ElementRole, span: Span) {
        if let Some(body) = &self.open_body {
            if let Some(parameter) = body.parameter_of(kind, self.type_definitions) {
                self.open_copies.push(OpenCopy {
                    body: body.clone(),
                    parameter,
                    role,
                    span,
                    file: None,
                });
                return;
            }
        }
        if refuses_copy_of(kind, role, self.type_definitions, self.resources) {
            self.errors.push(copy_refusal(kind, None, span, Vec::new()));
        }
    }
}
