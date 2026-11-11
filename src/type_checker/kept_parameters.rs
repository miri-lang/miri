// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The resource parameters a function owns: the one answer to "who holds the
//! reference a call hands this parameter".
//!
//! A call lends its arguments, except a resource the callee keeps: the type
//! checker consumes that argument at the call ([`super::use_after_move`]), and
//! the callee becomes its owner. The caller's reference transfers at the call,
//! the callee binds it as a local of its own, and its drop hook runs where the
//! callee's new owner ends — the aggregate or binding holding it — or where
//! the callee ends on a path that keeps it nowhere.
//!
//! A parameter is owned when the converged escape summary keeps it and its
//! declared type hands its reference on when consumed
//! ([`super::use_after_move::hands_on_a_reference`]), the predicate the
//! consumption check reads. Every function that cannot keep one calling
//! convention lends every parameter instead, and its callers keep releasing a
//! consumed argument after the call:
//!
//! - a method, which its class's dispatch reaches as well as its name, and
//!   whose receiver a call writes in place;
//! - a function named as a value, called through the value with the one
//!   convention every function of its type shares;
//! - a generic function, whose body is shared by its instantiations, so no
//!   instance can own a parameter the shared body leaves open;
//! - a `gpu`, `async` or `parallel` function, and one with no body;
//! - a function an imported module declares. The consumption check walks the
//!   program's own bodies only, so an imported body records no consumed
//!   argument and could not hand an owned parameter on by `move`.
//!
//! An `out` parameter is the caller's binding itself and is never owned.
//!
//! MIR lowering reads this table on both sides of every call: the callee's
//! signature carries the owned parameters, and each direct call hands its
//! argument over at the positions the callee owns.

use std::collections::{HashMap, HashSet};

use crate::ast::expression::ExpressionKind;
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::ast::Parameter;

use super::context::TypeDefinition;
use super::escape_analysis::{EscapeSummary, FunctionId};
use super::function_analysis::{DeclaredFunction, ModuleId};
use super::utils::hands_on_a_reference;

/// For each function owning a resource parameter, which of its parameters
/// it owns, in parameter order, by the id of the statement declaring it. A
/// function absent from the table lends every parameter.
#[derive(Debug, Default)]
pub struct KeptParameters {
    by_declaration: HashMap<usize, Vec<bool>>,
    declarations: HashMap<DeclaredFunction, usize>,
}

impl KeptParameters {
    /// Which parameters `function` owns, in parameter order; empty when it
    /// owns none.
    pub fn of(&self, function: &DeclaredFunction) -> &[bool] {
        (self.declarations.get(function)).map_or(&[], |&id| self.of_declaration(id))
    }

    /// Which parameters the function the statement `statement_id` declares
    /// owns, in parameter order; empty when it owns none.
    pub fn of_declaration(&self, statement_id: usize) -> &[bool] {
        self.by_declaration
            .get(&statement_id)
            .map_or(&[], Vec::as_slice)
    }
}

/// What deciding the owned parameters reads of the checked program.
pub(crate) struct KeepFacts<'a> {
    pub type_definitions: &'a HashMap<String, TypeDefinition>,
    pub escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    /// The module declaring each top-level function, by statement id.
    pub modules: &'a HashMap<usize, ModuleId>,
    /// The functions some expression names as a value rather than calls.
    pub function_values: &'a HashSet<DeclaredFunction>,
}

/// The owned resource parameters of every top-level function among
/// `statements`.
pub(crate) fn decide_kept_parameters<'a>(
    statements: impl IntoIterator<Item = &'a Statement>,
    facts: &KeepFacts<'_>,
) -> KeptParameters {
    let mut kept = KeptParameters::default();
    for statement in statements {
        collect_kept_parameters(statement, facts, &mut kept);
    }
    kept
}

fn collect_kept_parameters(
    statement: &Statement,
    facts: &KeepFacts<'_>,
    kept: &mut KeptParameters,
) {
    if let StatementKind::Block(statements) = &statement.node {
        for inner in statements {
            collect_kept_parameters(inner, facts, kept);
        }
        return;
    }
    let StatementKind::FunctionDeclaration(decl) = &statement.node else {
        return;
    };
    let Some(module) = facts.modules.get(&statement.id) else {
        return;
    };
    let function = DeclaredFunction {
        module: module.clone(),
        name: decl.name.clone(),
    };
    if let Some(owned) = owned_parameters(decl, &function, facts) {
        kept.by_declaration.insert(statement.id, owned);
        kept.declarations.insert(function, statement.id);
    }
}

/// Which parameters of `decl`, declared as `function`, it owns; `None` when
/// it owns none.
fn owned_parameters(
    decl: &FunctionDeclarationData,
    function: &DeclaredFunction,
    facts: &KeepFacts<'_>,
) -> Option<Vec<bool>> {
    if !has_one_calling_convention(decl, function, facts) {
        return None;
    }
    let summary = (facts.escape_summaries).get(&FunctionId::Declared(function.clone()))?;
    let is_owned = |(index, param): (usize, &Parameter)| {
        summary.directly_escapes(index) && is_handed_on(param, facts.type_definitions)
    };
    (decl.params.iter().enumerate().any(is_owned))
        .then(|| decl.params.iter().enumerate().map(is_owned).collect())
}

/// Whether every call reaches `decl` directly, with one body for all of them:
/// a host function with a body, no type parameters, never named as a value.
fn has_one_calling_convention(
    decl: &FunctionDeclarationData,
    function: &DeclaredFunction,
    facts: &KeepFacts<'_>,
) -> bool {
    let properties = &decl.properties;
    let is_host = !(properties.is_gpu || properties.is_async || properties.is_parallel);
    let is_generic = decl
        .generics
        .as_ref()
        .is_some_and(|generics| !generics.is_empty());
    is_host && !is_generic && decl.body.is_some() && !facts.function_values.contains(function)
}

/// Whether a value `param` declares hands its reference on when a call keeps
/// it: not `out`, and of a type whose own drop hook its release runs.
fn is_handed_on(param: &Parameter, type_definitions: &HashMap<String, TypeDefinition>) -> bool {
    let ExpressionKind::Type(declared, false) = &param.typ.node else {
        return false;
    };
    !param.is_out && hands_on_a_reference(&declared.kind, type_definitions)
}
