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
//!   receiver, as [`super::ReceiverWrites`] reads it off the method's body,
//! - an `out` argument, or
//! - an argument a runtime entry point writes in place, as its declaration
//!   states. A cast hands the runtime a value of its own and is not followed.
//!
//! Every function, method, nested function and closure is analysed, each
//! against its own body. A parameter is known by the id of its type
//! expression, which lowering reads off the same declaration.
//! Over-approximating costs one copy; missing a write is caught where the
//! write is lowered (`expression::unshare`).

use std::collections::HashSet;

use super::{BodyWrites, Mention, Role};
use crate::ast::common::Parameter;
use crate::ast::expression::{bind_arguments_to_parameters, Expression, ExpressionKind};
use crate::ast::types::{FunctionTypeData, TypeKind};
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

    /// Records the parameters each function and closure the body `writes`
    /// walked declares, at any depth, writes in its own body.
    pub(super) fn record(&mut self, writes: &BodyWrites, checker: &TypeChecker) {
        let written: HashSet<(usize, &str)> = (writes.mentions.iter())
            .filter(|mention| is_written(checker, mention))
            .map(|mention| (mention.owner, mention.name()))
            .collect();
        for function in &writes.functions {
            for param in function.params {
                if written.contains(&(function.body.id, param.name.as_str())) {
                    self.written.insert(param.typ.id);
                }
            }
        }
    }
}

/// Whether `mention` of a binding writes it, or writes through a path
/// starting at it: as an assignment's target, a writing method's receiver,
/// an `out` argument, or an argument a runtime entry point writes in place.
pub(super) fn is_written(checker: &TypeChecker, mention: &Mention) -> bool {
    match mention.role {
        Some(Role::Assigned { .. }) => true,
        Some(Role::Receiver { object, method }) => writes_receiver(checker, object, method),
        Some(Role::Argument {
            callee,
            arguments,
            index,
            through_cast,
            ..
        }) => writes_argument(checker, callee, arguments, index, through_cast),
        None => false,
    }
}

/// Whether a call of `callee` on `arguments` writes the one written
/// `index`th: an `out` argument, or one a runtime entry point writes in place
/// unless a cast, which hands the runtime a value of its own, stands between.
pub(super) fn writes_argument(
    checker: &TypeChecker,
    callee: &Expression,
    arguments: &[Expression],
    index: usize,
    through_cast: bool,
) -> bool {
    binds_out_parameter(checker, callee, arguments, index)
        || !through_cast && runtime_writes_argument(checker, callee, arguments, index)
}

/// Whether calling `method` on `object` writes the built-in collection
/// `object` is.
pub(super) fn writes_receiver(
    checker: &TypeChecker,
    object: &Expression,
    method: &Expression,
) -> bool {
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
    bound_parameter(checker, callee, arguments, index)
        .is_some_and(|(position, _)| effect.writes_in_place(position))
}

/// Whether the argument written `index`th of a call of `callee` on
/// `arguments` binds an `out` parameter, which the callee writes through.
fn binds_out_parameter(
    checker: &TypeChecker,
    callee: &Expression,
    arguments: &[Expression],
    index: usize,
) -> bool {
    bound_parameter(checker, callee, arguments, index)
        .and_then(|(position, signature)| signature.params.get(position))
        .is_some_and(|param| param.is_out)
}

/// The position of the parameter the argument written `index`th of a call
/// of `callee` on `arguments` binds, with the signature `callee` was checked
/// at; `None` when `callee` is no function.
fn bound_parameter<'c>(
    checker: &'c TypeChecker,
    callee: &Expression,
    arguments: &[Expression],
    index: usize,
) -> Option<(usize, &'c FunctionTypeData)> {
    let TypeKind::Function(signature) = &checker.get_type(callee.id)?.kind else {
        return None;
    };
    let parameters: Vec<&str> = signature.params.iter().map(|p| p.name.as_str()).collect();
    let &(position, _) = bind_arguments_to_parameters(arguments, &parameters).get(index)?;
    Some((position, signature))
}

#[cfg(test)]
mod tests;
