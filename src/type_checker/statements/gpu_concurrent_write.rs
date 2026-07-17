// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Concurrent-write check for GPU kernel bodies (syntactic baseline).
//!
//! A GPU kernel launches one thread per index in its range. A write `arr[i] = e`
//! to a non-atomic gpu buffer is race-free only when `i` is a unique address
//! per thread. Static race-freedom is undecidable in general, so this pass
//! applies a conservative syntactic rule: the index must be a literal, a
//! thread-unique coordinate, or a linear function of thread-unique coordinates
//! (built from `+`, `-`, `*` over coordinates and constants). Any other index —
//! a buffer read used as a subscript (scatter), an integer division/modulo that
//! folds several threads onto one element, a call, or a value that is uniform
//! across the thread grid — is rejected as not provably unique.
//!
//! A thread-unique coordinate is a `forall` loop variable (for the `forall`
//! surface) or any `kernel` context access (`kernel.global_idx.x`,
//! `kernel.thread_idx.y`, `kernel.block_idx.x`, `kernel.warp.lane_id`, …) for the
//! explicit-launch `gpu fn` surface. Kernel builtins are scalars, never buffers,
//! so an index built from them is a coordinate/uniform atom and never a scatter
//! source. A nested CPU loop variable is *not* thread-unique — every thread runs
//! the full nested range — so an index that depends on it is rejected.
//!
//! Both surfaces route through the same walk. They differ only in how the set of
//! writable gpu buffers is discovered: the `forall` pass takes the captured
//! bindings; the `gpu fn` pass takes the buffer-typed parameters.
//!
//! Writes are flagged in subscript form (`buf[i] = e`) and in method form
//! (`buf.set(i, e)`). The rule biases toward "noisy but safe" over "permissive
//! but unsound": some provably-disjoint indices the checker cannot see are also
//! rejected. The escape hatch is an atomic element (`Array<Atomic<T>, N>`),
//! whose writes are race-free by construction and exempt from this pass.

use std::collections::HashSet;

use crate::ast::captures::collect_free_identifiers_excluding;
use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::operator::BinaryOp;
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{
    Type, TypeKind, ATOMIC_TYPE_NAME, GPU_CONTEXT_DEPRECATED_IDENT, KERNEL_CONTEXT_IDENT,
};
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::utils::captured_buffer_element;
use crate::type_checker::TypeChecker;

/// The index-write collection method (`buf.set(index, value)`), the method-form
/// equivalent of a `buf[index] = value` subscript write.
const INDEX_SET_METHOD: &str = "set";

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
        let buffers = self.captured_checkable_buffers(body, &loop_vars, context);
        self.check_kernel_body_writes(body, &buffers);
    }

    /// Reject every non-injective write to a non-atomic buffer parameter in an
    /// explicit-launch `gpu fn` body. Called once per GPU function after its body
    /// is type-checked, while the parameters are still resolvable in `context`.
    /// Thread uniqueness here is carried by `kernel.global_idx`/`thread_idx`
    /// rather than a `forall` loop variable.
    pub(crate) fn check_kernel_concurrent_writes(
        &mut self,
        params: &[Parameter],
        body: &Statement,
        context: &Context,
    ) {
        let buffers = self.parameter_checkable_buffers(params, context);
        self.check_kernel_body_writes(body, &buffers);
    }

    /// Walk a kernel body and flag each non-injective write to one of `buffers`.
    fn check_kernel_body_writes(&mut self, body: &Statement, buffers: &HashSet<String>) {
        if buffers.is_empty() {
            return;
        }
        let mut buffer_derived: HashSet<String> = HashSet::new();
        self.walk_writes(body, buffers, &mut buffer_derived);
    }

    /// The set of captured, non-atomic gpu-buffer names whose element writes a
    /// `forall` pass must validate. Atomic buffers are race-free by construction
    /// and excluded; body-local declarations (never free in `body`) are excluded.
    fn captured_checkable_buffers(
        &self,
        body: &Statement,
        loop_vars: &HashSet<String>,
        context: &Context,
    ) -> HashSet<String> {
        let captured = collect_free_identifiers_excluding(body, loop_vars);
        let mut buffers = HashSet::new();
        for name in captured {
            if self.is_non_atomic_buffer(&name, context) {
                buffers.insert(name);
            }
        }
        buffers
    }

    /// The set of non-atomic gpu-buffer parameter names whose element writes a
    /// `gpu fn` pass must validate. Atomic buffers are excluded.
    fn parameter_checkable_buffers(
        &self,
        params: &[Parameter],
        context: &Context,
    ) -> HashSet<String> {
        let mut buffers = HashSet::new();
        for param in params {
            if self.is_non_atomic_buffer(&param.name, context) {
                buffers.insert(param.name.clone());
            }
        }
        buffers
    }

    /// True if `name` resolves to a non-atomic gpu-buffer binding in `context`.
    fn is_non_atomic_buffer(&self, name: &str, context: &Context) -> bool {
        let Some(info) = context.resolve_info(name) else {
            return false;
        };
        captured_buffer_element(&info.ty.kind).is_some_and(|elem| !is_atomic_element(&elem))
    }

    /// Walk the body, tracking locals whose value derives from a non-injective
    /// source (a buffer read, or a nested-loop variable), and flag each
    /// non-injective buffer write. Also track buffer aliases: locals initialized
    /// to a buffer name become aliases that inherit write-checking obligations.
    fn walk_writes(
        &mut self,
        stmt: &Statement,
        buffers: &HashSet<String>,
        buffer_derived: &mut HashSet<String>,
    ) {
        let mut buffer_aliases: HashSet<String> = HashSet::new();
        self.walk_writes_with_aliases(stmt, buffers, buffer_derived, &mut buffer_aliases);
    }

    /// Inner walk with buffer-alias tracking. Aliases are locals initialized to
    /// a buffer or another alias, so writes through them must be checked.
    fn walk_writes_with_aliases(
        &mut self,
        stmt: &Statement,
        buffers: &HashSet<String>,
        buffer_derived: &mut HashSet<String>,
        buffer_aliases: &mut HashSet<String>,
    ) {
        match &stmt.node {
            StatementKind::Block(stmts) => {
                for inner in stmts {
                    self.walk_writes_with_aliases(inner, buffers, buffer_derived, buffer_aliases);
                }
            }
            StatementKind::Variable(decls, _) => {
                self.record_index_derivation_and_aliases(
                    decls,
                    buffer_derived,
                    buffers,
                    buffer_aliases,
                );
            }
            StatementKind::Expression(expr) => {
                self.check_write_expression(expr, buffers, buffer_derived, buffer_aliases);
            }
            StatementKind::If(_, then_branch, else_branch, _) => {
                self.walk_writes_with_aliases(then_branch, buffers, buffer_derived, buffer_aliases);
                if let Some(else_branch) = else_branch {
                    self.walk_writes_with_aliases(
                        else_branch,
                        buffers,
                        buffer_derived,
                        buffer_aliases,
                    );
                }
            }
            StatementKind::While(_, loop_body, _) => {
                self.walk_writes_with_aliases(loop_body, buffers, buffer_derived, buffer_aliases);
            }
            StatementKind::For(decls, _, loop_body) => {
                self.walk_nested_loop_with_aliases(
                    decls,
                    loop_body,
                    buffers,
                    buffer_derived,
                    buffer_aliases,
                );
            }
            _ => {}
        }
    }

    /// Walk a nested CPU loop. Its induction variables are the same across every
    /// `forall`/kernel thread, so writes indexed by them race; taint them as
    /// non-injective for the loop body, then restore the outer scope's taint.
    fn walk_nested_loop_with_aliases(
        &mut self,
        decls: &[VariableDeclaration],
        loop_body: &Statement,
        buffers: &HashSet<String>,
        buffer_derived: &mut HashSet<String>,
        buffer_aliases: &mut HashSet<String>,
    ) {
        let tainted: Vec<String> = decls
            .iter()
            .filter(|d| buffer_derived.insert(d.name.clone()))
            .map(|d| d.name.clone())
            .collect();
        self.walk_writes_with_aliases(loop_body, buffers, buffer_derived, buffer_aliases);
        for name in tainted {
            buffer_derived.remove(&name);
        }
    }

    /// Record any declared local whose initializer is not a provably-injective
    /// index expression; such a local (e.g. bound to a buffer read) taints any
    /// write it later subscripts. Also track buffer aliases: if a local is
    /// initialized to a buffer name (or another alias), it becomes an alias and
    /// must be checked as a buffer write target.
    fn record_index_derivation_and_aliases(
        &self,
        decls: &[VariableDeclaration],
        buffer_derived: &mut HashSet<String>,
        buffers: &HashSet<String>,
        buffer_aliases: &mut HashSet<String>,
    ) {
        for decl in decls {
            if let Some(init) = &decl.initializer {
                if let ExpressionKind::Identifier(source_name, _) = &init.node {
                    if buffers.contains(source_name) || buffer_aliases.contains(source_name) {
                        buffer_aliases.insert(decl.name.clone());
                        continue;
                    }
                }
                if !is_injective_index(init, buffer_derived) {
                    buffer_derived.insert(decl.name.clone());
                }
            }
        }
    }

    /// Flag a non-injective buffer write — in subscript (`buf[i] = e`) or method
    /// (`buf.set(i, e)`) form — and propagate index-derivation taint through a
    /// plain identifier reassignment.
    fn check_write_expression(
        &mut self,
        expr: &Expression,
        buffers: &HashSet<String>,
        buffer_derived: &mut HashSet<String>,
        buffer_aliases: &mut HashSet<String>,
    ) {
        if let ExpressionKind::Assignment(lhs, _, rhs) = &expr.node {
            self.check_subscript_write(lhs, buffers, buffer_derived, buffer_aliases, expr.span);
            self.propagate_reassignment_taint(lhs, rhs, buffer_derived, buffers, buffer_aliases);
            return;
        }
        self.check_method_set_write(expr, buffers, buffer_derived, buffer_aliases);
    }

    /// Flag a subscript write `buf[index] = e` when `buf` is a checkable buffer
    /// (or an alias of one) and `index` is not provably unique per thread.
    fn check_subscript_write(
        &mut self,
        lhs: &LeftHandSideExpression,
        buffers: &HashSet<String>,
        buffer_derived: &HashSet<String>,
        buffer_aliases: &HashSet<String>,
        span: Span,
    ) {
        let LeftHandSideExpression::Index(index_expr) = lhs else {
            return;
        };
        let ExpressionKind::Index(base, index) = &index_expr.node else {
            return;
        };
        if let ExpressionKind::Identifier(buf, _) = &base.node {
            let is_checkable = buffers.contains(buf) || buffer_aliases.contains(buf);
            if is_checkable && !is_injective_index(index, buffer_derived) {
                self.report_concurrent_write(buf, span);
            }
        }
    }

    /// Flag a method-form write `buf.set(index, value)` when `buf` is a checkable
    /// buffer (or an alias of one) and `index` is not provably unique per thread.
    /// This mirrors the subscript path for the collection index-write method.
    fn check_method_set_write(
        &mut self,
        expr: &Expression,
        buffers: &HashSet<String>,
        buffer_derived: &HashSet<String>,
        buffer_aliases: &HashSet<String>,
    ) {
        let ExpressionKind::Call(callee, args) = &expr.node else {
            return;
        };
        let ExpressionKind::Member(receiver, method) = &callee.node else {
            return;
        };
        if !is_named_method(method, INDEX_SET_METHOD) || args.len() != 2 {
            return;
        }
        if let ExpressionKind::Identifier(buf, _) = &receiver.node {
            let is_checkable = buffers.contains(buf) || buffer_aliases.contains(buf);
            if is_checkable && !is_injective_index(&args[0], buffer_derived) {
                self.report_concurrent_write(buf, expr.span);
            }
        }
    }

    /// Propagate index-derivation taint through a plain identifier reassignment
    /// (`name = rhs`): the local becomes tainted iff `rhs` is not injective.
    /// Also propagate buffer-alias status: if `rhs` is a buffer or alias, the
    /// reassigned local becomes an alias (fail-closed).
    fn propagate_reassignment_taint(
        &self,
        lhs: &LeftHandSideExpression,
        rhs: &Expression,
        buffer_derived: &mut HashSet<String>,
        buffers: &HashSet<String>,
        buffer_aliases: &mut HashSet<String>,
    ) {
        let LeftHandSideExpression::Identifier(name_expr) = lhs else {
            return;
        };
        if let ExpressionKind::Identifier(name, _) = &name_expr.node {
            if let ExpressionKind::Identifier(source, _) = &rhs.node {
                if buffers.contains(source) || buffer_aliases.contains(source) {
                    buffer_aliases.insert(name.clone());
                    return;
                }
            }
            if is_injective_index(rhs, buffer_derived) {
                buffer_derived.remove(name);
                buffer_aliases.remove(name);
            } else {
                buffer_derived.insert(name.clone());
                buffer_aliases.remove(name);
            }
        }
    }

    /// Emit the concurrent-write diagnostic for a buffer written at a
    /// non-injective index.
    fn report_concurrent_write(&mut self, buffer: &str, span: Span) {
        self.report_error(
            format!(
                "concurrent write to gpu buffer '{buffer}': the index is not provably unique per \
                 thread, so parallel threads may write the same element. Index by the per-thread \
                 coordinate (a 'forall' variable or 'kernel.global_idx', or a linear function of \
                 it), or use an atomic element ('Array<Atomic<T>, N>')."
            ),
            span,
        );
    }
}

/// True if `elem` is an `Atomic<T>` element type.
fn is_atomic_element(elem: &Type) -> bool {
    matches!(&elem.kind, TypeKind::Custom(name, _) if name == ATOMIC_TYPE_NAME)
}

/// True if `method` is the identifier `name` (a method selector expression).
fn is_named_method(method: &Expression, name: &str) -> bool {
    matches!(&method.node, ExpressionKind::Identifier(m, _) if m == name)
}

/// True if `expr` is a syntactically injective index over the per-thread
/// coordinates — a literal, an identifier (a loop variable or a uniform
/// constant), a `kernel` context access (`kernel.global_idx.x`,
/// `kernel.warp.lane_id`, …), or an affine combination built from `+`, `-`, `*`.
/// A buffer read, division, modulo, call, or any nested-loop-/buffer-derived
/// local is not injective.
fn is_injective_index(expr: &Expression, buffer_derived: &HashSet<String>) -> bool {
    match &expr.node {
        ExpressionKind::Literal(_) => true,
        ExpressionKind::Identifier(name, _) => !buffer_derived.contains(name),
        ExpressionKind::Member(..) => is_kernel_context_access(expr),
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

/// True if `expr` is a member access rooted at the `kernel` context identifier —
/// a thread-index coordinate (`kernel.global_idx.x`, `kernel.thread_idx.y`,
/// `kernel.block_idx.x`, `kernel.warp.lane_id`) or a uniform launch dimension
/// (`kernel.block_dim.x`). Kernel builtins are scalars, never buffers, so an
/// index built from them is a coordinate/uniform atom, never a scatter source.
fn is_kernel_context_access(expr: &Expression) -> bool {
    let ExpressionKind::Member(obj, _) = &expr.node else {
        return false;
    };
    is_kernel_context_identifier(obj) || is_kernel_context_access(obj)
}

/// True if `expr` is the `kernel` context identifier (or its deprecated alias).
fn is_kernel_context_identifier(expr: &Expression) -> bool {
    matches!(
        &expr.node,
        ExpressionKind::Identifier(name, _)
            if name == KERNEL_CONTEXT_IDENT || name == GPU_CONTEXT_DEPRECATED_IDENT
    )
}
