// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The names a parallel-loop body writes.
//!
//! A `forall` or `gpu frame` pass binds each buffer it captures either
//! read-only or read-write, and the type checker refuses a frame pass that
//! writes nothing. Both decisions rest on this one walk, so lowering and the
//! checker cannot disagree about whether a store counts: a store reached
//! through any statement or expression that holds one — a `match` arm, a block
//! expression, a conditional branch, a declaration's initializer — is a write.
//! A lambda body is not walked: it runs only if called, and a GPU kernel
//! cannot call a closure.

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::{Statement, StatementKind};
use crate::gpu_target::GpuAtomicOp;

/// How a body writes a named value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferWrite {
    /// An assignment whose target is rooted at the name (`buf[i] = v`,
    /// `buf[i].x = v`, `x = v`).
    Store,
    /// The name is the first argument of a call to an atomic builtin
    /// (`atomic_add(buf, i, 1)`).
    Atomic,
}

/// Reports every write in `stmt` to `sink` as `(name, kind)`, in source order.
///
/// The atomic builtins — the callees whose first argument is mutated — are
/// the ones [`GpuAtomicOp::from_builtin_name`] recognizes.
pub fn visit_buffer_writes(stmt: &Statement, sink: &mut dyn FnMut(&str, BufferWrite)) {
    let mut walker = WriteWalker { sink };
    walker.statement(stmt);
}

/// The walk's state: where writes are reported.
struct WriteWalker<'a> {
    sink: &'a mut dyn FnMut(&str, BufferWrite),
}

impl WriteWalker<'_> {
    fn statement(&mut self, stmt: &Statement) {
        match &stmt.node {
            StatementKind::Block(stmts) => stmts.iter().for_each(|s| self.statement(s)),
            StatementKind::Expression(expr) => self.expression(expr),
            StatementKind::Variable(decls, _) => decls
                .iter()
                .filter_map(|d| d.initializer.as_deref())
                .for_each(|init| self.expression(init)),
            StatementKind::Return(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            StatementKind::If(cond, then_branch, else_branch, _) => {
                self.expression(cond);
                self.statement(then_branch);
                if let Some(else_branch) = else_branch {
                    self.statement(else_branch);
                }
            }
            StatementKind::While(cond, body, _) => {
                self.expression(cond);
                self.statement(body);
            }
            StatementKind::For(_, iterable, body) | StatementKind::GpuFrame(_, iterable, body) => {
                self.expression(iterable);
                self.statement(body);
            }
            StatementKind::Forall { iterable, body, .. } => {
                self.expression(iterable);
                self.statement(body);
            }
            StatementKind::GpuFrameBlock(block) => self.statement(block),
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Use(_, _)
            | StatementKind::Type(_, _)
            | StatementKind::FunctionDeclaration(_)
            | StatementKind::Enum(_, _, _, _, _, _)
            | StatementKind::Struct(_, _, _, _, _, _)
            | StatementKind::Class(_)
            | StatementKind::Trait(_, _, _, _, _)
            | StatementKind::RuntimeFunctionDeclaration(_, _, _, _)
            | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
        }
    }

    fn expression(&mut self, expr: &Expression) {
        match &expr.node {
            ExpressionKind::Assignment(lhs, _, rhs) => {
                self.store(lhs);
                self.expression(rhs);
            }
            ExpressionKind::Call(callee, args) => self.call(callee, args),
            ExpressionKind::Match(scrutinee, branches) => {
                self.expression(scrutinee);
                for branch in branches {
                    if let Some(guard) = &branch.guard {
                        self.expression(guard);
                    }
                    self.statement(&branch.body);
                }
            }
            ExpressionKind::Block(stmts, value) => {
                stmts.iter().for_each(|s| self.statement(s));
                self.expression(value);
            }
            ExpressionKind::Conditional(cond, then_value, else_value, _) => {
                self.expression(cond);
                self.expression(then_value);
                if let Some(else_value) = else_value {
                    self.expression(else_value);
                }
            }
            ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
                self.expression(lhs);
                self.expression(rhs);
            }
            ExpressionKind::Index(base, index) => {
                self.expression(base);
                self.expression(index);
            }
            ExpressionKind::Member(base, _) => self.expression(base),
            ExpressionKind::Range(start, end, _) => {
                self.expression(start);
                if let Some(end) = end {
                    self.expression(end);
                }
            }
            ExpressionKind::Unary(_, inner)
            | ExpressionKind::Guard(_, inner)
            | ExpressionKind::NamedArgument(_, inner)
            | ExpressionKind::Cast(inner, _) => self.expression(inner),
            ExpressionKind::List(items)
            | ExpressionKind::Array(items, _)
            | ExpressionKind::Tuple(items)
            | ExpressionKind::Set(items)
            | ExpressionKind::FormattedString(items)
            | ExpressionKind::EnumValue(_, items) => items.iter().for_each(|e| self.expression(e)),
            ExpressionKind::Map(entries) => entries.iter().for_each(|(key, value)| {
                self.expression(key);
                self.expression(value);
            }),
            ExpressionKind::Lambda(_)
            | ExpressionKind::Identifier(_, _)
            | ExpressionKind::Literal(_)
            | ExpressionKind::Super
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::StructMember(_, _) => {}
        }
    }

    /// Reports the name an assignment target is rooted at, then walks the
    /// target's own subexpressions (an index may itself hold a write).
    fn store(&mut self, lhs: &LeftHandSideExpression) {
        let target = match lhs {
            LeftHandSideExpression::Identifier(e)
            | LeftHandSideExpression::Member(e)
            | LeftHandSideExpression::Index(e) => e,
        };
        if let Some(name) = place_root(target) {
            (self.sink)(name, BufferWrite::Store);
        }
        if let ExpressionKind::Index(base, index) = &target.node {
            self.expression(base);
            self.expression(index);
        }
    }

    /// An atomic builtin writes its first argument; every argument is walked.
    fn call(&mut self, callee: &Expression, args: &[Expression]) {
        if let ExpressionKind::Identifier(name, _) = &callee.node {
            if GpuAtomicOp::from_builtin_name(name).is_some() {
                if let Some(ExpressionKind::Identifier(buffer, _)) = args.first().map(|a| &a.node) {
                    (self.sink)(buffer, BufferWrite::Atomic);
                }
            }
        }
        args.iter().for_each(|arg| self.expression(arg));
    }
}

/// The variable a place expression is rooted at: `buf` for `buf`, `buf[i]`,
/// `buf[i].x` and `buf[i][j]`; `None` for a place rooted at anything else.
fn place_root(place: &Expression) -> Option<&str> {
    match &place.node {
        ExpressionKind::Identifier(name, _) => Some(name),
        ExpressionKind::Index(base, _) | ExpressionKind::Member(base, _) => place_root(base),
        ExpressionKind::Literal(_)
        | ExpressionKind::Binary(_, _, _)
        | ExpressionKind::Logical(_, _, _)
        | ExpressionKind::Unary(_, _)
        | ExpressionKind::Assignment(_, _, _)
        | ExpressionKind::Conditional(_, _, _, _)
        | ExpressionKind::Range(_, _, _)
        | ExpressionKind::Guard(_, _)
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
        | ExpressionKind::Match(_, _)
        | ExpressionKind::FormattedString(_)
        | ExpressionKind::NamedArgument(_, _)
        | ExpressionKind::Super
        | ExpressionKind::Block(_, _)
        | ExpressionKind::Cast(_, _) => None,
    }
}
