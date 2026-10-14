// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The exit status a bare `return` gives an entry point that declares none.
//!
//! A `main` written without a return type is compiled as one returning `int`,
//! the process exit status, with `return 0` appended. A bare `return` the
//! author wrote in that body means "stop here, successfully", so it is given
//! the same `0`; left bare, it would be checked against the `int` the entry
//! point was given and refused as returning nothing.
//!
//! Only returns that leave the entry point itself are rewritten. A function or
//! lambda declared inside it has a return type of its own, and a kernel body
//! (`forall`, `gpu frame`) returns from the kernel, not from `main`.

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::factory::int_literal_expression;
use crate::ast::statement::{Statement, StatementKind, VariableDeclaration};

/// Rewrite every bare `return` that leaves `body` as `return 0`.
pub fn bare_returns_exit_zero(body: &mut Statement) {
    statement(body);
}

fn statement(stmt: &mut Statement) {
    match &mut stmt.node {
        StatementKind::Return(value @ None) => {
            *value = Some(Box::new(int_literal_expression(0)));
        }
        StatementKind::Return(Some(value)) => expression(value),
        StatementKind::Block(stmts) => stmts.iter_mut().for_each(statement),
        StatementKind::Expression(expr) => expression(expr),
        StatementKind::Variable(decls, _) => variable_initializers(decls),
        StatementKind::If(cond, then_branch, else_branch, _) => {
            expression(cond);
            statement(then_branch);
            if let Some(else_branch) = else_branch {
                statement(else_branch);
            }
        }
        StatementKind::While(cond, body, _) => {
            expression(cond);
            statement(body);
        }
        StatementKind::For(decls, iterable, body) => {
            variable_initializers(decls);
            expression(iterable);
            statement(body);
        }
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Forall { .. }
        | StatementKind::GpuFrame(..)
        | StatementKind::GpuFrameBlock(_)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Use(..)
        | StatementKind::Type(..)
        | StatementKind::Enum(..)
        | StatementKind::Struct(..)
        | StatementKind::Class(_)
        | StatementKind::Trait(..)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => {}
    }
}

fn variable_initializers(decls: &mut [VariableDeclaration]) {
    for decl in decls {
        if let Some(initializer) = &mut decl.initializer {
            expression(initializer);
        }
    }
}

/// Reach the statements an expression carries: a `match` arm's body and a
/// block expression's statements. Everything else only holds expressions.
fn expression(expr: &mut Expression) {
    match &mut expr.node {
        ExpressionKind::Match(scrutinee, branches) => {
            expression(scrutinee);
            for branch in branches {
                if let Some(guard) = &mut branch.guard {
                    expression(guard);
                }
                statement(&mut branch.body);
            }
        }
        ExpressionKind::Block(stmts, tail) => {
            stmts.iter_mut().for_each(statement);
            expression(tail);
        }
        ExpressionKind::Binary(lhs, _, rhs)
        | ExpressionKind::Logical(lhs, _, rhs)
        | ExpressionKind::Member(lhs, rhs)
        | ExpressionKind::Index(lhs, rhs) => {
            expression(lhs);
            expression(rhs);
        }
        ExpressionKind::Unary(_, operand)
        | ExpressionKind::Guard(_, operand)
        | ExpressionKind::NamedArgument(_, operand)
        | ExpressionKind::Cast(operand, _) => expression(operand),
        ExpressionKind::Assignment(lhs, _, rhs) => {
            assignment_target(lhs);
            expression(rhs);
        }
        ExpressionKind::Conditional(then_expr, cond, else_expr, _) => {
            expression(then_expr);
            expression(cond);
            if let Some(else_expr) = else_expr {
                expression(else_expr);
            }
        }
        ExpressionKind::Range(start, end, _) => {
            expression(start);
            if let Some(end) = end {
                expression(end);
            }
        }
        ExpressionKind::Call(callee, args) | ExpressionKind::EnumValue(callee, args) => {
            expression(callee);
            args.iter_mut().for_each(expression);
        }
        ExpressionKind::List(elems)
        | ExpressionKind::Tuple(elems)
        | ExpressionKind::Set(elems)
        | ExpressionKind::FormattedString(elems, _)
        | ExpressionKind::Array(elems, _) => elems.iter_mut().for_each(expression),
        ExpressionKind::Map(entries) => {
            for (key, value) in entries {
                expression(key);
                expression(value);
            }
        }
        ExpressionKind::Lambda(_)
        | ExpressionKind::Type(..)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(..)
        | ExpressionKind::Super => {}
    }
}

fn assignment_target(lhs: &mut LeftHandSideExpression) {
    match lhs {
        LeftHandSideExpression::Identifier(e)
        | LeftHandSideExpression::Member(e)
        | LeftHandSideExpression::Index(e) => expression(e),
    }
}
