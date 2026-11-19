// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The function handed to a method at a parameter the method lends through
//! ([`crate::type_checker::lent_functions`]) keeps none of the values it is
//! lent.
//!
//! Inside the method a call through the parameter lends the arguments at
//! the positions the method's body hands a value it does not own whole, an
//! element of the receiver among them, so the function passed may only read
//! what it is handed there. A lambda literal or a named function is judged
//! by its own escape summary; a function value whose body cannot be seen is
//! refused where it would be lent a resource. A parameter of a type that is
//! no resource at the call is never refused: keeping it copies nothing a
//! hook is run for.

use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::ast::captures::collect_lambda_captures;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{FunctionTypeData, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::type_error::{TypeError, TypeErrorKind};

use super::super::escape_analysis::{free_callee, lambda_summary, EscapeSummary};
use super::super::open_consumption::OpenRefusal;
use super::{argument_value, UseAfterMoveChecker};

/// What the function handed at a lending parameter is, as far as its body
/// can be seen.
enum Handed<'s> {
    /// A function whose escape summary says which parameters it keeps.
    Seen(Cow<'s, EscapeSummary>),
    /// A lending parameter of the method being analysed: it keeps nothing
    /// at the positions it lends, and may keep every other argument.
    Lent(BTreeSet<usize>),
    /// A function value whose body cannot be seen.
    Unseen,
}

impl UseAfterMoveChecker<'_> {
    /// Refuses each function `args` hands the method `callee` calls at a
    /// parameter it lends through, when that function keeps a parameter of a
    /// resource type.
    pub(super) fn refuse_keepers_handed_to_lenders(
        &mut self,
        callee: &Expression,
        args: &[Expression],
    ) {
        let Some((type_name, method)) = self.method_call_target(callee) else {
            return;
        };
        let lending = (self.facts.lending).lending_at(&self.place_facts(), (type_name, method));
        let spelled = format!("{type_name}.{method}");
        for parameter in lending {
            if let Some(passed) = args.get(parameter.position).map(argument_value) {
                self.refuse_keeping_function(passed, (&spelled, parameter.name), &parameter.lent);
            }
        }
    }

    /// Refuses `passed`, handed to `method` at its lending parameter
    /// `parameter`, lending the positions `lent`, at the first of those of a
    /// resource type it may keep.
    fn refuse_keeping_function(
        &mut self,
        passed: &Expression,
        (method, parameter): (&str, &str),
        lent: &BTreeSet<usize>,
    ) {
        let Some(TypeKind::Function(function)) = self.types.get(&passed.id).map(|ty| &ty.kind)
        else {
            return;
        };
        let Some((kept, open)) = self.first_kept_resource(passed, function, lent) else {
            return;
        };
        let message = format!(
            "`{method}` lends the function passed as `{parameter}` its argument {}, \
             so that function cannot keep it",
            kept + 1
        );
        let error = TypeError {
            kind: TypeErrorKind::Coded {
                code: DiagnosticCode::OwnLentArgumentKept,
                message,
                help: Some(LENT_ARGUMENT_HELP.to_string()),
                expected: None,
                actual: None,
            },
            span: passed.span,
            source_override: None,
            repair: None,
            related: Vec::new(),
        };
        let (Some(parameter), Some(body)) = (open, self.open_body.clone()) else {
            self.errors.push(error);
            return;
        };
        self.open_refusals.push(OpenRefusal {
            error,
            body,
            parameter,
        });
    }

    /// The first parameter, by position, among the positions `lent`, of a
    /// resource type that the function `passed`, of the type `function`,
    /// may keep; with the open parameter of the body analysed its type is,
    /// for one that is a resource only at some copies of that body, which
    /// is refused at those copies.
    fn first_kept_resource(
        &self,
        passed: &Expression,
        function: &FunctionTypeData,
        lent: &BTreeSet<usize>,
    ) -> Option<(usize, Option<String>)> {
        let resources: Vec<(usize, Option<String>)> = (function.params.iter().enumerate())
            .filter(|(position, _)| lent.contains(position))
            .filter_map(|(position, param)| {
                let ExpressionKind::Type(ty, _) = &param.typ.node else {
                    return None;
                };
                if self.resources.holds(&ty.kind) {
                    return Some((position, None));
                }
                let open = self.open_body.as_ref()?;
                Some((
                    position,
                    Some(open.parameter_of(&ty.kind, self.type_definitions)?),
                ))
            })
            .collect();
        let mut kept = resources.into_iter();
        match self.handed_function(passed) {
            Handed::Lent(own) => kept.find(|(position, _)| !own.contains(position)),
            Handed::Unseen => kept.next(),
            Handed::Seen(summary) => kept.find(|(position, _)| summary.directly_escapes(*position)),
        }
    }

    /// What the function `passed` is, as far as its body can be seen.
    fn handed_function(&self, passed: &Expression) -> Handed<'_> {
        let facts = &self.facts;
        if let ExpressionKind::Lambda(lambda) = &passed.node {
            let enclosing = collect_lambda_captures(lambda);
            let summary = lambda_summary(lambda, enclosing, facts, self.escape_summaries);
            return Handed::Seen(Cow::Owned(summary));
        }
        let ExpressionKind::Identifier(name, _) = &passed.node else {
            return Handed::Unseen;
        };
        if let Some(own) = self.lent_functions.get(name) {
            return Handed::Lent(own.clone());
        }
        if self.fn_bindings.contains(name) {
            return Handed::Unseen;
        }
        let function = free_callee(passed, name, self.callees);
        (self.escape_summaries.get(&function)).map_or(Handed::Unseen, |summary| {
            Handed::Seen(Cow::Borrowed(summary))
        })
    }
}

/// The help every refusal of a function keeping a lent argument carries.
const LENT_ARGUMENT_HELP: &str = "pass a function that only reads what it is handed, and copy \
                                  out the plain data it needs to keep";
