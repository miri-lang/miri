// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `gpu for` capture validation.
//!
//! Walks the body of a `gpu for` looking for free identifiers (references to
//! outer-scope variables) that lower to WGSL storage buffers, and rejects two
//! classes of invalid capture:
//!
//!   * **Host-resident captures** (GPU_DRAFT §5.5, §6.4, §10.5). A buffer may
//!     only be captured into the kernel when its binding residency is `Gpu`.
//!     Capturing a host-resident binding is a type error with two
//!     machine-applicable fix-its (annotate with `gpu let`, or copy
//!     explicitly above the loop). There is no implicit upload — residency is
//!     source-visible.
//!   * **Non-buffer-eligible element types**. Even a gpu-resident buffer must
//!     hold a WGSL storage-buffer-eligible scalar. Bool is the motivating
//!     rejection: WGSL allows `bool` as a local but forbids it inside
//!     `var<storage>` bindings, so an `Array<Boolean, N>` capture would
//!     round-trip as invalid shader source.

use std::collections::HashSet;

use crate::ast::expression::LeftHandSideExpression;
use crate::ast::statement::BindingResidency;
use crate::ast::types::Type;
use crate::ast::{Expression, ExpressionKind, Statement, StatementKind, VariableDeclaration};
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::utils::{
    captured_buffer_element, is_gpu_buffer_element, is_residency_gated_buffer,
};
use crate::type_checker::TypeChecker;

/// A rejected `gpu for` capture, paired with the span of the offending
/// reference inside the loop body.
enum CaptureViolation {
    HostResident {
        name: String,
        span: Span,
    },
    NonBufferElement {
        name: String,
        elem_ty: Type,
        span: Span,
    },
}

impl TypeChecker {
    /// Reports a diagnostic for every invalid captured outer-scope buffer: a
    /// host-resident capture (must be gpu-resident), or a gpu-resident buffer
    /// whose element type is not WGSL-storage-eligible.
    ///
    /// Runs after the body's per-expression GPU-compatibility checks; both
    /// checks here are ones those checks cannot perform, since the captured
    /// variable was bound outside the kernel scope where the in-GPU predicate
    /// does not apply.
    pub(crate) fn check_gpu_for_captures(
        &mut self,
        loop_decls: &[VariableDeclaration],
        body: &Statement,
        context: &Context,
    ) {
        let mut bound: HashSet<String> = loop_decls.iter().map(|d| d.name.clone()).collect();
        let mut reported: HashSet<String> = HashSet::new();
        let mut violations: Vec<CaptureViolation> = Vec::new();

        visit_stmt(body, &mut bound, context, &mut reported, &mut violations);

        for violation in violations {
            self.report_capture_violation(violation);
        }
    }

    fn report_capture_violation(&mut self, violation: CaptureViolation) {
        match violation {
            CaptureViolation::HostResident { name, span } => self.report_error_with_help(
                format!("'gpu for' capture '{}' must be gpu-resident.", name),
                span,
                format!(
                    "Annotate the binding with 'gpu let', or copy explicitly: 'gpu let {}_gpu = {}'.",
                    name, name
                ),
            ),
            CaptureViolation::NonBufferElement {
                name,
                elem_ty,
                span,
            } => self.report_error(
                format!(
                    "'gpu for' capture '{}' has element type '{}' which is not a valid WGSL storage-buffer element. WGSL storage buffers require a numeric scalar (i32 / u32 / i64 / u64 / f32 / f64); 'bool' must be packed to 'i32' or 'u32'",
                    name, elem_ty
                ),
                span,
            ),
        }
    }
}

fn visit_stmt(
    stmt: &Statement,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            let scope_snapshot = bound.clone();
            for s in stmts {
                visit_stmt(s, bound, context, reported, violations);
            }
            *bound = scope_snapshot;
        }
        StatementKind::Expression(expr) => visit_expr(expr, bound, context, reported, violations),
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    visit_expr(init, bound, context, reported, violations);
                }
                bound.insert(d.name.clone());
            }
        }
        StatementKind::Return(Some(e)) => visit_expr(e, bound, context, reported, violations),
        StatementKind::Return(None) => {}
        StatementKind::If(cond, then_branch, else_branch, _) => {
            visit_expr(cond, bound, context, reported, violations);
            visit_stmt(then_branch, bound, context, reported, violations);
            if let Some(eb) = else_branch {
                visit_stmt(eb, bound, context, reported, violations);
            }
        }
        StatementKind::While(cond, body, _) => {
            visit_expr(cond, bound, context, reported, violations);
            visit_stmt(body, bound, context, reported, violations);
        }
        StatementKind::For(inner_decls, iter, body)
        | StatementKind::GpuFor(inner_decls, iter, body) => {
            visit_expr(iter, bound, context, reported, violations);
            let scope_snapshot = bound.clone();
            for d in inner_decls {
                bound.insert(d.name.clone());
            }
            visit_stmt(body, bound, context, reported, violations);
            *bound = scope_snapshot;
        }
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Enum(_, _, _, _, _, _)
        | StatementKind::Struct(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Trait(_, _, _, _, _)
        | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
        | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
    }
}

fn visit_expr(
    expr: &Expression,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => {
            check_captured_identifier(name, expr.span, bound, context, reported, violations);
        }
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            visit_expr(lhs, bound, context, reported, violations);
            visit_expr(rhs, bound, context, reported, violations);
        }
        ExpressionKind::Unary(_, inner)
        | ExpressionKind::Guard(_, inner)
        | ExpressionKind::NamedArgument(_, inner) => {
            visit_expr(inner, bound, context, reported, violations);
        }
        ExpressionKind::Call(callee, args) | ExpressionKind::EnumValue(callee, args) => {
            visit_call(callee, args, bound, context, reported, violations);
        }
        ExpressionKind::Index(left, right) | ExpressionKind::Member(left, right) => {
            visit_expr(left, bound, context, reported, violations);
            visit_expr(right, bound, context, reported, violations);
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            visit_lhs(lhs, bound, context, reported, violations);
            visit_expr(rhs, bound, context, reported, violations);
        }
        ExpressionKind::Conditional(cond, then_e, else_opt, _) => {
            visit_conditional(
                cond,
                then_e,
                else_opt.as_deref(),
                bound,
                context,
                reported,
                violations,
            );
        }
        ExpressionKind::Range(start, end, _) => {
            visit_range(start, end.as_deref(), bound, context, reported, violations);
        }
        ExpressionKind::Array(elems, _)
        | ExpressionKind::List(elems)
        | ExpressionKind::Tuple(elems)
        | ExpressionKind::Set(elems)
        | ExpressionKind::FormattedString(elems) => {
            visit_each(elems, bound, context, reported, violations);
        }
        ExpressionKind::Map(entries) => {
            visit_map_entries(entries, bound, context, reported, violations);
        }
        ExpressionKind::Match(scrutinee, branches) => {
            visit_match(scrutinee, branches, bound, context, reported, violations);
        }
        ExpressionKind::Block(stmts, final_expr) => {
            visit_block_expr(stmts, final_expr, bound, context, reported, violations);
        }
        ExpressionKind::Cast(value_expr, _target_type_expr) => {
            visit_expr(value_expr, bound, context, reported, violations);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Super
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::Lambda(_) => {}
    }
}

fn visit_call(
    callee: &Expression,
    args: &[Expression],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    visit_expr(callee, bound, context, reported, violations);
    for a in args {
        visit_expr(a, bound, context, reported, violations);
    }
}

fn visit_conditional(
    cond: &Expression,
    then_e: &Expression,
    else_e: Option<&Expression>,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    visit_expr(cond, bound, context, reported, violations);
    visit_expr(then_e, bound, context, reported, violations);
    if let Some(else_e) = else_e {
        visit_expr(else_e, bound, context, reported, violations);
    }
}

fn visit_match(
    scrutinee: &Expression,
    branches: &[crate::ast::pattern::MatchBranch],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    visit_expr(scrutinee, bound, context, reported, violations);
    for b in branches {
        if let Some(guard) = &b.guard {
            visit_expr(guard, bound, context, reported, violations);
        }
        visit_stmt(&b.body, bound, context, reported, violations);
    }
}

fn visit_range(
    start: &Expression,
    end: Option<&Expression>,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    visit_expr(start, bound, context, reported, violations);
    if let Some(end) = end {
        visit_expr(end, bound, context, reported, violations);
    }
}

fn visit_each(
    elems: &[Expression],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    for e in elems {
        visit_expr(e, bound, context, reported, violations);
    }
}

fn visit_map_entries(
    entries: &[(Expression, Expression)],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    for (k, v) in entries {
        visit_expr(k, bound, context, reported, violations);
        visit_expr(v, bound, context, reported, violations);
    }
}

fn visit_block_expr(
    stmts: &[Statement],
    final_expr: &Expression,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    let snap = bound.clone();
    for s in stmts {
        visit_stmt(s, bound, context, reported, violations);
    }
    visit_expr(final_expr, bound, context, reported, violations);
    *bound = snap;
}

fn visit_lhs(
    lhs: &LeftHandSideExpression,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    match lhs {
        LeftHandSideExpression::Identifier(expr)
        | LeftHandSideExpression::Member(expr)
        | LeftHandSideExpression::Index(expr) => {
            visit_expr(expr, bound, context, reported, violations);
        }
    }
}

fn check_captured_identifier(
    name: &str,
    span: Span,
    bound: &HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    violations: &mut Vec<CaptureViolation>,
) {
    if bound.contains(name) || reported.contains(name) {
        return;
    }
    let Some(info) = context.resolve_info(name) else {
        return;
    };
    let Some(elem_ty) = captured_buffer_element(&info.ty.kind) else {
        return;
    };
    if info.residency == BindingResidency::Host && is_residency_gated_buffer(&info.ty.kind) {
        reported.insert(name.to_string());
        violations.push(CaptureViolation::HostResident {
            name: name.to_string(),
            span,
        });
        return;
    }
    if is_gpu_buffer_element(&elem_ty.kind) {
        return;
    }
    reported.insert(name.to_string());
    violations.push(CaptureViolation::NonBufferElement {
        name: name.to_string(),
        elem_ty,
        span,
    });
}
