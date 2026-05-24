// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `gpu for` capture-type validation.
//!
//! Walks the body of a `gpu for` looking for free identifiers (references to
//! outer-scope variables). For each free identifier whose resolved type is a
//! collection that would lower to a WGSL storage buffer, asserts the element
//! type is a buffer-eligible scalar. Bool is the motivating rejection: WGSL
//! allows `bool` as a local but forbids it inside `var<storage>` bindings, so
//! an `Array<Boolean, N>` capture would round-trip as invalid shader source.

use std::collections::HashSet;

use crate::ast::expression::LeftHandSideExpression;
use crate::ast::types::Type;
use crate::ast::{Expression, ExpressionKind, Statement, StatementKind, VariableDeclaration};
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::utils::{captured_buffer_element, is_gpu_buffer_element};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reports a diagnostic for every captured outer-scope variable whose
    /// type would lower to a WGSL storage buffer with a non-buffer-eligible
    /// element type.
    ///
    /// Runs after the body's per-expression GPU-compatibility checks; the
    /// element-scalar check here is the one those checks do not (and cannot)
    /// perform, since the captured variable was bound outside the kernel
    /// scope where the in-GPU predicate does not apply.
    pub(crate) fn check_gpu_for_capture_buffer_elements(
        &mut self,
        loop_decls: &[VariableDeclaration],
        body: &Statement,
        context: &Context,
    ) {
        let mut bound: HashSet<String> = loop_decls.iter().map(|d| d.name.clone()).collect();
        let mut reported: HashSet<String> = HashSet::new();
        let mut errors: Vec<(String, Type, Span)> = Vec::new();

        visit_stmt(body, &mut bound, context, &mut reported, &mut errors);

        for (name, elem_ty, span) in errors {
            self.report_error(
                format!(
                    "'gpu for' capture '{}' has element type '{}' which is not a valid WGSL storage-buffer element. WGSL storage buffers require a numeric scalar (i32 / u32 / i64 / u64 / f32 / f64); 'bool' must be packed to 'i32' or 'u32'",
                    name, elem_ty
                ),
                span,
            );
        }
    }
}

fn visit_stmt(
    stmt: &Statement,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            let scope_snapshot = bound.clone();
            for s in stmts {
                visit_stmt(s, bound, context, reported, errors);
            }
            *bound = scope_snapshot;
        }
        StatementKind::Expression(expr) => visit_expr(expr, bound, context, reported, errors),
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    visit_expr(init, bound, context, reported, errors);
                }
                bound.insert(d.name.clone());
            }
        }
        StatementKind::Return(Some(e)) => visit_expr(e, bound, context, reported, errors),
        StatementKind::Return(None) => {}
        StatementKind::If(cond, then_branch, else_branch, _) => {
            visit_expr(cond, bound, context, reported, errors);
            visit_stmt(then_branch, bound, context, reported, errors);
            if let Some(eb) = else_branch {
                visit_stmt(eb, bound, context, reported, errors);
            }
        }
        StatementKind::While(cond, body, _) => {
            visit_expr(cond, bound, context, reported, errors);
            visit_stmt(body, bound, context, reported, errors);
        }
        StatementKind::For(inner_decls, iter, body)
        | StatementKind::GpuFor(inner_decls, iter, body) => {
            visit_expr(iter, bound, context, reported, errors);
            let scope_snapshot = bound.clone();
            for d in inner_decls {
                bound.insert(d.name.clone());
            }
            visit_stmt(body, bound, context, reported, errors);
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
    errors: &mut Vec<(String, Type, Span)>,
) {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => {
            check_captured_identifier(name, expr.span, bound, context, reported, errors);
        }
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            visit_expr(lhs, bound, context, reported, errors);
            visit_expr(rhs, bound, context, reported, errors);
        }
        ExpressionKind::Unary(_, inner)
        | ExpressionKind::Guard(_, inner)
        | ExpressionKind::NamedArgument(_, inner) => {
            visit_expr(inner, bound, context, reported, errors);
        }
        ExpressionKind::Call(callee, args) | ExpressionKind::EnumValue(callee, args) => {
            visit_call(callee, args, bound, context, reported, errors);
        }
        ExpressionKind::Index(left, right) | ExpressionKind::Member(left, right) => {
            visit_expr(left, bound, context, reported, errors);
            visit_expr(right, bound, context, reported, errors);
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            visit_lhs(lhs, bound, context, reported, errors);
            visit_expr(rhs, bound, context, reported, errors);
        }
        ExpressionKind::Conditional(cond, then_e, else_opt, _) => {
            visit_conditional(
                cond,
                then_e,
                else_opt.as_deref(),
                bound,
                context,
                reported,
                errors,
            );
        }
        ExpressionKind::Range(start, end, _) => {
            visit_range(start, end.as_deref(), bound, context, reported, errors);
        }
        ExpressionKind::Array(elems, _)
        | ExpressionKind::List(elems)
        | ExpressionKind::Tuple(elems)
        | ExpressionKind::Set(elems)
        | ExpressionKind::FormattedString(elems) => {
            visit_each(elems, bound, context, reported, errors);
        }
        ExpressionKind::Map(entries) => {
            visit_map_entries(entries, bound, context, reported, errors);
        }
        ExpressionKind::Match(scrutinee, branches) => {
            visit_match(scrutinee, branches, bound, context, reported, errors);
        }
        ExpressionKind::Block(stmts, final_expr) => {
            visit_block_expr(stmts, final_expr, bound, context, reported, errors);
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
    errors: &mut Vec<(String, Type, Span)>,
) {
    visit_expr(callee, bound, context, reported, errors);
    for a in args {
        visit_expr(a, bound, context, reported, errors);
    }
}

fn visit_conditional(
    cond: &Expression,
    then_e: &Expression,
    else_e: Option<&Expression>,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    visit_expr(cond, bound, context, reported, errors);
    visit_expr(then_e, bound, context, reported, errors);
    if let Some(else_e) = else_e {
        visit_expr(else_e, bound, context, reported, errors);
    }
}

fn visit_match(
    scrutinee: &Expression,
    branches: &[crate::ast::pattern::MatchBranch],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    visit_expr(scrutinee, bound, context, reported, errors);
    for b in branches {
        if let Some(guard) = &b.guard {
            visit_expr(guard, bound, context, reported, errors);
        }
        visit_stmt(&b.body, bound, context, reported, errors);
    }
}

fn visit_range(
    start: &Expression,
    end: Option<&Expression>,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    visit_expr(start, bound, context, reported, errors);
    if let Some(end) = end {
        visit_expr(end, bound, context, reported, errors);
    }
}

fn visit_each(
    elems: &[Expression],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    for e in elems {
        visit_expr(e, bound, context, reported, errors);
    }
}

fn visit_map_entries(
    entries: &[(Expression, Expression)],
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    for (k, v) in entries {
        visit_expr(k, bound, context, reported, errors);
        visit_expr(v, bound, context, reported, errors);
    }
}

fn visit_block_expr(
    stmts: &[Statement],
    final_expr: &Expression,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    let snap = bound.clone();
    for s in stmts {
        visit_stmt(s, bound, context, reported, errors);
    }
    visit_expr(final_expr, bound, context, reported, errors);
    *bound = snap;
}

fn visit_lhs(
    lhs: &LeftHandSideExpression,
    bound: &mut HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
) {
    match lhs {
        LeftHandSideExpression::Identifier(expr)
        | LeftHandSideExpression::Member(expr)
        | LeftHandSideExpression::Index(expr) => {
            visit_expr(expr, bound, context, reported, errors);
        }
    }
}

fn check_captured_identifier(
    name: &str,
    span: Span,
    bound: &HashSet<String>,
    context: &Context,
    reported: &mut HashSet<String>,
    errors: &mut Vec<(String, Type, Span)>,
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
    if is_gpu_buffer_element(&elem_ty.kind) {
        return;
    }
    reported.insert(name.to_string());
    errors.push((name.to_string(), elem_ty, span));
}
