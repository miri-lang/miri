// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The one rule for where a value must exist.
//!
//! A call that returns nothing has the type `void` and leaves no bytes behind,
//! so an expression that reads such a result has nothing to read. The operands
//! each expression form reads are listed once, in
//! [`for_each_value_operand`]; the forms that read a value without being an
//! expression's operand (a binding's initializer, a returned value) call
//! [`TypeChecker::demand_value`] themselves. An empty literal such as `{}` is
//! typed `Map<void, void>`, which is a value and does not trip the rule.

use super::diagnostics::spelled;
use super::expressions::types::yielded_expression;
use super::type_mismatch::TypeSides;
use super::TypeChecker;
use crate::ast::factory::make_type;
use crate::ast::types::TypeKind;
use crate::ast::{Expression, ExpressionKind, Statement};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;

/// Calls `visit` on every operand `kind` reads as a value. A form's other
/// children are named here too, so a new form has to say what it reads.
fn for_each_value_operand<'a>(kind: &'a ExpressionKind, visit: &mut impl FnMut(&'a Expression)) {
    match kind {
        ExpressionKind::Binary(left, _, right)
        | ExpressionKind::Logical(left, _, right)
        | ExpressionKind::Index(left, right) => {
            visit(left);
            visit(right);
        }
        ExpressionKind::Unary(_, operand)
        | ExpressionKind::Cast(operand, _)
        | ExpressionKind::Member(operand, _)
        | ExpressionKind::Match(operand, _)
        | ExpressionKind::Assignment(_, _, operand) => visit(operand),
        ExpressionKind::Range(start, end, _) => {
            visit(start);
            if let Some(end) = end {
                visit(end);
            }
        }
        ExpressionKind::Call(_, operands)
        | ExpressionKind::EnumValue(_, operands)
        | ExpressionKind::List(operands)
        | ExpressionKind::Set(operands)
        | ExpressionKind::Tuple(operands)
        | ExpressionKind::Array(operands, _)
        | ExpressionKind::FormattedString(operands, _) => operands.iter().for_each(visit),
        ExpressionKind::Map(entries) => entries.iter().for_each(|(key, value)| {
            visit(key);
            visit(value);
        }),
        ExpressionKind::Conditional(_, condition, _, _) => visit(condition),
        ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(_, _)
        | ExpressionKind::Guard(_, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::Lambda(_)
        | ExpressionKind::NamedArgument(_, _)
        | ExpressionKind::Super
        | ExpressionKind::Block(_, _) => {}
    }
}

impl TypeChecker {
    /// Refuses every operand `expr` reads that produces no value.
    pub(crate) fn demand_value_operands(&mut self, expr: &Expression) {
        for_each_value_operand(&expr.node, &mut |operand| self.demand_value(operand));
    }

    /// Refuses `value` when it, or a branch that can supply it, produces no
    /// value. Each branch is examined because a branch returning nothing is
    /// skipped when the type of the whole `if` or `match` is worked out. A
    /// branch that can never finish supplies nothing to read and needs no
    /// value, but one branch must supply a value for the whole to have a type.
    pub(crate) fn demand_value(&mut self, value: &Expression) {
        match &value.node {
            ExpressionKind::Conditional(then_branch, _, Some(else_branch), _) => {
                let finishing: Vec<&Expression> = [&**then_branch, &**else_branch]
                    .into_iter()
                    .filter(|branch| !self.expression_never_ends(branch, true))
                    .collect();
                self.demand_some_branch(finishing.is_empty(), value);
                finishing
                    .into_iter()
                    .for_each(|branch| self.demand_value(branch));
            }
            ExpressionKind::Match(_, branches) => {
                let finishing: Vec<&Statement> = branches
                    .iter()
                    .map(|branch| &*branch.body)
                    .filter(|body| !self.statement_never_finishes(body))
                    .collect();
                self.demand_some_branch(finishing.is_empty(), value);
                finishing.into_iter().for_each(|body| self.demand_arm(body));
            }
            ExpressionKind::Block(_, last) | ExpressionKind::NamedArgument(_, last) => {
                self.demand_value(last);
            }
            ExpressionKind::Conditional(_, _, None, _)
            | ExpressionKind::Literal(_)
            | ExpressionKind::Identifier(_, _)
            | ExpressionKind::Binary(_, _, _)
            | ExpressionKind::Logical(_, _, _)
            | ExpressionKind::Unary(_, _)
            | ExpressionKind::Assignment(_, _, _)
            | ExpressionKind::Range(_, _, _)
            | ExpressionKind::Guard(_, _)
            | ExpressionKind::Member(_, _)
            | ExpressionKind::Index(_, _)
            | ExpressionKind::Call(_, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::EnumValue(_, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::Lambda(_)
            | ExpressionKind::List(_)
            | ExpressionKind::Array(_, _)
            | ExpressionKind::Map(_)
            | ExpressionKind::Tuple(_)
            | ExpressionKind::Set(_)
            | ExpressionKind::FormattedString(_, _)
            | ExpressionKind::Super
            | ExpressionKind::Cast(_, _) => {
                self.refuse_void_value(value);
            }
        }
    }

    /// Refuses an `if` or `match` read as a value when no branch can finish:
    /// the others are what give the whole expression its type.
    fn demand_some_branch(&mut self, every_branch_leaves: bool, value: &Expression) {
        if every_branch_leaves {
            self.report_no_value(value.span);
        }
    }

    /// Demands a value of a `match` arm `body` that can finish: the expression
    /// it ends on, or a refusal when it ends on a statement.
    fn demand_arm(&mut self, body: &Statement) {
        match yielded_expression(body) {
            Some(yielded) => self.demand_value(yielded),
            None => self.report_no_value(body.span),
        }
    }

    /// Reports `value` when its type is `void`.
    fn refuse_void_value(&mut self, value: &Expression) {
        let is_void = self
            .type_table
            .types
            .get(&value.id)
            .is_some_and(|found| matches!(found.kind, TypeKind::Void));
        if is_void {
            self.report_no_value(value.span);
        }
    }

    /// Reports the absence of a value at `span`, unless an error at the same
    /// place already says so, as a mismatch against a declared type does.
    fn report_no_value(&mut self, span: Span) {
        if self.diagnostics.errors.iter().any(|e| e.span == span) {
            return;
        }
        let void = make_type(TypeKind::Void);
        self.report_type_mismatch(
            DiagnosticCode::TypTypeMismatch,
            format!(
                "this expression produces no value: its type is `{}`",
                spelled(&void)
            ),
            span,
            TypeSides::wanting("a value", &void),
            "call it on a line of its own, or make it return the value this place needs"
                .to_string(),
        );
    }
}
