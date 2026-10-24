// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which methods write their receiver, read off the methods' own bodies.
//!
//! A collection is a value: a second binding shares the first one's block
//! until either writes, and the write copies a block still shared. A method
//! that writes its receiver must therefore be handed a receiver only its
//! caller holds, and the call site checks it before the call. Which methods
//! write is decided here, from what their bodies do with `self`, so the
//! compiler names no method of the standard library.
//!
//! A body writes its receiver when it
//! - hands `self` to a runtime entry point at a position the runtime does
//!   not only read ([`crate::runtime_fns::only_reads_argument`]), the way
//!   every runtime entry point that mutates a collection is reached; a cast
//!   of `self` is looked through,
//! - assigns through `self`, to an element or a field of it,
//! - calls a method on `self` that writes its receiver, or one on `super`
//!   whose body in the parent chain does, or
//! - uses `self` any other way: binds it, casts it outside a runtime call,
//!   hands it to a function or a method of another value, or captures it in
//!   a closure. A runtime entry point writes the block it is handed in place,
//!   so a write through any of those reaches the receiver's block itself.
//!
//! Reading `self` writes nothing: indexing it, reading a field of it,
//! comparing it, formatting it, iterating over it, returning it, and handing
//! it to a runtime entry point at a position the runtime only reads.
//!
//! Calls on `self` are followed through every method the class has, its own
//! and those its base classes and traits supply bodies for, nearest first,
//! to a fixed point. A call of a method that has no body anywhere the class
//! can reach is taken to write, and so is any method of a class this pass
//! never analysed: a needless copy costs time, a missing one loses a write
//! or frees a block another binding still holds.

use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::statement::{Statement, StatementKind};
use std::collections::{HashMap, HashSet};

/// The name the receiver goes by inside a method body.
const RECEIVER: &str = "self";

/// The methods of every class that write their receiver, by the identity of
/// the class the receiver is an instance of and the method's name, with every
/// such pair the analysis looked at.
#[derive(Debug, Default)]
pub struct ReceiverWrites {
    writers: HashSet<(String, String)>,
    analysed: HashSet<(String, String)>,
}

impl ReceiverWrites {
    /// Whether calling `method` on an instance of the class `class` can
    /// write the receiver: it does, or it was never analysed.
    pub fn writes_receiver(&self, class: &str, method: &str) -> bool {
        let key = (class.to_string(), method.to_string());
        self.writers.contains(&key) || !self.analysed.contains(&key)
    }

    /// The receiver writers among the classes `statements` declare, at any
    /// depth of top-level blocks, each class and trait named by `identity`.
    pub(crate) fn of<'s>(
        statements: impl Iterator<Item = &'s Statement> + Clone,
        identity: impl Fn(&Expression) -> Option<String>,
    ) -> Self {
        let runtime = runtime_function_names(statements.clone());
        let declarations = Declarations::of(statements, &identity);
        let mut writes = Self::default();
        for class in &declarations.classes {
            let methods = declarations.methods_of(class);
            let writers = Analysis::of(&declarations, &methods, &runtime).writers();
            for (name, node) in &methods {
                let key = (class.clone(), name.to_string());
                if writers.contains(node) {
                    writes.writers.insert(key.clone());
                }
                writes.analysed.insert(key);
            }
        }
        writes
    }
}

/// One method body: the class or trait that declares it and its name.
type Node<'d> = (&'d str, &'d str);

/// The classes and traits a program declares, each with the methods it
/// writes a body for, and the classes and traits each extends or implements.
struct Declarations<'s> {
    classes: Vec<String>,
    bodies: HashMap<String, Vec<(&'s str, &'s Statement)>>,
    parents: HashMap<String, Vec<String>>,
}

impl<'s> Declarations<'s> {
    fn of(
        statements: impl Iterator<Item = &'s Statement>,
        identity: &impl Fn(&Expression) -> Option<String>,
    ) -> Self {
        let mut declarations = Self {
            classes: Vec::new(),
            bodies: HashMap::new(),
            parents: HashMap::new(),
        };
        for statement in statements.flat_map(top_level) {
            declarations.add(statement, identity);
        }
        declarations
    }

    fn add(&mut self, statement: &'s Statement, identity: &impl Fn(&Expression) -> Option<String>) {
        match &statement.node {
            StatementKind::Class(class) => {
                let Some(name) = identity(&class.name) else {
                    return;
                };
                let parents = class.base_class.iter().map(|base| &**base);
                let named: Vec<String> = (parents.chain(&class.traits))
                    .filter_map(|parent| type_named(parent, identity))
                    .collect();
                self.parents.insert(name.clone(), named);
                self.bodies.insert(name.clone(), bodies(&class.body));
                self.classes.push(name);
            }
            StatementKind::Trait(name, _, parents, body, _) => {
                let Some(name) = identity(name) else {
                    return;
                };
                let named = (parents.iter())
                    .filter_map(|parent| type_named(parent, identity))
                    .collect();
                self.parents.insert(name.clone(), named);
                self.bodies.insert(name, bodies(body));
            }
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Expression(_)
            | StatementKind::Block(_)
            | StatementKind::Variable(..)
            | StatementKind::If(..)
            | StatementKind::While(..)
            | StatementKind::For(..)
            | StatementKind::Forall { .. }
            | StatementKind::GpuFrame(..)
            | StatementKind::GpuFrameBlock(_)
            | StatementKind::FunctionDeclaration(_)
            | StatementKind::Return(_)
            | StatementKind::Use(..)
            | StatementKind::Type(..)
            | StatementKind::Enum(..)
            | StatementKind::Struct(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..) => {}
        }
    }

    /// The body each method an instance of `class` runs comes from, by
    /// name: its own, then those of its base classes and traits it does not
    /// declare itself, nearest first.
    fn methods_of<'d>(&'d self, class: &str) -> HashMap<&'d str, Node<'d>> {
        let mut methods = HashMap::new();
        for owner in self.lineage(class) {
            for &(name, _) in self.bodies_of(owner) {
                methods.entry(name).or_insert((owner, name));
            }
        }
        methods
    }

    /// The body of `method` the parents of `owner` supply, nearest first.
    fn inherited<'d>(&'d self, owner: &str, method: &str) -> Option<Node<'d>> {
        (self.lineage(owner).into_iter().skip(1)).find_map(|parent| {
            let &(name, _) = (self.bodies_of(parent).iter()).find(|&&(name, _)| name == method)?;
            Some((parent, name))
        })
    }

    /// The body `node` names.
    fn body(&self, (owner, method): Node<'_>) -> Option<&'s Statement> {
        (self.bodies_of(owner).iter())
            .find(|&&(name, _)| name == method)
            .map(|&(_, body)| body)
    }

    fn bodies_of(&self, owner: &str) -> &[(&'s str, &'s Statement)] {
        self.bodies.get(owner).map_or(&[], Vec::as_slice)
    }

    /// `name` and every class and trait it extends or implements that this
    /// program declares, nearest first, each once.
    fn lineage<'d>(&'d self, name: &str) -> Vec<&'d str> {
        let mut lineage: Vec<&'d str> = (self.parents.get_key_value(name))
            .map(|(name, _)| name.as_str())
            .into_iter()
            .collect();
        let mut next = 0;
        while let Some(&current) = lineage.get(next) {
            next += 1;
            for parent in self.parents.get(current).into_iter().flatten() {
                let Some((parent, _)) = self.parents.get_key_value(parent) else {
                    continue;
                };
                if !lineage.contains(&parent.as_str()) {
                    lineage.push(parent.as_str());
                }
            }
        }
        lineage
    }
}

/// What every body an instance of one class can run does with its
/// receiver, keyed by body.
struct Analysis<'s, 'd> {
    declarations: &'d Declarations<'s>,
    methods: &'d HashMap<&'d str, Node<'d>>,
    facts: HashMap<Node<'d>, BodyFacts<'s>>,
}

impl<'s, 'd> Analysis<'s, 'd> {
    /// The facts of the bodies `methods` name and of every parent body a
    /// call on `super` in one of them reaches.
    fn of(
        declarations: &'d Declarations<'s>,
        methods: &'d HashMap<&'d str, Node<'d>>,
        runtime: &HashSet<&str>,
    ) -> Self {
        let mut facts = HashMap::new();
        let mut pending: Vec<Node<'d>> = methods.values().copied().collect();
        while let Some(node) = pending.pop() {
            if facts.contains_key(&node) {
                continue;
            }
            let Some(body) = declarations.body(node) else {
                continue;
            };
            let fact = BodyFacts::of(body, runtime);
            pending.extend(
                (fact.calls_on_super.iter())
                    .filter_map(|method| declarations.inherited(node.0, method)),
            );
            facts.insert(node, fact);
        }
        Self {
            declarations,
            methods,
            facts,
        }
    }

    /// The bodies that write their receiver: those that do themselves, then,
    /// round by round, those calling one.
    fn writers(&self) -> HashSet<Node<'d>> {
        let mut writers: HashSet<Node<'d>> = (self.facts.iter())
            .filter(|(_, fact)| fact.writes)
            .map(|(&node, _)| node)
            .collect();
        loop {
            let found: Vec<Node<'d>> = (self.facts.iter())
                .filter(|(node, _)| !writers.contains(*node))
                .filter(|(&node, fact)| self.calls_a_writer(node, fact, &writers))
                .map(|(&node, _)| node)
                .collect();
            if found.is_empty() {
                return writers;
            }
            writers.extend(found);
        }
    }

    /// Whether the body `node`, doing `fact`, calls a method among `writers`
    /// or one no body is found for.
    fn calls_a_writer(
        &self,
        node: Node<'d>,
        fact: &BodyFacts<'s>,
        writers: &HashSet<Node<'d>>,
    ) -> bool {
        let writes =
            |callee: Option<Node<'d>>| callee.is_none_or(|callee| writers.contains(&callee));
        (fact.calls_on_self.iter()).any(|method| writes(self.methods.get(method).copied()))
            || (fact.calls_on_super.iter())
                .any(|method| writes(self.declarations.inherited(node.0, method)))
    }
}

/// The identity of the class or trait the reference `expression` names,
/// read through the type arguments it is written with (`Queryable<T>`).
fn type_named(
    expression: &Expression,
    identity: &impl Fn(&Expression) -> Option<String>,
) -> Option<String> {
    identity(expression).or_else(|| {
        let (ExpressionKind::TypeDeclaration(named, ..) | ExpressionKind::GenericType(named, ..)) =
            &expression.node
        else {
            return None;
        };
        type_named(named, identity)
    })
}

/// `statement`, or the statements of the top-level block it is.
fn top_level(statement: &Statement) -> Vec<&Statement> {
    if let StatementKind::Block(statements) = &statement.node {
        statements.iter().collect()
    } else {
        vec![statement]
    }
}

/// The methods `body` declares with a body of their own, by name.
fn bodies(body: &[Statement]) -> Vec<(&str, &Statement)> {
    body.iter()
        .filter_map(|statement| {
            let StatementKind::FunctionDeclaration(declaration) = &statement.node else {
                return None;
            };
            let body = declaration.body.as_deref()?;
            Some((declaration.name.as_str(), body))
        })
        .collect()
}

/// The name of every runtime entry point `statements` declare, at any depth
/// of the classes, traits and blocks that declare them.
fn runtime_function_names<'s>(statements: impl Iterator<Item = &'s Statement>) -> HashSet<&'s str> {
    let mut names = HashSet::new();
    let mut pending: Vec<&Statement> = statements.collect();
    while let Some(statement) = pending.pop() {
        match &statement.node {
            StatementKind::RuntimeFunctionDeclaration(_, name, _, _) => {
                names.insert(name.as_str());
            }
            StatementKind::Block(body) | StatementKind::Trait(_, _, _, body, _) => {
                pending.extend(body);
            }
            StatementKind::Class(class) => pending.extend(&class.body),
            StatementKind::Struct(_, _, _, body, _, _)
            | StatementKind::Enum(_, _, _, body, _, _) => {
                pending.extend(body);
            }
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Expression(_)
            | StatementKind::Variable(..)
            | StatementKind::If(..)
            | StatementKind::While(..)
            | StatementKind::For(..)
            | StatementKind::Forall { .. }
            | StatementKind::GpuFrame(..)
            | StatementKind::GpuFrameBlock(_)
            | StatementKind::FunctionDeclaration(_)
            | StatementKind::Return(_)
            | StatementKind::Use(..)
            | StatementKind::Type(..)
            | StatementKind::IntrinsicFunctionDeclaration(..) => {}
        }
    }
    names
}

/// What one method body does with `self`.
#[derive(Debug, Default)]
struct BodyFacts<'b> {
    /// Whether the body writes the receiver itself.
    writes: bool,
    /// Whether the body names the receiver anywhere.
    mentions_receiver: bool,
    /// The methods the body calls on `self`.
    calls_on_self: Vec<&'b str>,
    /// The methods the body calls on `super`.
    calls_on_super: Vec<&'b str>,
}

impl<'b> BodyFacts<'b> {
    /// The facts of the function body `body`, whose last expression, when it
    /// ends in one, is the value it returns.
    fn of(body: &'b Statement, runtime: &HashSet<&str>) -> Self {
        let mut facts = Self::default();
        let statements = if let StatementKind::Block(statements) = &body.node {
            statements.as_slice()
        } else {
            std::slice::from_ref(body)
        };
        if let Some((last, before)) = statements.split_last() {
            facts.statements(before, runtime);
            if let StatementKind::Expression(value) = &last.node {
                facts.read(value, runtime);
            } else {
                facts.statement(last, runtime);
            }
        }
        facts
    }

    fn statement(&mut self, statement: &'b Statement, runtime: &HashSet<&str>) {
        match &statement.node {
            StatementKind::Expression(expression) => self.expression(expression, runtime),
            StatementKind::Block(statements) => self.statements(statements, runtime),
            StatementKind::Variable(declarations, _) => {
                for declaration in declarations {
                    self.optional(declaration.initializer.as_deref(), runtime);
                }
            }
            StatementKind::If(condition, then_branch, else_branch, _) => {
                self.expression(condition, runtime);
                self.statement(then_branch, runtime);
                if let Some(else_branch) = else_branch {
                    self.statement(else_branch, runtime);
                }
            }
            StatementKind::While(condition, body, _) => {
                self.expression(condition, runtime);
                self.statement(body, runtime);
            }
            StatementKind::For(_, iterable, body)
            | StatementKind::GpuFrame(_, iterable, body)
            | StatementKind::Forall { iterable, body, .. } => {
                self.read(iterable, runtime);
                self.statement(body, runtime);
            }
            StatementKind::GpuFrameBlock(body) => self.statement(body, runtime),
            StatementKind::FunctionDeclaration(declaration) => {
                if let Some(body) = declaration.body.as_deref() {
                    self.statement(body, runtime);
                }
            }
            StatementKind::Return(value) => {
                if let Some(value) = value.as_deref() {
                    self.read(value, runtime);
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

    fn statements(&mut self, statements: &'b [Statement], runtime: &HashSet<&str>) {
        for statement in statements {
            self.statement(statement, runtime);
        }
    }

    fn optional(&mut self, expression: Option<&'b Expression>, runtime: &HashSet<&str>) {
        if let Some(expression) = expression {
            self.expression(expression, runtime);
        }
    }

    fn expressions(&mut self, expressions: &'b [Expression], runtime: &HashSet<&str>) {
        for expression in expressions {
            self.expression(expression, runtime);
        }
    }

    /// `expression` where it is only read: the receiver itself reads nothing
    /// into a write there.
    fn read(&mut self, expression: &'b Expression, runtime: &HashSet<&str>) {
        if is_receiver(expression) {
            self.mentions_receiver = true;
        } else {
            self.expression(expression, runtime);
        }
    }

    /// `expression` where the receiver, met anywhere the walk does not stop
    /// at a read, a runtime argument or a method call first, is written.
    fn expression(&mut self, expression: &'b Expression, runtime: &HashSet<&str>) {
        match &expression.node {
            ExpressionKind::Identifier(..) | ExpressionKind::Super => {
                if is_receiver(expression) {
                    self.mentions_receiver = true;
                    self.writes = true;
                }
            }
            ExpressionKind::Call(callee, arguments) => self.call(callee, arguments, runtime),
            ExpressionKind::Assignment(target, _, value) => {
                if writes_through_receiver(target) {
                    self.writes = true;
                }
                self.expression(target.expression(), runtime);
                self.expression(value, runtime);
            }
            ExpressionKind::Index(collection, index) => {
                self.read(collection, runtime);
                self.expression(index, runtime);
            }
            ExpressionKind::Member(object, _) | ExpressionKind::StructMember(object, _) => {
                self.read(object, runtime);
            }
            ExpressionKind::Binary(left, _, right) | ExpressionKind::Logical(left, _, right) => {
                self.read(left, runtime);
                self.read(right, runtime);
            }
            ExpressionKind::FormattedString(parts, _) => {
                for part in parts {
                    self.read(part, runtime);
                }
            }
            ExpressionKind::Unary(_, operand)
            | ExpressionKind::Guard(_, operand)
            | ExpressionKind::NamedArgument(_, operand)
            | ExpressionKind::Cast(operand, _) => self.expression(operand, runtime),
            ExpressionKind::Conditional(then_value, condition, else_value, _) => {
                self.expression(condition, runtime);
                self.expression(then_value, runtime);
                self.optional(else_value.as_deref(), runtime);
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start, runtime);
                self.optional(end.as_deref(), runtime);
            }
            ExpressionKind::Block(statements, value) => {
                self.statements(statements, runtime);
                self.expression(value, runtime);
            }
            ExpressionKind::Match(scrutinee, branches) => {
                self.expression(scrutinee, runtime);
                for branch in branches {
                    self.optional(branch.guard.as_deref(), runtime);
                    self.statement(&branch.body, runtime);
                }
            }
            ExpressionKind::Lambda(lambda) => self.lambda(&lambda.body, runtime),
            ExpressionKind::List(elements)
            | ExpressionKind::Set(elements)
            | ExpressionKind::Tuple(elements)
            | ExpressionKind::EnumValue(_, elements) => self.expressions(elements, runtime),
            ExpressionKind::Array(elements, size) => {
                self.expressions(elements, runtime);
                self.expression(size, runtime);
            }
            ExpressionKind::Map(entries) => self.entries(entries, runtime),
            ExpressionKind::Literal(_)
            | ExpressionKind::ImportPath(..)
            | ExpressionKind::Type(..)
            | ExpressionKind::GenericType(..)
            | ExpressionKind::TypeDeclaration(..) => {}
        }
    }

    fn entries(&mut self, entries: &'b [(Expression, Expression)], runtime: &HashSet<&str>) {
        for (key, value) in entries {
            self.expression(key, runtime);
            self.expression(value, runtime);
        }
    }

    /// A closure with `body`: one that captures the receiver holds it past
    /// any one call and may write it whenever it runs, so it writes.
    fn lambda(&mut self, body: &'b Statement, runtime: &HashSet<&str>) {
        let inner = Self::of(body, runtime);
        if inner.mentions_receiver {
            self.mentions_receiver = true;
            self.writes = true;
        }
    }

    /// A call of `callee` on `arguments`: a write when it hands the receiver
    /// to a runtime entry point that does not only read it, a call to follow
    /// when it is a method called on the receiver, and otherwise a use of
    /// whatever it is handed.
    fn call(
        &mut self,
        callee: &'b Expression,
        arguments: &'b [Expression],
        runtime: &HashSet<&str>,
    ) {
        if let Some(name) = runtime_callee(callee, runtime) {
            for (position, argument) in arguments.iter().enumerate() {
                if is_receiver_argument(argument) {
                    self.mentions_receiver = true;
                    self.writes |= !crate::runtime_fns::only_reads_argument(name, position);
                } else {
                    self.expression(argument, runtime);
                }
            }
            return;
        }
        match method_called_on_receiver(callee) {
            Some((method, ReceiverWord::Itself)) => self.calls_on_self.push(method),
            Some((method, ReceiverWord::Super)) => self.calls_on_super.push(method),
            None => self.expression(callee, runtime),
        }
        if method_called_on_receiver(callee).is_some() {
            self.mentions_receiver = true;
        }
        self.expressions(arguments, runtime);
    }
}

/// How a method body names its receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverWord {
    /// `self`, whose methods dispatch as the instance's class has them.
    Itself,
    /// `super`, whose methods are the parent chain's.
    Super,
}

/// How `expression` names the receiver, if it is the receiver itself.
fn receiver_word(expression: &Expression) -> Option<ReceiverWord> {
    if let ExpressionKind::NamedArgument(_, value) = &expression.node {
        return receiver_word(value);
    }
    if matches!(expression.node, ExpressionKind::Super) {
        return Some(ReceiverWord::Super);
    }
    let ExpressionKind::Identifier(name, None) = &expression.node else {
        return None;
    };
    (name == RECEIVER).then_some(ReceiverWord::Itself)
}

/// Whether `expression` is the receiver itself.
fn is_receiver(expression: &Expression) -> bool {
    receiver_word(expression).is_some()
}

/// Whether the runtime argument `expression` is the receiver, looked at
/// through a cast, which hands a runtime entry point the same block.
fn is_receiver_argument(expression: &Expression) -> bool {
    if let ExpressionKind::Cast(value, _) = &expression.node {
        return is_receiver_argument(value);
    }
    if let ExpressionKind::NamedArgument(_, value) = &expression.node {
        return is_receiver_argument(value);
    }
    is_receiver(expression)
}

/// The runtime entry point `callee` names, when it names one of `runtime`.
fn runtime_callee<'c>(callee: &'c Expression, runtime: &HashSet<&str>) -> Option<&'c str> {
    let ExpressionKind::Identifier(name, None) = &callee.node else {
        return None;
    };
    runtime.contains(name.as_str()).then_some(name.as_str())
}

/// The method `callee` calls on the receiver, when it is one, with the
/// word the receiver is named by.
fn method_called_on_receiver(callee: &Expression) -> Option<(&str, ReceiverWord)> {
    let ExpressionKind::Member(object, method) = &callee.node else {
        return None;
    };
    let ExpressionKind::Identifier(method, _) = &method.node else {
        return None;
    };
    receiver_word(object).map(|word| (method.as_str(), word))
}

/// Whether assigning to `target` writes through the receiver: to an element
/// or a field of `self`, at any depth.
fn writes_through_receiver(target: &LeftHandSideExpression) -> bool {
    match target {
        LeftHandSideExpression::Identifier(_) => false,
        LeftHandSideExpression::Member(place) | LeftHandSideExpression::Index(place) => {
            reaches_receiver(place)
        }
    }
}

/// Whether the place `expression` names lies inside the receiver.
fn reaches_receiver(expression: &Expression) -> bool {
    let (ExpressionKind::Member(base, _) | ExpressionKind::Index(base, _)) = &expression.node
    else {
        return false;
    };
    is_receiver(base) || reaches_receiver(base)
}

#[cfg(test)]
mod tests;
