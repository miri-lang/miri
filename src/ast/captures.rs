// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Identifier capture collection for nested scopes.
//!
//! Walks AST to collect all identifiers referenced in a statement that are not
//! bound by local declarations within the statement. Used by type checking (GPU
//! forall validation) and MIR lowering (GPU capture marshaling) to identify
//! which outer-scope variables must be captured.

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::{Statement, StatementKind};
use std::collections::HashSet;

/// Collects all outer-scope identifiers referenced in a statement.
///
/// Returns the set of names that are read/written in `stmt` but not bound
/// by local declarations within `stmt` itself. Results respect scope shadowing:
/// - Variables bound at the top level of `stmt` shadow outer bindings.
/// - Nested blocks re-snapshot the binding set (Forall/For/While bodies).
pub fn collect_free_identifiers(stmt: &Statement) -> HashSet<String> {
    let mut bound: HashSet<String> = HashSet::new();
    let mut captured = HashSet::new();
    collect_identifiers_in_stmt(stmt, &mut bound, &mut captured);
    captured
}

/// Collects identifiers referenced in a statement, excluding a pre-bound set.
///
/// Useful when certain identifiers (e.g., loop variables) should not be treated
/// as captures. `initially_bound` names are never added to `captured`.
pub fn collect_free_identifiers_excluding(
    stmt: &Statement,
    initially_bound: &HashSet<String>,
) -> HashSet<String> {
    let mut bound = initially_bound.clone();
    let mut captured = HashSet::new();
    collect_identifiers_in_stmt(stmt, &mut bound, &mut captured);
    captured
}

/// Collects all outer-scope identifiers referenced in an expression.
pub fn collect_free_identifiers_expr(expr: &Expression) -> HashSet<String> {
    let mut bound = HashSet::new();
    let mut captured = HashSet::new();
    collect_identifiers_in_expr(expr, &mut bound, &mut captured);
    captured
}

fn collect_identifiers_in_stmt(
    stmt: &Statement,
    bound: &mut HashSet<String>,
    captured: &mut HashSet<String>,
) {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            let scope_snapshot = bound.clone();
            for s in stmts {
                collect_identifiers_in_stmt(s, bound, captured);
            }
            *bound = scope_snapshot;
        }
        StatementKind::Expression(expr) => {
            collect_identifiers_in_expr(expr, bound, captured);
        }
        StatementKind::Variable(decls, _) => {
            for d in decls {
                if let Some(init) = &d.initializer {
                    collect_identifiers_in_expr(init, bound, captured);
                }
                bound.insert(d.name.clone());
            }
        }
        StatementKind::Return(Some(e)) => {
            collect_identifiers_in_expr(e, bound, captured);
        }
        StatementKind::Return(None) => {}
        StatementKind::If(cond, then_branch, else_branch, _) => {
            collect_identifiers_in_expr(cond, bound, captured);
            collect_identifiers_in_stmt(then_branch, bound, captured);
            if let Some(eb) = else_branch {
                collect_identifiers_in_stmt(eb, bound, captured);
            }
        }
        StatementKind::While(cond, body, _) => {
            collect_identifiers_in_expr(cond, bound, captured);
            collect_identifiers_in_stmt(body, bound, captured);
        }
        StatementKind::For(inner_decls, iter, body)
        | StatementKind::GpuFrame(inner_decls, iter, body) => {
            collect_identifiers_in_expr(iter, bound, captured);
            let scope_snapshot = bound.clone();
            for d in inner_decls {
                bound.insert(d.name.clone());
            }
            collect_identifiers_in_stmt(body, bound, captured);
            *bound = scope_snapshot;
        }
        StatementKind::Forall {
            vars: inner_decls,
            iterable: iter,
            body,
            ..
        } => {
            collect_identifiers_in_expr(iter, bound, captured);
            let scope_snapshot = bound.clone();
            for d in inner_decls {
                bound.insert(d.name.clone());
            }
            collect_identifiers_in_stmt(body, bound, captured);
            *bound = scope_snapshot;
        }
        StatementKind::GpuFrameBlock(block) => {
            collect_identifiers_in_stmt(block, bound, captured);
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

/// Collects all identifiers referenced in an expression that are not bound locally.
fn collect_identifiers_in_expr(
    expr: &Expression,
    bound: &mut HashSet<String>,
    captured: &mut HashSet<String>,
) {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => {
            if !bound.contains(name) {
                captured.insert(name.clone());
            }
        }
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            collect_identifiers_in_expr(lhs, bound, captured);
            collect_identifiers_in_expr(rhs, bound, captured);
        }
        ExpressionKind::Range(lhs, Some(rhs), _) => {
            collect_identifiers_in_expr(lhs, bound, captured);
            collect_identifiers_in_expr(rhs, bound, captured);
        }
        ExpressionKind::Range(lhs, None, _) => {
            collect_identifiers_in_expr(lhs, bound, captured);
        }
        ExpressionKind::Unary(_, e) => {
            collect_identifiers_in_expr(e, bound, captured);
        }
        ExpressionKind::Call(func, args) => {
            collect_identifiers_in_expr(func, bound, captured);
            for arg in args {
                collect_identifiers_in_expr(arg, bound, captured);
            }
        }
        ExpressionKind::Index(base, index) => {
            collect_identifiers_in_expr(base, bound, captured);
            collect_identifiers_in_expr(index, bound, captured);
        }
        ExpressionKind::Member(base, _) => {
            collect_identifiers_in_expr(base, bound, captured);
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            collect_identifiers_in_expr(rhs, bound, captured);
            match lhs.as_ref() {
                LeftHandSideExpression::Identifier(e) => {
                    if let ExpressionKind::Identifier(name, _) = &e.node {
                        if !bound.contains(name) {
                            captured.insert(name.clone());
                        }
                    }
                }
                LeftHandSideExpression::Index(e) | LeftHandSideExpression::Member(e) => {
                    collect_identifiers_in_expr(e, bound, captured);
                }
            }
        }
        ExpressionKind::Array(exprs, init_expr) => {
            for e in exprs {
                collect_identifiers_in_expr(e, bound, captured);
            }
            collect_identifiers_in_expr(init_expr, bound, captured);
        }
        ExpressionKind::List(exprs) => {
            for e in exprs {
                collect_identifiers_in_expr(e, bound, captured);
            }
        }
        ExpressionKind::Set(exprs) => {
            for e in exprs {
                collect_identifiers_in_expr(e, bound, captured);
            }
        }
        ExpressionKind::Tuple(exprs) => {
            for e in exprs {
                collect_identifiers_in_expr(e, bound, captured);
            }
        }
        ExpressionKind::Map(entries) => {
            for (k, v) in entries {
                collect_identifiers_in_expr(k, bound, captured);
                collect_identifiers_in_expr(v, bound, captured);
            }
        }
        ExpressionKind::Conditional(cond, then_e, else_opt, _) => {
            collect_identifiers_in_expr(cond, bound, captured);
            collect_identifiers_in_expr(then_e, bound, captured);
            if let Some(else_e) = else_opt {
                collect_identifiers_in_expr(else_e, bound, captured);
            }
        }
        ExpressionKind::Block(stmts, final_expr) => {
            let snap = bound.clone();
            for s in stmts {
                collect_identifiers_in_stmt(s, bound, captured);
            }
            collect_identifiers_in_expr(final_expr, bound, captured);
            *bound = snap;
        }
        ExpressionKind::Cast(value_expr, target_type_expr) => {
            collect_identifiers_in_expr(value_expr, bound, captured);
            collect_identifiers_in_expr(target_type_expr, bound, captured);
        }
        ExpressionKind::Match(scrutinee, branches) => {
            collect_identifiers_in_expr(scrutinee, bound, captured);
            for b in branches {
                if let Some(guard) = &b.guard {
                    collect_identifiers_in_expr(guard, bound, captured);
                }
                collect_identifiers_in_stmt(&b.body, bound, captured);
            }
        }
        ExpressionKind::EnumValue(name_expr, args) => {
            collect_identifiers_in_expr(name_expr, bound, captured);
            for a in args {
                collect_identifiers_in_expr(a, bound, captured);
            }
        }
        ExpressionKind::NamedArgument(_, inner) => {
            collect_identifiers_in_expr(inner, bound, captured);
        }
        ExpressionKind::Guard(_, inner) => {
            collect_identifiers_in_expr(inner, bound, captured);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Super
        | ExpressionKind::Type(_, _)
        | ExpressionKind::GenericType(_, _, _)
        | ExpressionKind::TypeDeclaration(_, _, _, _)
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::StructMember(_, _)
        | ExpressionKind::Lambda(_)
        | ExpressionKind::FormattedString(_) => {}
    }
}
