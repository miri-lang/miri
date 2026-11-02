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
//! - hands `self` to a runtime entry point at a position its declaration
//!   does not state it only reads ([`RuntimeEffect::only_reads`]), the way
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

use crate::runtime_effects::{RuntimeEffect, RuntimeEffects};

use super::{BodyWrites, Mention, Place, Role, Scope};
use crate::ast::expression::{Expression, ExpressionKind};
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
        statements: impl Iterator<Item = &'s Statement>,
        identity: impl Fn(&Expression) -> Option<String>,
        runtime: &RuntimeEffects,
    ) -> Self {
        let declarations = Declarations::of(statements, &identity);
        let mut writes = Self::default();
        for class in &declarations.classes {
            let methods = declarations.methods_of(class);
            let writers = Analysis::of(&declarations, &methods, runtime).writers();
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
        runtime: &RuntimeEffects,
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
    /// The facts of the function body `body`, read off its mentions of the
    /// receiver ([`BodyWrites`]).
    fn of(body: &'b Statement, runtime: &RuntimeEffects) -> Self {
        let mut facts = Self::default();
        for mention in &BodyWrites::of(body).mentions {
            if let Some(word) = receiver_word(mention.expression) {
                facts.receiver_mention(mention, word, runtime);
            }
        }
        facts
    }

    /// A mention of the receiver: a write when a closure holds it, when it
    /// starts an assignment's target, when it is handed to a runtime entry
    /// point that does not only read it, or when it stands anywhere but a
    /// read; a call to follow when a method is called on it.
    fn receiver_mention(
        &mut self,
        mention: &Mention<'b>,
        word: ReceiverWord,
        runtime: &RuntimeEffects,
    ) {
        self.mentions_receiver = true;
        if mention.scope == Scope::Closure {
            self.writes = true;
            return;
        }
        match mention.role {
            Some(Role::Assigned) => self.writes = true,
            Some(Role::Receiver { object, method }) if std::ptr::eq(object, mention.expression) => {
                if let ExpressionKind::Identifier(method, _) = &method.node {
                    match word {
                        ReceiverWord::Itself => self.calls_on_self.push(method.as_str()),
                        ReceiverWord::Super => self.calls_on_super.push(method.as_str()),
                    }
                }
            }
            Some(Role::Argument {
                callee,
                index,
                is_bare,
                ..
            }) => match runtime_callee(callee, runtime).filter(|_| is_bare) {
                Some(effect) => self.writes |= !effect.only_reads(index),
                None => self.writes |= mention.place == Place::Value,
            },
            Some(Role::Receiver { .. }) | None => {
                self.writes |= mention.place == Place::Value;
            }
        }
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

/// What the runtime entry point `callee` names does, when it names one.
fn runtime_callee<'r>(
    callee: &Expression,
    runtime: &'r RuntimeEffects,
) -> Option<&'r RuntimeEffect> {
    let ExpressionKind::Identifier(name, None) = &callee.node else {
        return None;
    };
    runtime.get(name.as_str()).map(|effect| &**effect)
}

#[cfg(test)]
mod tests;
