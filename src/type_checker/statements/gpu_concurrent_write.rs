// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Concurrent-write check for `forall` kernel bodies (syntactic baseline).
//!
//! A `forall` launches one thread per index in its range. A write `arr[i] = e`
//! to a non-atomic gpu buffer is race-free only when `i` is a unique address
//! per thread. Static race-freedom is undecidable in general, so this pass
//! applies a conservative syntactic rule: the index must be a literal, a
//! `forall` variable, or a linear function of `forall` variables (built from
//! `+`, `-`, `*` over variables and constants). Any other index — a buffer read
//! used as a subscript (scatter), an integer division/modulo that folds several
//! threads onto one element, or a call — is rejected as not provably unique.
//!
//! The rule biases toward "noisy but safe" over "permissive but unsound": some
//! provably-disjoint indices the checker cannot see are also rejected. The
//! escape hatch is an atomic element (`Array<Atomic<T>, N>`), whose writes are
//! race-free by construction and exempt from this pass.

use std::collections::HashSet;

use crate::ast::captures::collect_free_identifiers_excluding;
use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::operator::BinaryOp;
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{Type, TypeKind, ATOMIC_TYPE_NAME};
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::utils::captured_buffer_element;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reject every non-injective write to a captured non-atomic gpu buffer in a
    /// `forall` pass. Called once per pass after its body is type-checked, while
    /// the loop variables and captures are still resolvable in `context`.
    pub(crate) fn check_concurrent_writes(
        &mut self,
        decls: &[VariableDeclaration],
        body: &Statement,
        context: &Context,
    ) {
        let loop_vars: HashSet<String> = decls.iter().map(|d| d.name.clone()).collect();
        let buffers = self.checkable_buffers(body, &loop_vars, context);
        if buffers.is_empty() {
            return;
        }

        let mut buffer_derived: HashSet<String> = HashSet::new();
        self.walk_writes(body, &buffers, &mut buffer_derived);
    }

    /// The set of captured, non-atomic gpu-buffer names whose element writes
    /// this pass must validate. Atomic buffers are race-free by construction and
    /// excluded; body-local declarations (never free in `body`) are excluded.
    fn checkable_buffers(
        &self,
        body: &Statement,
        loop_vars: &HashSet<String>,
        context: &Context,
    ) -> HashSet<String> {
        let captured = collect_free_identifiers_excluding(body, loop_vars);
        let mut buffers = HashSet::new();
        for name in captured {
            let Some(info) = context.resolve_info(&name) else {
                continue;
            };
            if let Some(elem) = captured_buffer_element(&info.ty.kind) {
                if !is_atomic_element(&elem) {
                    buffers.insert(name);
                }
            }
        }
        buffers
    }

    /// Walk the body, tracking locals whose value derives from a buffer read (a
    /// disguised scatter source), and flag each non-injective buffer write.
    fn walk_writes(
        &mut self,
        stmt: &Statement,
        buffers: &HashSet<String>,
        buffer_derived: &mut HashSet<String>,
    ) {
        match &stmt.node {
            StatementKind::Block(stmts) => {
                for inner in stmts {
                    self.walk_writes(inner, buffers, buffer_derived);
                }
            }
            StatementKind::Variable(decls, _) => {
                self.record_index_derivation(decls, buffer_derived);
            }
            StatementKind::Expression(expr) => {
                self.check_write_expression(expr, buffers, buffer_derived);
            }
            StatementKind::If(_, then_branch, else_branch, _) => {
                self.walk_writes(then_branch, buffers, buffer_derived);
                if let Some(else_branch) = else_branch {
                    self.walk_writes(else_branch, buffers, buffer_derived);
                }
            }
            StatementKind::While(_, loop_body, _) => {
                self.walk_writes(loop_body, buffers, buffer_derived);
            }
            StatementKind::For(_, _, loop_body) => {
                self.walk_writes(loop_body, buffers, buffer_derived);
            }
            _ => {}
        }
    }

    /// Record any declared local whose initializer is not a provably-injective
    /// index expression; such a local (e.g. bound to a buffer read) taints any
    /// write it later subscripts.
    fn record_index_derivation(
        &self,
        decls: &[VariableDeclaration],
        buffer_derived: &mut HashSet<String>,
    ) {
        for decl in decls {
            if let Some(init) = &decl.initializer {
                if !is_injective_index(init, buffer_derived) {
                    buffer_derived.insert(decl.name.clone());
                }
            }
        }
    }

    /// Flag a non-injective buffer write, and propagate index-derivation taint
    /// through a plain identifier reassignment.
    fn check_write_expression(
        &mut self,
        expr: &Expression,
        buffers: &HashSet<String>,
        buffer_derived: &mut HashSet<String>,
    ) {
        let ExpressionKind::Assignment(lhs, _, rhs) = &expr.node else {
            return;
        };

        if let LeftHandSideExpression::Index(index_expr) = lhs.as_ref() {
            if let ExpressionKind::Index(base, index) = &index_expr.node {
                if let ExpressionKind::Identifier(buf, _) = &base.node {
                    if buffers.contains(buf) && !is_injective_index(index, buffer_derived) {
                        self.report_concurrent_write(buf, expr.span);
                    }
                }
            }
        }

        if let LeftHandSideExpression::Identifier(name_expr) = lhs.as_ref() {
            if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                if is_injective_index(rhs, buffer_derived) {
                    buffer_derived.remove(name);
                } else {
                    buffer_derived.insert(name.clone());
                }
            }
        }
    }

    /// Emit the concurrent-write diagnostic for a buffer written at a
    /// non-injective index.
    fn report_concurrent_write(&mut self, buffer: &str, span: Span) {
        self.report_error(
            format!(
                "concurrent write to gpu buffer '{buffer}': the index is not provably unique per \
                 thread, so parallel threads may write the same element. Index by the 'forall' \
                 variable (or a linear function of it), or use an atomic element \
                 ('Array<Atomic<T>, N>')."
            ),
            span,
        );
    }
}

/// True if `elem` is an `Atomic<T>` element type.
fn is_atomic_element(elem: &Type) -> bool {
    matches!(&elem.kind, TypeKind::Custom(name, _) if name == ATOMIC_TYPE_NAME)
}

/// True if `expr` is a syntactically injective index over the `forall`
/// variables — a literal, an identifier (a loop variable or a uniform constant),
/// or an affine combination built from `+`, `-`, `*`. A buffer read, division,
/// modulo, call, or any buffer-derived local is not injective.
fn is_injective_index(expr: &Expression, buffer_derived: &HashSet<String>) -> bool {
    match &expr.node {
        ExpressionKind::Literal(_) => true,
        ExpressionKind::Identifier(name, _) => !buffer_derived.contains(name),
        ExpressionKind::Unary(_, inner) | ExpressionKind::Cast(inner, _) => {
            is_injective_index(inner, buffer_derived)
        }
        ExpressionKind::Binary(lhs, op, rhs) => {
            matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
                && is_injective_index(lhs, buffer_derived)
                && is_injective_index(rhs, buffer_derived)
        }
        _ => false,
    }
}
