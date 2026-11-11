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
//! - a method a vtable slot stands for — one a trait or an abstract class
//!   declares, or one a class overrides — whose every body shares the one
//!   convention the slot's callers use;
//! - a constructor, a drop hook, and a method the compiler calls without a
//!   written call: a runtime thunk's, an operator's, a loop's;
//! - a function named as a value, called through the value with the one
//!   convention every function of its type shares;
//! - a generic function or method, or a method of a generic class, whose
//!   body is shared by its instantiations, so no instance can own a
//!   parameter the shared body leaves open;
//! - a method of an enum, which the escape analysis does not summarize;
//! - a `gpu`, `async` or `parallel` function, and one with no body;
//! - a function an imported module declares. The consumption check walks the
//!   program's own bodies only, so an imported body records no consumed
//!   argument and could not hand an owned parameter on by `move`.
//!
//! An `out` parameter is the caller's binding itself and is never owned.
//!
//! A method's receiver is a parameter like any other: a method keeping its
//! instance — storing it, returning it, handing it to a keeper — owns it.
//! The table lists a method's owned parameters in the order a call passes
//! its operands, the receiver first ([`KeptSlots`]).
//!
//! MIR lowering reads this table on both sides of every call: the callee's
//! signature carries the owned parameters, and each direct call hands its
//! argument over at the positions the callee owns.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::implicit_methods::is_called_without_a_written_call;
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::ast::types::TypeKind;
use crate::ast::Parameter;

use super::context::{MethodInfo, TypeDefinition};
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
    /// The statements declaring each method, by the type declaring it and
    /// then its name. A call names a method by those two alone, so a method
    /// two statements declare owns nothing: no call could tell which body it
    /// reaches.
    methods: HashMap<String, HashMap<String, Vec<usize>>>,
}

impl KeptParameters {
    /// Which parameters `function` owns, in parameter order; empty when it
    /// owns none.
    pub fn of(&self, function: &DeclaredFunction) -> &[bool] {
        (self.declarations.get(function)).map_or(&[], |&id| self.of_declaration(id))
    }

    /// Which parameters the method `method` of the type `owner` owns, in the
    /// order a call passes its operands, the receiver first ([`KeptSlots`]);
    /// empty when it owns none.
    pub fn of_method(&self, owner: &str, method: &str) -> &[bool] {
        match self
            .methods
            .get(owner)
            .and_then(|methods| methods.get(method))
        {
            Some(declarations) if declarations.len() == 1 => self.of_declaration(declarations[0]),
            Some(_) | None => &[],
        }
    }

    /// Takes back what any method declared by two statements of one name
    /// on one type owns: a call names it by type and name alone.
    fn forget_ambiguous_methods(&mut self) {
        let ambiguous = (self.methods.values())
            .flat_map(HashMap::values)
            .filter(|declarations| declarations.len() > 1)
            .flatten();
        for id in ambiguous {
            self.by_declaration.remove(id);
        }
    }

    /// Which parameters the function or method the statement `statement_id`
    /// declares owns, in the order a call passes its operands; empty when it
    /// owns none.
    pub fn of_declaration(&self, statement_id: usize) -> &[bool] {
        self.by_declaration
            .get(&statement_id)
            .map_or(&[], Vec::as_slice)
    }
}

/// The parameters a method owns: its receiver, when it has one, and each
/// parameter a caller supplies, in declaration order.
pub(crate) struct KeptSlots {
    /// Whether the method owns its receiver; `None` for a static method,
    /// which has none.
    pub receiver: Option<bool>,
    pub params: Vec<bool>,
}

impl KeptSlots {
    /// The owned parameters in the order a call passes its operands: the
    /// receiver first, then each parameter. The order MIR numbers a body's
    /// parameters in, so both sides of a call read one list.
    pub(crate) fn in_operand_order(self) -> Vec<bool> {
        self.receiver.into_iter().chain(self.params).collect()
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
    /// Every method name a vtable slot stands for
    /// ([`super::context::vtable_selectors`]).
    pub selectors: &'a BTreeSet<&'a str>,
}

/// The owned resource parameters of every top-level function, and of every
/// method of a class, among `statements`.
pub(crate) fn decide_kept_parameters<'a>(
    statements: impl IntoIterator<Item = &'a Statement>,
    facts: &KeepFacts<'_>,
) -> KeptParameters {
    let mut kept = KeptParameters::default();
    for statement in statements {
        collect_kept_parameters(statement, facts, &mut kept);
    }
    kept.forget_ambiguous_methods();
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
    if let Some((owner, methods)) = declared_methods(statement) {
        for method in methods {
            collect_kept_method(owner, method, facts, kept);
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

/// The name of the class `statement` declares, with the statements of its
/// body; `None` for any other statement.
///
/// TODO: an enum's methods can own a receiver they keep only once the escape
/// analysis summarizes them; until then no enum method is seen keeping
/// anything, and every call to one lends its receiver.
fn declared_methods(statement: &Statement) -> Option<(&str, &[Statement])> {
    let StatementKind::Class(class) = &statement.node else {
        return None;
    };
    Some((declared_type_name(&class.name)?, class.body.as_slice()))
}

/// The name a type declaration's name expression spells, its type
/// parameters aside.
fn declared_type_name(name: &Expression) -> Option<&str> {
    if let ExpressionKind::Identifier(name, _) = &name.node {
        return Some(name);
    }
    if let ExpressionKind::TypeDeclaration(inner, ..) = &name.node {
        return declared_type_name(inner);
    }
    None
}

/// Records the parameters the method `statement` declares in `owner`'s body
/// owns, when it owns any.
fn collect_kept_method(
    owner: &str,
    statement: &Statement,
    facts: &KeepFacts<'_>,
    kept: &mut KeptParameters,
) {
    let StatementKind::FunctionDeclaration(decl) = &statement.node else {
        return;
    };
    let declarations = kept.methods.entry(owner.to_string()).or_default();
    (declarations.entry(decl.name.clone()).or_default()).push(statement.id);
    if let Some(owned) = owned_method_parameters(owner, decl, facts) {
        kept.by_declaration.insert(statement.id, owned);
    }
}

/// Which parameters the method `decl` of `owner` owns, in operand order;
/// `None` when it owns none or cannot own any.
///
/// The escape summary of a method numbers its receiver 0 and each parameter
/// a caller supplies from 1, a static method's included.
fn owned_method_parameters(
    owner: &str,
    decl: &FunctionDeclarationData,
    facts: &KeepFacts<'_>,
) -> Option<Vec<bool>> {
    let method = declared_method(owner, &decl.name, facts.type_definitions)?;
    if !is_called_by_name_alone(&decl.name, method, facts) || !has_one_body(decl) {
        return None;
    }
    let function = FunctionId::method(owner, &decl.name);
    let summary = facts.escape_summaries.get(&function)?;
    let receiver_kind = TypeKind::Custom(owner.to_string(), None);
    let receiver = (!method.is_static).then(|| {
        summary.directly_escapes(0) && hands_on_a_reference(&receiver_kind, facts.type_definitions)
    });
    let params = (decl.explicit_params().iter().enumerate())
        .map(|(index, param)| {
            summary.directly_escapes(index + 1) && is_handed_on(param, facts.type_definitions)
        })
        .collect();
    let owned = KeptSlots { receiver, params }.in_operand_order();
    owned.contains(&true).then_some(owned)
}

/// The signature `owner` registers for its method `name`, when `owner` is a
/// class declaring no type parameters.
fn declared_method<'d>(
    owner: &str,
    name: &str,
    type_definitions: &'d HashMap<String, TypeDefinition>,
) -> Option<&'d MethodInfo> {
    let Some(TypeDefinition::Class(class)) = type_definitions.get(owner) else {
        return None;
    };
    if (class.generics.as_ref()).is_some_and(|generics| !generics.is_empty()) {
        return None;
    }
    class.methods.get(name)
}

/// Whether every call to the method `name` names it, rather than reaching it
/// through a vtable slot, a runtime thunk, an operator or a loop: no
/// constructor, no generic method, and a name no vtable slot stands for and
/// the compiler calls nothing by.
fn is_called_by_name_alone(name: &str, method: &MethodInfo, facts: &KeepFacts<'_>) -> bool {
    !method.is_constructor
        && !method.declares_type_parameters()
        && !facts.selectors.contains(name)
        && !is_called_without_a_written_call(name)
}

/// Whether `decl` has one body every call runs: a host function with a body
/// and no type parameters of its own.
fn has_one_body(decl: &FunctionDeclarationData) -> bool {
    let properties = &decl.properties;
    let is_host = !(properties.is_gpu || properties.is_async || properties.is_parallel);
    let is_generic = decl
        .generics
        .as_ref()
        .is_some_and(|generics| !generics.is_empty());
    is_host && !is_generic && decl.body.is_some()
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
    has_one_body(decl) && !facts.function_values.contains(function)
}

/// Whether a value `param` declares hands its reference on when a call keeps
/// it: not `out`, and of a type whose own drop hook its release runs.
fn is_handed_on(param: &Parameter, type_definitions: &HashMap<String, TypeDefinition>) -> bool {
    let ExpressionKind::Type(declared, false) = &param.typ.node else {
        return false;
    };
    !param.is_out && hands_on_a_reference(&declared.kind, type_definitions)
}
