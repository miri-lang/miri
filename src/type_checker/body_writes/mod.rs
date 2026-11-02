// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every mention of a binding in a body, with what the body does with it.
//!
//! Two questions are answered off a body before it is lowered: which methods
//! write their receiver (`receiver_writes`), and which parameters a body
//! writes, so lowering binds a copy of each on entry
//! (`mir::lowering::written_parameters`). Both are "does this body write
//! through binding X", so both read this one walk, and a new kind of write is
//! taught here once.
//!
//! The walk records each identifier, `self` and `super` it meets as a
//! [`Mention`]: whether the spot only reads it ([`Place::Read`]: indexing it,
//! reading a field of it, comparing it, formatting it, iterating over it or
//! returning it), what role the path it starts plays ([`Role`]: the target of
//! an assignment, the receiver of a method call, an argument of a call), and
//! whether it sits in a closure or a nested function the body declares. What
//! counts as a write is each reader's own rule, applied to these facts.

use std::collections::HashMap;

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};

/// How the spot a binding is mentioned at uses its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// The value is only read where it stands.
    Read,
    /// The value is used some other way: bound, handed on, cast, captured.
    Value,
}

/// Where in the body a mention sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The body itself.
    Body,
    /// A closure the body builds; it may run at any later time.
    Closure,
    /// A function the body declares inside itself.
    Nested,
}

/// The role of the path a mention starts: the mention, with every field and
/// element step taken off the path above it, is the path's root.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Role<'b> {
    /// The target of an assignment, at any depth of fields and elements.
    Assigned,
    /// The receiver `object` of a call of the method `method`.
    Receiver {
        object: &'b Expression,
        method: &'b Expression,
    },
    /// The argument written `index`th of a call of `callee`, looked at
    /// through any named-argument label and cast around it.
    Argument {
        callee: &'b Expression,
        arguments: &'b [Expression],
        index: usize,
        /// Whether the argument is the binding itself, not a path into it.
        is_bare: bool,
        /// Whether a cast stands between the argument and the binding.
        through_cast: bool,
    },
}

/// One mention of a binding.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mention<'b> {
    /// The identifier, `self` or `super` expression itself.
    pub expression: &'b Expression,
    pub place: Place,
    pub role: Option<Role<'b>>,
    pub scope: Scope,
}

impl<'b> Mention<'b> {
    /// The name the mention goes by: the identifier's, or `super`.
    pub(crate) fn name(&self) -> &'b str {
        if let ExpressionKind::Identifier(name, _) = &self.expression.node {
            name.as_str()
        } else {
            "super"
        }
    }
}

/// A function or closure a body declares inside itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Function<'b> {
    pub params: &'b [Parameter],
    pub body: &'b Statement,
}

/// Every mention of a binding in one body, in the order the walk meets them,
/// and every function and closure declared in it, at any depth.
#[derive(Debug, Default)]
pub(crate) struct BodyWrites<'b> {
    pub mentions: Vec<Mention<'b>>,
    pub functions: Vec<Function<'b>>,
}

impl<'b> BodyWrites<'b> {
    /// The mentions in the function body `body`, whose last expression, when
    /// it ends in one, is the value it returns and so only read.
    pub(crate) fn of(body: &'b Statement) -> Self {
        let mut walk = Walk {
            roles: HashMap::new(),
            scope: Scope::Body,
            mentions: Vec::new(),
            functions: Vec::new(),
        };
        walk.function_body(body);
        Self {
            mentions: walk.mentions,
            functions: walk.functions,
        }
    }

    /// The mentions of the binding `name` in the body itself, outside any
    /// closure or nested function.
    pub(crate) fn of_binding<'s>(
        &'s self,
        name: &'s str,
    ) -> impl Iterator<Item = &'s Mention<'b>> + 's {
        (self.mentions.iter())
            .filter(move |mention| mention.scope == Scope::Body && mention.name() == name)
    }
}

/// The expression a path of fields and elements starts at.
pub(crate) fn path_root(expression: &Expression) -> &Expression {
    let mut root = expression;
    while let ExpressionKind::Member(inner, _) | ExpressionKind::Index(inner, _) = &root.node {
        root = inner;
    }
    root
}

struct Walk<'b> {
    /// The role of each path root met so far, by the root's address.
    roles: HashMap<*const Expression, Role<'b>>,
    scope: Scope,
    mentions: Vec<Mention<'b>>,
    functions: Vec<Function<'b>>,
}

impl<'b> Walk<'b> {
    fn function_body(&mut self, body: &'b Statement) {
        let statements = if let StatementKind::Block(statements) = &body.node {
            statements.as_slice()
        } else {
            std::slice::from_ref(body)
        };
        if let Some((last, before)) = statements.split_last() {
            self.statements(before);
            if let StatementKind::Expression(value) = &last.node {
                self.read(value);
            } else {
                self.statement(last);
            }
        }
    }

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
                self.read(iterable);
                self.statement(body);
            }
            StatementKind::GpuFrameBlock(body) => self.statement(body),
            StatementKind::FunctionDeclaration(declaration) => {
                if let Some(body) = declaration.body.as_deref() {
                    self.functions.push(Function {
                        params: &declaration.params,
                        body,
                    });
                    self.within(Scope::Nested, |walk| walk.statement(body));
                }
            }
            StatementKind::Return(value) => {
                if let Some(value) = value.as_deref() {
                    self.read(value);
                }
            }
            StatementKind::Empty
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

    /// Runs `walk` with mentions recorded in `scope`, unless the walk is
    /// already inside a closure, which every scope within it stays.
    fn within(&mut self, scope: Scope, walk: impl FnOnce(&mut Self)) {
        let outer = self.scope;
        if outer != Scope::Closure {
            self.scope = scope;
        }
        walk(self);
        self.scope = outer;
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

    /// `expression` where its value is only read: a binding standing there
    /// is mentioned as read.
    fn read(&mut self, expression: &'b Expression) {
        let mut inner = expression;
        while let ExpressionKind::NamedArgument(_, value) = &inner.node {
            inner = value;
        }
        if is_binding(inner) {
            self.mention(inner, Place::Read);
        } else {
            self.expression(expression);
        }
    }

    fn expression(&mut self, expression: &'b Expression) {
        match &expression.node {
            ExpressionKind::Identifier(..) | ExpressionKind::Super => {
                self.mention(expression, Place::Value);
            }
            ExpressionKind::Call(callee, arguments) => self.call(callee, arguments),
            ExpressionKind::Assignment(target, _, value) => {
                let target = target.expression();
                self.assign_role(path_root(target), Role::Assigned);
                self.expression(target);
                self.expression(value);
            }
            ExpressionKind::Index(collection, index) => {
                self.read(collection);
                self.expression(index);
            }
            ExpressionKind::Member(object, _) | ExpressionKind::StructMember(object, _) => {
                self.read(object);
            }
            ExpressionKind::Binary(left, _, right) | ExpressionKind::Logical(left, _, right) => {
                self.read(left);
                self.read(right);
            }
            ExpressionKind::FormattedString(parts, _) => {
                for part in parts {
                    self.read(part);
                }
            }
            ExpressionKind::Unary(_, operand)
            | ExpressionKind::Guard(_, operand)
            | ExpressionKind::NamedArgument(_, operand)
            | ExpressionKind::Cast(operand, _) => self.expression(operand),
            ExpressionKind::Conditional(then_value, condition, else_value, _) => {
                self.expression(condition);
                self.expression(then_value);
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
            ExpressionKind::Match(scrutinee, branches) => {
                self.expression(scrutinee);
                for branch in branches {
                    self.optional(branch.guard.as_deref());
                    self.statement(&branch.body);
                }
            }
            ExpressionKind::Lambda(lambda) => {
                self.functions.push(Function {
                    params: &lambda.params,
                    body: &lambda.body,
                });
                self.within(Scope::Closure, |walk| walk.function_body(&lambda.body));
            }
            ExpressionKind::List(elements)
            | ExpressionKind::Set(elements)
            | ExpressionKind::Tuple(elements)
            | ExpressionKind::EnumValue(_, elements) => self.expressions(elements),
            ExpressionKind::Array(elements, size) => {
                self.expressions(elements);
                self.expression(size);
            }
            ExpressionKind::Map(entries) => {
                for (key, value) in entries {
                    self.expression(key);
                    self.expression(value);
                }
            }
            ExpressionKind::Literal(_)
            | ExpressionKind::ImportPath(..)
            | ExpressionKind::Type(..)
            | ExpressionKind::GenericType(..)
            | ExpressionKind::TypeDeclaration(..) => {}
        }
    }

    /// A call of `callee` on `arguments`: the receiver of a method call and
    /// each argument take their role before either is walked.
    fn call(&mut self, callee: &'b Expression, arguments: &'b [Expression]) {
        if let ExpressionKind::Member(object, method) = &callee.node {
            self.assign_role(path_root(object), Role::Receiver { object, method });
        }
        for (index, argument) in arguments.iter().enumerate() {
            let (value, through_cast) = argument_value(argument);
            let root = path_root(value);
            self.assign_role(
                root,
                Role::Argument {
                    callee,
                    arguments,
                    index,
                    is_bare: std::ptr::eq(root, value),
                    through_cast,
                },
            );
        }
        self.expression(callee);
        self.expressions(arguments);
    }

    fn assign_role(&mut self, root: &'b Expression, role: Role<'b>) {
        if is_binding(root) {
            self.roles.insert(root as *const Expression, role);
        }
    }

    fn mention(&mut self, expression: &'b Expression, place: Place) {
        let role = self.roles.get(&(expression as *const Expression)).copied();
        self.mentions.push(Mention {
            expression,
            place,
            role,
            scope: self.scope,
        });
    }
}

/// Whether `expression` names a binding: an identifier, `self` or `super`.
fn is_binding(expression: &Expression) -> bool {
    matches!(
        expression.node,
        ExpressionKind::Identifier(..) | ExpressionKind::Super
    )
}

/// The value an argument hands on, with any named-argument label and cast
/// taken off, and whether a cast was among them.
fn argument_value(argument: &Expression) -> (&Expression, bool) {
    let mut value = argument;
    let mut through_cast = false;
    loop {
        match &value.node {
            ExpressionKind::NamedArgument(_, inner) => value = inner,
            ExpressionKind::Cast(inner, _) => {
                value = inner;
                through_cast = true;
            }
            _ => return (value, through_cast),
        }
    }
}

mod parameters;
mod receiver;

pub use parameters::ParameterWrites;
pub use receiver::ReceiverWrites;

#[cfg(test)]
mod tests;
