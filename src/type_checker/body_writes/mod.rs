// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every mention of a binding in a body, with what the body does with it.
//!
//! Three questions are answered off a body before it is lowered: which
//! methods write their receiver (`receiver_writes`), which parameters a body
//! writes, so lowering binds a copy of each on entry
//! (`mir::lowering::written_parameters`), and which operands a later operand
//! may write, so lowering reads each on the spot (`mir::lowering::sequencing`).
//! All are "does this code write through binding X", so all read this one
//! walk, and a new kind of write is taught here once.
//!
//! The walk records each identifier, `self` and `super` it meets as a
//! [`Mention`]: whether the spot only reads it ([`Place::Read`]: indexing it,
//! reading a field of it, comparing it, formatting it, iterating over it or
//! returning it), what role the path it starts plays ([`Role`]: the target of
//! an assignment, the receiver of a method call, an argument of a call), and
//! whether it sits in a closure or a nested function the body declares. What
//! counts as a write is each reader's own rule, applied to these facts.
//!
//! The walk also records every expression that evaluates a list of operands
//! one after another ([`BodyWrites::sequences`]), every call and operator
//! ([`BodyWrites::effects`]), and where each expression's own records lie
//! ([`BodyWrites::extents`]), which is what [`HeldOperands`] reads to decide
//! which operand a later one may write.

use std::collections::HashMap;
use std::ops::Range;

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
    /// The id of the body the mention runs in: the function or closure body
    /// innermost around it, [`OUTSIDE_ANY_BODY`] outside every one.
    pub owner: usize,
}

impl<'b> Mention<'b> {
    /// The name the mention goes by: the identifier's, or `super`.
    pub(crate) fn name(&self) -> &'b str {
        binding_name(self.expression).unwrap_or("super")
    }
}

/// The owner of what a walk meets outside every function and closure body.
pub(crate) const OUTSIDE_ANY_BODY: usize = 0;

/// An expression the walk records, with the id of the body it runs in, as a
/// [`Mention::owner`] is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Recorded<'b> {
    pub expression: &'b Expression,
    pub owner: usize,
}

/// Where the records the walk made inside one expression lie: the walk
/// records an expression's parts one after another, so they are a run of
/// positions in [`BodyWrites::mentions`] and one in [`BodyWrites::effects`].
#[derive(Debug, Clone)]
pub(crate) struct Extent {
    pub mentions: Range<usize>,
    pub effects: Range<usize>,
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
    /// Every expression that evaluates operands in sequence — a call, an
    /// operator, an index, a range, an array, tuple or variant literal — at
    /// any depth, closures and nested functions included, but no step of
    /// the path an assignment writes.
    pub sequences: Vec<Recorded<'b>>,
    /// Every call and binary operator, in the order the walk meets them.
    pub effects: Vec<Recorded<'b>>,
    /// Where each walked expression's own records lie, by its address.
    pub extents: HashMap<*const Expression, Extent>,
}

impl<'b> BodyWrites<'b> {
    /// The mentions in the function body `body`, whose last expression, when
    /// it ends in one, is the value it returns and so only read.
    pub(crate) fn of(body: &'b Statement) -> Self {
        let mut walk = Walk::default();
        walk.function_body(body);
        walk.finish()
    }
}

/// The parameters every function and closure `statements` declare writes,
/// and the operands of every body lowering holds, at any depth: each
/// statement of the program is walked once, and both read the one walk.
pub(crate) fn analyse<'s>(
    statements: impl Iterator<Item = &'s Statement>,
    checker: &crate::type_checker::TypeChecker,
) -> (ParameterWrites, HeldOperands) {
    let mut parameters = ParameterWrites::default();
    let mut held = HeldOperands::default();
    for statement in statements {
        for_each_member(statement, &mut |member| {
            let writes = BodyWrites::of(member);
            parameters.record(&writes, checker);
            held.record(&writes, checker);
        });
    }
    (parameters, held)
}

/// Calls `visit` on every statement of the program `statement` stands for:
/// a block's statements and a type declaration's members in its place, at
/// any depth, and any other statement itself.
pub(crate) fn for_each_member<'s>(statement: &'s Statement, visit: &mut impl FnMut(&'s Statement)) {
    let members: &'s [Statement] = match &statement.node {
        StatementKind::Block(statements) => statements,
        StatementKind::Class(class) => &class.body,
        StatementKind::Struct(_, _, _, methods, _, _)
        | StatementKind::Enum(_, _, _, methods, _, _)
        | StatementKind::Trait(_, _, _, methods, _) => methods,
        StatementKind::FunctionDeclaration(_)
        | StatementKind::Expression(_)
        | StatementKind::Variable(..)
        | StatementKind::If(..)
        | StatementKind::While(..)
        | StatementKind::For(..)
        | StatementKind::Forall { .. }
        | StatementKind::GpuFrame(..)
        | StatementKind::GpuFrameBlock(_)
        | StatementKind::Return(_)
        | StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(..)
        | StatementKind::Type(..)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => {
            visit(statement);
            return;
        }
    };
    for member in members {
        for_each_member(member, visit);
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
    /// The id of the function or closure body being walked.
    owner: usize,
    mentions: Vec<Mention<'b>>,
    functions: Vec<Function<'b>>,
    sequences: Vec<Recorded<'b>>,
    effects: Vec<Recorded<'b>>,
    extents: HashMap<*const Expression, Extent>,
}

impl Default for Walk<'_> {
    fn default() -> Self {
        Self {
            roles: HashMap::new(),
            scope: Scope::Body,
            owner: OUTSIDE_ANY_BODY,
            mentions: Vec::new(),
            functions: Vec::new(),
            sequences: Vec::new(),
            effects: Vec::new(),
            extents: HashMap::new(),
        }
    }
}

impl<'b> Walk<'b> {
    fn finish(self) -> BodyWrites<'b> {
        BodyWrites {
            mentions: self.mentions,
            functions: self.functions,
            sequences: self.sequences,
            effects: self.effects,
            extents: self.extents,
        }
    }

    /// Runs `walk` over `expression`, recording where the records it makes
    /// lie.
    fn extent(&mut self, expression: &'b Expression, walk: impl FnOnce(&mut Self)) {
        let (mentions, effects) = (self.mentions.len(), self.effects.len());
        walk(self);
        let extent = Extent {
            mentions: mentions..self.mentions.len(),
            effects: effects..self.effects.len(),
        };
        self.extents.insert(expression as *const Expression, extent);
    }

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
                    self.within(Scope::Nested, body, |walk| walk.statement(body));
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

    /// Runs `walk` over the function or closure body `body`, with mentions
    /// recorded in `scope`, unless the walk is already inside a closure,
    /// which every scope within it stays.
    fn within(&mut self, scope: Scope, body: &Statement, walk: impl FnOnce(&mut Self)) {
        let (outer, outer_owner) = (self.scope, self.owner);
        if outer != Scope::Closure {
            self.scope = scope;
        }
        self.owner = body.id;
        walk(self);
        (self.scope, self.owner) = (outer, outer_owner);
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
        let inner = unlabelled(expression);
        if binding_name(inner).is_some() {
            self.extent(inner, |walk| walk.mention(inner, Place::Read));
        } else {
            self.expression(expression);
        }
    }

    fn expression(&mut self, expression: &'b Expression) {
        self.extent(expression, |walk| {
            walk.record_sequence(expression);
            walk.parts(expression);
        });
    }

    /// The expressions `expression` is built from, walked without recording
    /// `expression` itself.
    fn parts(&mut self, expression: &'b Expression) {
        match &expression.node {
            ExpressionKind::Identifier(..) | ExpressionKind::Super => {
                self.mention(expression, Place::Value);
            }
            ExpressionKind::Call(callee, arguments) => self.call(callee, arguments),
            ExpressionKind::Assignment(target, _, value) => {
                let target = target.expression();
                self.assign_role(path_root(target), Role::Assigned);
                self.write_target(target);
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
                self.within(Scope::Closure, &lambda.body, |walk| {
                    walk.function_body(&lambda.body)
                });
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

    /// The path an assignment writes. The assignment evaluates the path
    /// itself, in its own order, so no step along it is a sequence of its
    /// own; an index along it is an expression like any other.
    fn write_target(&mut self, target: &'b Expression) {
        if let ExpressionKind::Member(object, _) = &target.node {
            self.write_target_step(object);
        } else if let ExpressionKind::Index(collection, index) = &target.node {
            self.write_target_step(collection);
            self.expression(index);
        } else {
            self.parts(target);
        }
    }

    /// The value a step of a written path steps into.
    fn write_target_step(&mut self, object: &'b Expression) {
        if matches!(
            object.node,
            ExpressionKind::Member(..) | ExpressionKind::Index(..)
        ) {
            self.write_target(object);
        } else {
            self.read(object);
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

    /// Records `expression` when it evaluates operands in sequence, and as
    /// an effect when it is a call or an operator.
    fn record_sequence(&mut self, expression: &'b Expression) {
        let is_effect = matches!(
            expression.node,
            ExpressionKind::Call(..) | ExpressionKind::Binary(..)
        );
        let recorded = Recorded {
            expression,
            owner: self.owner,
        };
        if is_effect {
            self.effects.push(recorded);
        }
        if is_sequence(expression) {
            self.sequences.push(recorded);
        }
    }

    fn assign_role(&mut self, root: &'b Expression, role: Role<'b>) {
        if binding_name(root).is_some() {
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
            owner: self.owner,
        });
    }
}

/// Whether `expression` evaluates operands one after another.
fn is_sequence(expression: &Expression) -> bool {
    !held_operands::sequence_slots(expression).is_empty()
}

/// The name of the binding `expression` is: an identifier's, `self`'s, or
/// `super`.
pub(crate) fn binding_name(expression: &Expression) -> Option<&str> {
    if let ExpressionKind::Identifier(name, _) = &expression.node {
        return Some(name);
    }
    matches!(expression.node, ExpressionKind::Super).then_some("super")
}

/// `expression` with any named-argument label taken off.
pub(crate) fn unlabelled(expression: &Expression) -> &Expression {
    let mut value = expression;
    while let ExpressionKind::NamedArgument(_, inner) = &value.node {
        value = inner;
    }
    value
}

/// The value an argument hands on, with any named-argument label and cast
/// taken off, and whether a cast was among them.
pub(crate) fn argument_value(argument: &Expression) -> (&Expression, bool) {
    let mut value = unlabelled(argument);
    let mut through_cast = false;
    while let ExpressionKind::Cast(inner, _) = &value.node {
        value = unlabelled(inner);
        through_cast = true;
    }
    (value, through_cast)
}

mod held_operands;
mod parameters;
mod receiver;

pub use held_operands::{HeldOperand, HeldOperands};
pub use parameters::ParameterWrites;
pub use receiver::ReceiverWrites;

#[cfg(test)]
mod tests;
