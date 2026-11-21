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
//!
//! A place handed to an `out` parameter is written by the call whatever the
//! callee's body does with it. Which arguments bind an `out` parameter is the
//! type checker's to say, once it has resolved each callee; the walk asks it
//! through [`OutArgumentQuery`] and never guesses from a callee's name.

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::place_path::{path_links, path_root_name, PathLink};
use crate::ast::statement::{Statement, StatementKind};
use crate::gpu_target::GpuAtomicOp;

/// How a body writes a named value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferWrite<'e> {
    /// An assignment whose target is rooted at the name (`buf[i] = v`,
    /// `buf[i].x = v`, `x = v`).
    Store,
    /// The name is the first argument of a call to an atomic builtin
    /// (`atomic_add(buf, i, 1)`).
    Atomic,
    /// The name roots a place handed to an `out` parameter (`bump(buf[i])`,
    /// `bump(pts[i].x)`, `bump(x)`), which the call writes whatever its
    /// callee does. `index` is the subscript taken on the name itself, the
    /// element every thread of a parallel pass writes; `None` when the place
    /// is the whole binding or starts with a field of it.
    Out { index: Option<&'e Expression> },
}

/// Which arguments of a call bind an `out` parameter of its callee.
///
/// The answer is read off the callee's declaration once the call is type
/// checked; a named argument is asked about as the value it names.
pub trait OutArgumentQuery {
    /// Whether `argument`, written in `call`, binds an `out` parameter.
    fn binds_out(&self, call: &Expression, argument: &Expression) -> bool;
}

/// Reports every write in `stmt` to `sink` as `(name, kind)`, in source order.
///
/// The atomic builtins — the callees whose first argument is mutated — are
/// the ones [`GpuAtomicOp::from_builtin_name`] recognizes; the arguments
/// written through an `out` parameter are the ones `outs` names.
pub fn visit_buffer_writes<'e>(
    stmt: &'e Statement,
    outs: &dyn OutArgumentQuery,
    sink: &mut dyn FnMut(&'e str, BufferWrite<'e>),
) {
    let mut walker = WriteWalker { outs, sink };
    walker.statement(stmt);
}

/// Reports every write in `expr` to `sink` as `(name, kind)`, in source
/// order, as [`visit_buffer_writes`] does for a statement.
pub fn visit_expression_buffer_writes<'e>(
    expr: &'e Expression,
    outs: &dyn OutArgumentQuery,
    sink: &mut dyn FnMut(&'e str, BufferWrite<'e>),
) {
    let mut walker = WriteWalker { outs, sink };
    walker.expression(expr);
}

/// The walk's state: which arguments are `out`, and where writes are
/// reported.
struct WriteWalker<'w, 'e> {
    outs: &'w dyn OutArgumentQuery,
    sink: &'w mut dyn FnMut(&'e str, BufferWrite<'e>),
}

impl<'e> WriteWalker<'_, 'e> {
    fn statement(&mut self, stmt: &'e Statement) {
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
            | StatementKind::Trait(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => {}
        }
    }

    fn expression(&mut self, expr: &'e Expression) {
        match &expr.node {
            ExpressionKind::Assignment(lhs, _, rhs) => {
                self.store(lhs);
                self.expression(rhs);
            }
            ExpressionKind::Call(callee, args) => self.call(expr, callee, args),
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
            | ExpressionKind::FormattedString(items, _)
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
    fn store(&mut self, lhs: &'e LeftHandSideExpression) {
        let target = match lhs {
            LeftHandSideExpression::Identifier(e)
            | LeftHandSideExpression::Member(e)
            | LeftHandSideExpression::Index(e) => e,
        };
        if let Some(name) = path_root_name(target) {
            (self.sink)(name, BufferWrite::Store);
        }
        if let ExpressionKind::Index(base, index) = &target.node {
            self.expression(base);
            self.expression(index);
        }
    }

    /// An atomic builtin writes its first argument, and a call writes each
    /// argument bound to an `out` parameter; every argument is walked.
    fn call(&mut self, call: &'e Expression, callee: &'e Expression, args: &'e [Expression]) {
        if let ExpressionKind::Identifier(name, _) = &callee.node {
            if GpuAtomicOp::from_builtin_name(name).is_some() {
                if let Some(ExpressionKind::Identifier(buffer, _)) = args.first().map(|a| &a.node) {
                    (self.sink)(buffer, BufferWrite::Atomic);
                }
            }
        }
        for value in args.iter().map(named_value) {
            if self.outs.binds_out(call, value) {
                self.out_argument(value);
            }
        }
        args.iter().for_each(|arg| self.expression(arg));
    }

    /// Reports the name a place handed to an `out` parameter is rooted at,
    /// with the subscript taken on that name.
    fn out_argument(&mut self, place: &'e Expression) {
        if let Some(name) = path_root_name(place) {
            let index = root_subscript(place);
            (self.sink)(name, BufferWrite::Out { index });
        }
    }
}

/// The value an argument passes: the argument itself, or the value a named
/// argument names.
fn named_value(arg: &Expression) -> &Expression {
    if let ExpressionKind::NamedArgument(_, value) = &arg.node {
        value
    } else {
        arg
    }
}

/// The subscript taken on the binding `place` starts at: `i` in `dst[i]`,
/// `pts[i].x` and `grid[i][j]`; `None` for the binding itself or a field of
/// it.
fn root_subscript(place: &Expression) -> Option<&Expression> {
    let (_, links) = path_links(place);
    match links.first()? {
        PathLink::Index(_, index, _) => Some(index),
        PathLink::Member(..) => None,
    }
}
