// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A default is a sibling argument the declaration spells: one that is an
//! earlier `out` parameter passes the value that parameter is handed a
//! second time.
//!
//! `fn store(r out Conn, keep Conn = r)` called as `store(c)` hands `c` to
//! `r` and, through the default, to `keep`. When `c` is a resource and
//! `store` keeps `keep`, the call keeps a second owner of `c` while `c` still
//! holds it, which `store(c, c)` spelled out is refused for
//! ([`super::passed_again_error`]). The default is judged at each call, as
//! the spelled-out argument is: whether the value is a resource is the
//! argument's type at the call, and a call passing `keep` explicitly never
//! evaluates the default.

use std::collections::HashMap;

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};

use super::super::escape_analysis::EscapeSummary;
use super::super::place_reads::value_spelling;
use super::{argument_value, UseAfterMoveChecker};

/// A call the default check reads: what it calls, the arguments it spells,
/// and the name it calls the callee by.
struct DefaultedCall<'e> {
    callee: &'e Expression,
    args: &'e [Expression],
    fn_name: &'e str,
}

impl<'a> UseAfterMoveChecker<'a> {
    /// Refuses each parameter `call` leaves to a default that hands on a
    /// resource taken `out`, read by the method's escape summary when the
    /// callee is a method, else by the function's.
    pub(super) fn refuse_kept_defaults(
        &mut self,
        (callee, args, fn_name): (&Expression, &[Expression], &str),
        (method, function): (Option<&EscapeSummary>, Option<&EscapeSummary>),
    ) {
        let call = DefaultedCall {
            callee,
            args,
            fn_name,
        };
        match (method, function) {
            (Some(summary), _) => {
                self.refuse_defaults_keeping_an_out_resource(&call, summary, true)
            }
            (None, Some(summary)) => {
                self.refuse_defaults_keeping_an_out_resource(&call, summary, false)
            }
            (None, None) => {}
        }
    }

    /// Refuses each parameter `call` leaves to its default when that default
    /// is an earlier `out` parameter handed a resource and the callee keeps
    /// the parameter, as `summary` says. `has_self_offset` shifts each
    /// position past a method's receiver.
    fn refuse_defaults_keeping_an_out_resource(
        &mut self,
        call: &DefaultedCall<'_>,
        summary: &EscapeSummary,
        has_self_offset: bool,
    ) {
        let types: &'a HashMap<usize, Type> = self.types;
        let Some(TypeKind::Function(function)) = types.get(&call.callee.id).map(|ty| &ty.kind)
        else {
            return;
        };
        let params = &function.params;
        let bound = bound_arguments(call.args, params);
        let offset = usize::from(has_self_offset);
        for (position, param) in params.iter().enumerate() {
            let is_left_out = bound.get(position).is_some_and(Option::is_none);
            let Some(out) = (param.default_value.as_deref())
                .filter(|_| is_left_out && summary.directly_escapes(position + offset))
                .and_then(|default| earlier_out_parameter(default, params))
                .filter(|out| *out < position)
            else {
                continue;
            };
            let handed = bound.get(out).copied().flatten();
            let out_name = params.get(out).map_or("", |out| out.name.as_str());
            if let Some(handed) = handed.filter(|handed| self.is_resource_expr(handed)) {
                let names = (call.fn_name, out_name, param.name.as_str());
                self.errors.push(kept_default_error(
                    &value_spelling(handed),
                    names,
                    handed.span,
                ));
            }
        }
    }
}

/// The value each parameter in `params` is handed by `args`, in parameter
/// order: `None` for one the call leaves to its default.
fn bound_arguments<'e>(
    args: &'e [Expression],
    params: &[Parameter],
) -> Vec<Option<&'e Expression>> {
    let mut bound = vec![None; params.len()];
    let mut next = 0usize;
    for arg in args {
        let position = if let ExpressionKind::NamedArgument(name, _) = &arg.node {
            params.iter().position(|p| &p.name == name)
        } else {
            next += 1;
            Some(next - 1)
        };
        if let Some(slot) = position.and_then(|position| bound.get_mut(position)) {
            *slot = Some(argument_value(arg));
        }
    }
    bound
}

/// The position among `params` of the `out` parameter `default` is, whole.
fn earlier_out_parameter(default: &Expression, params: &[Parameter]) -> Option<usize> {
    let ExpressionKind::Identifier(name, _) = &default.node else {
        return None;
    };
    (params.iter()).position(|param| param.is_out && &param.name == name)
}

/// The refusal of a resource handed to `fn_name`'s `out` parameter `out`
/// that the default of `kept`, which `fn_name` keeps, passes again.
fn kept_default_error(
    spelled: &str,
    (fn_name, out, kept): (&str, &str, &str),
    span: Span,
) -> TypeError {
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnUseOfMovedValue,
            message: format!(
                "'{spelled}' is passed to '{fn_name}' as '{out}', and the default of '{kept}' passes it again, and '{fn_name}' keeps it: a resource has one owner\n  fix: pass '{kept}' explicitly, a value of its own"
            ),
            help: None,
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related: Vec::new(),
    }
}
