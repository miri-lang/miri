// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which parameters a body writes, read off the body's mentions of them.
//!
//! A collection, a struct or a tuple passed to a function is a value: a write
//! to it inside the function is not seen by the caller, so lowering binds a
//! copy of each parameter the body writes on entry
//! (`mir::lowering::written_parameters`). It reads the verdict from here, made
//! once every body is checked and before any is lowered, so a read lowered
//! before the write, a loop header, reads the copy as well.
//!
//! A body writes a parameter when a mention of it in the body itself, outside
//! any closure or nested function, is
//! - the target of an assignment, at any depth of fields and elements,
//! - the receiver of a call of a method that writes a built-in collection
//!   receiver, as [`super::ReceiverWrites`] reads it off the method's body, or
//! - an argument a runtime entry point writes in place, as its declaration
//!   states. A cast hands the runtime a value of its own and is not followed.
//!
//! Every function, method, nested function and closure is analysed, each
//! against its own body. A parameter is known by the id of its type
//! expression, which lowering reads off the same declaration.
//! Over-approximating costs one copy; missing a write is caught where the
//! write is lowered (`expression::unshare`).

use std::collections::HashSet;

use super::{BodyWrites, Function, Mention, Role};
use crate::ast::common::Parameter;
use crate::ast::expression::{bind_arguments_to_parameters, Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::TypeKind;
use crate::type_checker::{CalleeKind, TypeChecker};

/// The parameters every body of a program writes.
#[derive(Debug, Default)]
pub struct ParameterWrites {
    /// The id of each written parameter's type expression.
    written: HashSet<usize>,
}

impl ParameterWrites {
    /// Whether the body that declares `parameter` writes it.
    pub fn writes(&self, parameter: &Parameter) -> bool {
        self.written.contains(&parameter.typ.id)
    }

    /// The written parameters of every function, method and closure
    /// `statements` declare, at any depth.
    pub(crate) fn of<'s>(
        statements: impl Iterator<Item = &'s Statement>,
        checker: &TypeChecker,
    ) -> Self {
        let mut writes = Self::default();
        for statement in statements {
            writes.declarations(statement, checker);
        }
        writes
    }

    /// Analyses the functions `statement` declares: a function or method
    /// with the functions inside it, and the closures a statement outside
    /// any function builds, a field initializer's included.
    fn declarations(&mut self, statement: &Statement, checker: &TypeChecker) {
        match &statement.node {
            StatementKind::FunctionDeclaration(declaration) => {
                if let Some(body) = declaration.body.as_deref() {
                    let function = Function {
                        params: &declaration.params,
                        body,
                    };
                    for inner in self.function(function, checker) {
                        self.function(inner, checker);
                    }
                }
            }
            StatementKind::Block(statements) => {
                statements
                    .iter()
                    .for_each(|s| self.declarations(s, checker));
            }
            StatementKind::Class(class) => {
                class
                    .body
                    .iter()
                    .for_each(|s| self.declarations(s, checker));
            }
            StatementKind::Struct(_, _, _, methods, _, _)
            | StatementKind::Enum(_, _, _, methods, _, _)
            | StatementKind::Trait(_, _, _, methods, _) => {
                methods.iter().for_each(|s| self.declarations(s, checker));
            }
            StatementKind::Expression(_)
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
                for inner in BodyWrites::of(statement).functions {
                    self.function(inner, checker);
                }
            }
        }
    }

    /// Records the parameters `function` writes, and hands back every
    /// function and closure it declares, at any depth.
    fn function<'b>(&mut self, function: Function<'b>, checker: &TypeChecker) -> Vec<Function<'b>> {
        let uses = BodyWrites::of(function.body);
        for param in function.params {
            if uses.of_binding(&param.name).any(|m| is_written(checker, m)) {
                self.written.insert(param.typ.id);
            }
        }
        uses.functions
    }
}

/// Whether `mention` of a binding writes it, or writes through a path
/// starting at it.
fn is_written(checker: &TypeChecker, mention: &Mention) -> bool {
    match mention.role {
        Some(Role::Assigned) => true,
        Some(Role::Receiver { object, method }) => writes_receiver(checker, object, method),
        Some(Role::Argument {
            callee,
            arguments,
            index,
            through_cast,
            ..
        }) => !through_cast && runtime_writes_argument(checker, callee, arguments, index),
        None => false,
    }
}

/// Whether calling `method` on `object` writes the built-in collection
/// `object` is.
fn writes_receiver(checker: &TypeChecker, object: &Expression, method: &Expression) -> bool {
    let ExpressionKind::Identifier(method, _) = &method.node else {
        return false;
    };
    let Some(ty) = checker.get_type(object.id) else {
        return false;
    };
    let Some(collection) = ty.kind.as_builtin_collection() else {
        return false;
    };
    let class = match &ty.kind {
        TypeKind::Custom(name, _) => name.as_str(),
        _ => collection.name(),
    };
    checker.receiver_writes.writes_receiver(class, method)
}

/// Whether the runtime entry point `callee` names, called on `arguments`,
/// writes the one written `index`th in place, as its declaration states;
/// false when `callee` names no runtime entry point.
///
/// Only a plain name is read: a runtime declaration is never public, so a
/// body never reaches one through a module alias.
fn runtime_writes_argument(
    checker: &TypeChecker,
    callee: &Expression,
    arguments: &[Expression],
    index: usize,
) -> bool {
    let ExpressionKind::Identifier(name, _) = &callee.node else {
        return false;
    };
    if checker.callee_kind(callee.id) != CalleeKind::Runtime {
        return false;
    }
    let Some(effect) = checker.runtime_effects.get(name.as_str()) else {
        return false;
    };
    let Some(TypeKind::Function(signature)) = checker.get_type(callee.id).map(|ty| &ty.kind) else {
        return false;
    };
    let parameters: Vec<&str> = signature.params.iter().map(|p| p.name.as_str()).collect();
    (bind_arguments_to_parameters(arguments, &parameters).get(index))
        .is_some_and(|&(position, _)| effect.writes_in_place(position))
}

#[cfg(test)]
mod tests;
