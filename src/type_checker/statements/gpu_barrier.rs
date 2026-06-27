// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Divergent-barrier deadlock guard for `gpu fn` bodies.
//!
//! `kernel.barrier()` lowers to a WGSL `workgroupBarrier()`, which every thread
//! in a workgroup must reach. A barrier placed under control flow whose
//! condition differs across threads — a `thread_idx`/`global_idx`-dependent
//! `if` or loop — is reached by some threads and not others, deadlocking the
//! workgroup on Metal/Vulkan. WGSL does not enforce barrier uniformity, so this
//! pass rejects such barriers at compile time.
//!
//! The analysis is a syntactic baseline: it tracks which locals are
//! *thread-varying* (derived from a per-thread index builtin) and flags any
//! barrier reached under a thread-varying guard. `block_idx`/`block_dim`/
//! `grid_dim` are uniform across a workgroup, so guards built only from them
//! stay uniform and their barriers compile.

use std::collections::HashSet;

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};
use crate::ast::types::{GPU_CONTEXT_DEPRECATED_IDENT, KERNEL_CONTEXT_IDENT};
use crate::type_checker::TypeChecker;

/// The per-thread index builtins. A control-flow guard derived from one of
/// these diverges across the workgroup; the per-block builtins do not.
const THREAD_VARYING_FIELDS: [&str; 2] = ["thread_idx", "global_idx"];

impl TypeChecker {
    /// Reject every `kernel.barrier()` reached under thread-divergent control
    /// flow within a `gpu fn` body. Called once per GPU function after its body
    /// is type-checked.
    pub(crate) fn check_barrier_uniformity(&mut self, body: &Statement) {
        let mut thread_varying: HashSet<String> = HashSet::new();
        self.walk_for_barrier(body, &mut thread_varying, false);
    }

    /// Recursively walk a statement, threading the set of thread-varying locals
    /// and whether the current position is under a thread-divergent guard.
    fn walk_for_barrier(
        &mut self,
        stmt: &Statement,
        thread_varying: &mut HashSet<String>,
        divergent: bool,
    ) {
        match &stmt.node {
            StatementKind::Block(stmts) => {
                for inner in stmts {
                    self.walk_for_barrier(inner, thread_varying, divergent);
                }
            }
            StatementKind::Variable(decls, _) => {
                self.record_thread_varying_decls(decls, thread_varying);
            }
            StatementKind::Expression(expr) => {
                self.check_expression_for_barrier(expr, thread_varying, divergent);
            }
            StatementKind::If(cond, then_branch, else_branch, _) => {
                let branch_divergent = divergent || is_thread_varying(cond, thread_varying);
                self.walk_for_barrier(then_branch, thread_varying, branch_divergent);
                if let Some(else_branch) = else_branch {
                    self.walk_for_barrier(else_branch, thread_varying, branch_divergent);
                }
            }
            StatementKind::While(cond, loop_body, _) => {
                let loop_divergent = divergent || is_thread_varying(cond, thread_varying);
                self.walk_for_barrier(loop_body, thread_varying, loop_divergent);
            }
            StatementKind::For(_, iterable, loop_body) => {
                let loop_divergent = divergent || is_thread_varying(iterable, thread_varying);
                self.walk_for_barrier(loop_body, thread_varying, loop_divergent);
            }
            _ => {}
        }
    }

    /// Add to `thread_varying` any declared local initialized from a
    /// thread-varying expression.
    fn record_thread_varying_decls(
        &self,
        decls: &[VariableDeclaration],
        thread_varying: &mut HashSet<String>,
    ) {
        for decl in decls {
            if let Some(init) = &decl.initializer {
                if is_thread_varying(init, thread_varying) {
                    thread_varying.insert(decl.name.clone());
                }
            }
        }
    }

    /// Handle an expression-statement: flag a divergent barrier, and propagate
    /// thread-varying-ness through assignment.
    fn check_expression_for_barrier(
        &mut self,
        expr: &Expression,
        thread_varying: &mut HashSet<String>,
        divergent: bool,
    ) {
        if is_barrier_call(expr) {
            if divergent {
                self.report_error(
                    "'kernel.barrier()' under thread-divergent control flow deadlocks the \
                     workgroup: some threads reach the barrier and others do not. Move the \
                     barrier to uniform control flow (reached by every thread in the workgroup)."
                        .to_string(),
                    expr.span,
                );
            }
            return;
        }

        if let ExpressionKind::Assignment(lhs, _, rhs) = &expr.node {
            if let LeftHandSideExpression::Identifier(name_expr) = lhs.as_ref() {
                if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                    if is_thread_varying(rhs, thread_varying) {
                        thread_varying.insert(name.clone());
                    }
                }
            }
        }
    }
}

/// True if `expr` is a `kernel.barrier()` (or the deprecated `gpu_context`
/// alias) call.
fn is_barrier_call(expr: &Expression) -> bool {
    let ExpressionKind::Call(func, _) = &expr.node else {
        return false;
    };
    let ExpressionKind::Member(obj, prop) = &func.node else {
        return false;
    };
    if !is_kernel_context_identifier(obj) {
        return false;
    }
    matches!(&prop.node, ExpressionKind::Identifier(name, _) if name == "barrier")
}

/// True if `expr` evaluates to a value that can differ across threads in a
/// workgroup — a per-thread index builtin, a thread-varying local, or any
/// expression built from one.
fn is_thread_varying(expr: &Expression, thread_varying: &HashSet<String>) -> bool {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => thread_varying.contains(name),
        ExpressionKind::Member(obj, prop) => {
            if is_thread_varying_builtin(obj, prop) {
                return true;
            }
            is_thread_varying(obj, thread_varying)
        }
        ExpressionKind::Index(base, index) => {
            is_thread_varying(base, thread_varying) || is_thread_varying(index, thread_varying)
        }
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            is_thread_varying(lhs, thread_varying) || is_thread_varying(rhs, thread_varying)
        }
        ExpressionKind::Unary(_, inner) | ExpressionKind::Guard(_, inner) => {
            is_thread_varying(inner, thread_varying)
        }
        ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
            is_thread_varying(cond, thread_varying)
                || is_thread_varying(then_expr, thread_varying)
                || else_expr
                    .as_ref()
                    .is_some_and(|e| is_thread_varying(e, thread_varying))
        }
        ExpressionKind::Range(start, end, _) => {
            is_thread_varying(start, thread_varying)
                || end
                    .as_ref()
                    .is_some_and(|e| is_thread_varying(e, thread_varying))
        }
        ExpressionKind::Call(_, args) => args.iter().any(|a| is_thread_varying(a, thread_varying)),
        _ => false,
    }
}

/// True if `obj.prop` reads a per-thread index builtin (`kernel.thread_idx` or
/// `kernel.global_idx`).
fn is_thread_varying_builtin(obj: &Expression, prop: &Expression) -> bool {
    if !is_kernel_context_identifier(obj) {
        return false;
    }
    matches!(&prop.node, ExpressionKind::Identifier(name, _) if THREAD_VARYING_FIELDS.contains(&name.as_str()))
}

/// True if `expr` is the `kernel` context identifier (or its deprecated alias).
fn is_kernel_context_identifier(expr: &Expression) -> bool {
    matches!(
        &expr.node,
        ExpressionKind::Identifier(name, _)
            if name == KERNEL_CONTEXT_IDENT || name == GPU_CONTEXT_DEPRECATED_IDENT
    )
}
