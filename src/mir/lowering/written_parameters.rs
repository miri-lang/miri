// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The copies a body binds on entry of the parameters it writes.
//!
//! A collection, a struct or a tuple passed to a function is a value: a write
//! to it inside the function is not seen by the caller. The caller still holds
//! the value it passed, often under more than one name, so its count says
//! nothing about whether the body may write it in place. A body that writes a
//! parameter, or writes through a path starting at one — a field, a tuple
//! element, a collection element, a map entry — is therefore lowered as if it
//! began with `var <name> = <name>`: the copy takes a reference of its own, and
//! every write goes through it, unsharing it first like any binding's value.
//! The caller's value is left alone and the copy is released when the body
//! ends, unless it is returned.
//!
//! Which parameters are written is read off the body before any of it is
//! lowered, so a read lowered before the write — a loop header — reads the
//! copy as well. A closure or nested function the body declares is a body of
//! its own and is not read here. Over-approximating costs one copy; missing a
//! write is caught where the write is lowered (`expression::unshare`).
//!
//! A method's receiver is never copied: a call to a method that writes it
//! hands it a value only the caller holds, so the body writes it in place.
//! Neither is an `out` parameter, which is the caller's binding itself.

use std::collections::HashSet;

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{Statement, StatementKind};
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::unshare::path_root;
use crate::mir::lowering::method_dispatch::is_receiver_written;
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::{
    Local, Operand, Place, Rvalue, Statement as MirStatement, StatementKind as MirStatementKind,
};

/// The scope the entry copies of a body's written parameters are bound in.
#[must_use]
pub(crate) struct WrittenParameterCopies {
    is_scoped: bool,
}

impl WrittenParameterCopies {
    /// Ends the scope of the copies once the body is lowered, releasing them
    /// when the body falls off its end; a `return` released them already.
    pub(crate) fn close(self, ctx: &mut LoweringContext, span: Span) {
        if self.is_scoped {
            ctx.pop_scope(span);
        }
    }
}

/// Binds, at the body's entry, a copy of each of `params` that `body` writes,
/// so every use of the parameter's name inside the body reads the copy.
pub(crate) fn copy_written_parameters(
    ctx: &mut LoweringContext,
    params: &[Parameter],
    body: Option<&Statement>,
) -> WrittenParameterCopies {
    let written = written_parameters(ctx, params, body);
    if written.is_empty() {
        return WrittenParameterCopies { is_scoped: false };
    }
    ctx.push_scope();
    for (param, local) in written {
        bind_parameter_copy(ctx, param, local);
    }
    WrittenParameterCopies { is_scoped: true }
}

/// Each of `params` that `body` writes and that holds a value its caller
/// still holds, with its local.
fn written_parameters<'p>(
    ctx: &LoweringContext,
    params: &'p [Parameter],
    body: Option<&Statement>,
) -> Vec<(&'p Parameter, Local)> {
    let Some(body) = body.filter(|_| !ctx.body.is_gpu()) else {
        return Vec::new();
    };
    let mut roots = WrittenRoots {
        ctx,
        names: HashSet::new(),
    };
    roots.statement(body);
    params
        .iter()
        .filter(|param| roots.names.contains(param.name.as_str()))
        .filter_map(|param| {
            let local = *ctx.variable_map.get(param.name.as_str())?;
            is_copied_parameter(ctx, local).then_some((param, local))
        })
        .collect()
}

/// Whether the parameter `local` holds a value its caller still holds and a
/// write copies: not the receiver, not `out`, not GPU-resident.
fn is_copied_parameter(ctx: &LoweringContext, local: Local) -> bool {
    let decl = &ctx.body.local_decls[local.0];
    ctx.body.is_lent_param(local)
        && decl.device_handle.is_none()
        && is_copied_on_write(&ctx.body, &decl.ty)
}

/// Binds `param`'s name to a new local holding a copy of the parameter
/// `local`, the way `var <name> = <name>` would.
///
/// The copy is a local of the body's own, numbered past `arg_count`, so the
/// rules that tell a parameter from a local by that number treat it as the
/// `var` it stands for, on purpose: moving a resource out of it clears it
/// (`variable::clear_moved_resource_source`), and calling its drop hook
/// releases it (`drop_hook_call::release_owned_local`). The caller's
/// parameter is never touched by either.
fn bind_parameter_copy(ctx: &mut LoweringContext, param: &Parameter, local: Local) {
    let span = param.typ.span;
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let copy = ctx.alloc_local(param.name.clone(), ty, span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(copy),
            Rvalue::Use(Operand::Copy(Place::new(local))),
        ),
        span,
    });
    ctx.bind_local_name(param.name.clone(), copy);
}

/// The names of the bindings a body writes, or writes through a path
/// starting at.
struct WrittenRoots<'c, 'a, 'b> {
    ctx: &'c LoweringContext<'a>,
    names: HashSet<&'b str>,
}

impl<'b> WrittenRoots<'_, '_, 'b> {
    fn statement(&mut self, statement: &'b Statement) {
        match &statement.node {
            StatementKind::Expression(expression) => self.expression(expression),
            StatementKind::Block(statements) => self.statements(statements),
            StatementKind::Variable(declarations, _) => {
                for declaration in declarations {
                    self.optional(declaration.initializer.as_deref());
                }
            }
            StatementKind::If(condition, then_branch, else_branch, _) => {
                self.expression(condition);
                self.statement(then_branch);
                if let Some(else_branch) = else_branch {
                    self.statement(else_branch);
                }
            }
            StatementKind::While(condition, body, _) => {
                self.expression(condition);
                self.statement(body);
            }
            StatementKind::For(_, iterable, body)
            | StatementKind::GpuFrame(_, iterable, body)
            | StatementKind::Forall { iterable, body, .. } => {
                self.expression(iterable);
                self.statement(body);
            }
            StatementKind::GpuFrameBlock(body) => self.statement(body),
            StatementKind::Return(value) => self.optional(value.as_deref()),
            StatementKind::FunctionDeclaration(_)
            | StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
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

    fn statements(&mut self, statements: &'b [Statement]) {
        for statement in statements {
            self.statement(statement);
        }
    }

    fn optional(&mut self, expression: Option<&'b Expression>) {
        if let Some(expression) = expression {
            self.expression(expression);
        }
    }

    fn expressions(&mut self, expressions: &'b [Expression]) {
        for expression in expressions {
            self.expression(expression);
        }
    }

    /// `expression`, recording the binding each write in it starts at. A
    /// closure's body is lowered on its own and is not read.
    fn expression(&mut self, expression: &'b Expression) {
        match &expression.node {
            ExpressionKind::Call(callee, arguments) => self.call(callee, arguments),
            ExpressionKind::Assignment(target, _, value) => {
                self.record(target.expression());
                self.pair(target.expression(), value);
            }
            ExpressionKind::Index(left, right)
            | ExpressionKind::Binary(left, _, right)
            | ExpressionKind::Logical(left, _, right) => self.pair(left, right),
            ExpressionKind::Member(operand, _)
            | ExpressionKind::StructMember(operand, _)
            | ExpressionKind::Unary(_, operand)
            | ExpressionKind::Guard(_, operand)
            | ExpressionKind::NamedArgument(_, operand)
            | ExpressionKind::Cast(operand, _) => self.expression(operand),
            ExpressionKind::Conditional(then_value, condition, else_value, _) => {
                self.pair(condition, then_value);
                self.optional(else_value.as_deref());
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start);
                self.optional(end.as_deref());
            }
            ExpressionKind::Block(statements, value) => {
                self.statements(statements);
                self.expression(value);
            }
            ExpressionKind::Match(scrutinee, branches) => self.branches(scrutinee, branches),
            ExpressionKind::List(elements)
            | ExpressionKind::Set(elements)
            | ExpressionKind::Tuple(elements)
            | ExpressionKind::EnumValue(_, elements)
            | ExpressionKind::FormattedString(elements, _) => self.expressions(elements),
            ExpressionKind::Array(elements, size) => {
                self.expressions(elements);
                self.expression(size);
            }
            ExpressionKind::Map(entries) => self.entries(entries),
            ExpressionKind::Lambda(_)
            | ExpressionKind::Identifier(..)
            | ExpressionKind::Super
            | ExpressionKind::Literal(_)
            | ExpressionKind::ImportPath(..)
            | ExpressionKind::Type(..)
            | ExpressionKind::GenericType(..)
            | ExpressionKind::TypeDeclaration(..) => {}
        }
    }

    fn pair(&mut self, first: &'b Expression, second: &'b Expression) {
        self.expression(first);
        self.expression(second);
    }

    fn branches(&mut self, scrutinee: &'b Expression, branches: &'b [MatchBranch]) {
        self.expression(scrutinee);
        for branch in branches {
            self.optional(branch.guard.as_deref());
            self.statement(&branch.body);
        }
    }

    fn entries(&mut self, entries: &'b [(Expression, Expression)]) {
        for (key, value) in entries {
            self.pair(key, value);
        }
    }

    /// A call of `callee` on `arguments`: a write into its receiver when it
    /// calls a method that writes it.
    fn call(&mut self, callee: &'b Expression, arguments: &'b [Expression]) {
        if let ExpressionKind::Member(receiver, method) = &callee.node {
            if is_receiver_written(self.ctx, receiver, method) {
                self.record(receiver);
            }
        }
        self.expression(callee);
        self.expressions(arguments);
    }

    /// Records the binding the written place `place` starts at, if a binding
    /// is where it starts.
    fn record(&mut self, place: &'b Expression) {
        if let ExpressionKind::Identifier(name, _) = &path_root(place).node {
            self.names.insert(name.as_str());
        }
    }
}
