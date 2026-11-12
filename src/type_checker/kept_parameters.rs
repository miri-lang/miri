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
//! type hands its reference on when consumed
//! ([`super::utils::hands_on_a_reference`]), the predicate the consumption
//! check reads. Every function that cannot keep one calling convention lends
//! every parameter instead, and its callers keep releasing a consumed
//! argument after the call:
//!
//! - a constructor, a drop hook, and a method the compiler calls without a
//!   written call: a runtime thunk's, an operator's, a loop's;
//! - a generic function, or a method declaring type parameters of its own;
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
//! The table lists the parameters a method keeps in the order a call passes
//! its operands, the receiver first ([`KeptSlots`]).
//!
//! A method is compiled once for each type it runs at: a trait's default
//! once for each class inheriting it, a method of a generic class once for
//! each instantiation. Each such copy owns what it keeps at the types it is
//! compiled at ([`owned_at`]): the copy of `Wrap<T>.keep(x T)` at
//! `Wrap<Handle>` owns the `Handle` it keeps where the copy at `Wrap<int>`
//! owns nothing, and a default keeping its receiver owns it in a class
//! running a hook and lends it in one running none. The escape summary, shared by every copy, says which
//! positions are kept; the copy's types say which of those hand a reference
//! on.
//!
//! MIR lowering reads this table on both sides of every call: the callee's
//! signature carries the owned parameters, read at the types its body is
//! compiled at, and each direct call hands its argument over at the
//! positions the body it names owns, read at the same types.
//!
//! A method a vtable slot stands for — one a trait or an abstract class
//! declares, or one a class overrides — owns what it keeps like any other.
//! A call naming it, a static call or a `super` call, follows its own
//! convention; a call through the slot follows the slot's, which owns what
//! any body filling it owns ([`super::slot_conventions`]), and reaches a body
//! keeping less through an adapter.
//!
//! A call through a function value or a closure cannot see which body it
//! reaches, so it follows the convention of the value's type instead
//! ([`function_value_convention`]): it owns every position whose declared
//! type hands its reference on. A lambda or nested function owns exactly
//! those positions. A function named as a value keeps its own convention at
//! the calls naming it, and the value reaches it through a forwarding body
//! that owns the value's positions, hands the function what it keeps, and
//! releases the rest once the function returns.

use std::collections::HashMap;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::implicit_methods::is_called_without_a_written_call;
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::ast::types::{FunctionTypeData, TypeKind};
use crate::ast::Parameter;

use super::context::{resolve_method_source, MethodInfo, MethodSource, TypeDefinition};
use super::escape_analysis::{EscapeSummary, FunctionId};
use super::function_analysis::{DeclaredFunction, ModuleId};
use super::utils::{hands_on_a_reference, ResourceTypes};

/// For each top-level function owning a resource parameter, which of its
/// parameters it owns, in parameter order, by the id of the statement
/// declaring it; beside it, for each method keeping a parameter, which ones
/// it keeps. A function absent from the table lends every parameter.
#[derive(Debug, Default)]
pub struct KeptParameters {
    by_declaration: HashMap<usize, Vec<bool>>,
    declarations: HashMap<DeclaredFunction, usize>,
    /// The statements declaring each method, by the type declaring it and
    /// then its name. A call names a method by those two alone, so a method
    /// two statements declare owns nothing: no call could tell which body it
    /// reaches.
    methods: HashMap<String, HashMap<String, Vec<usize>>>,
    /// For each method keeping a parameter it may own, which parameters it
    /// keeps, in operand order, by the id of the statement declaring it.
    /// Which of them a copy of the method owns is read at the types the copy
    /// is compiled at ([`owned_at`]).
    kept_by_methods: HashMap<usize, Vec<bool>>,
}

impl KeptParameters {
    /// Which parameters `function` owns, in parameter order; empty when it
    /// owns none.
    pub fn of(&self, function: &DeclaredFunction) -> &[bool] {
        (self.declarations.get(function)).map_or(&[], |&id| self.of_declaration(id))
    }

    /// Which parameters the method the statement `declaration` declares
    /// keeps, in the order a call passes its operands, the receiver first
    /// ([`KeptSlots`]); empty when it keeps none. A copy of the method owns
    /// those its types hand on ([`owned_at`]).
    pub fn kept_by_method(&self, declaration: usize) -> &[bool] {
        (self.kept_by_methods.get(&declaration)).map_or(&[], Vec::as_slice)
    }

    /// The one statement declaring the method `method` in the body of the
    /// class or trait `owner`; `None` when none or two do.
    pub fn method_declaration(&self, owner: &str, method: &str) -> Option<usize> {
        match (self.methods.get(owner)).and_then(|methods| methods.get(method)) {
            Some(declarations) if declarations.len() == 1 => Some(declarations[0]),
            Some(_) | None => None,
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
            self.kept_by_methods.remove(id);
        }
    }

    /// Which parameters the top-level function the statement `statement_id`
    /// declares owns, in parameter order; empty when it owns none.
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
    /// The named types that are resources, decided once for the program.
    pub resources: &'a ResourceTypes,
    pub escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    /// The module declaring each top-level function, by statement id.
    pub modules: &'a HashMap<usize, ModuleId>,
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

/// The name of the class or trait `statement` declares, with the statements
/// of its body; `None` for any other statement.
///
/// TODO: an enum's methods can own a receiver they keep only once the escape
/// analysis summarizes them; until then no enum method is seen keeping
/// anything, and every call to one lends its receiver.
fn declared_methods(statement: &Statement) -> Option<(&str, &[Statement])> {
    if let StatementKind::Trait(name, _, _, methods, _) = &statement.node {
        return Some((declared_type_name(name)?, methods.as_slice()));
    }
    let StatementKind::Class(class) = &statement.node else {
        return None;
    };
    Some((declared_type_name(&class.name)?, class.body.as_slice()))
}

/// The name a type declaration's name expression spells, its type
/// parameters aside.
pub(crate) fn declared_type_name(name: &Expression) -> Option<&str> {
    if let ExpressionKind::Identifier(name, _) = &name.node {
        return Some(name);
    }
    if let ExpressionKind::TypeDeclaration(inner, ..) = &name.node {
        return declared_type_name(inner);
    }
    None
}

/// Records the parameters the method `statement` declares in `owner`'s body
/// keeps, when it keeps any.
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
    let Some(method) = slot_method(owner, &decl.name, facts.type_definitions) else {
        return;
    };
    let Some(positions) = kept_method_positions(owner, (decl, method), facts) else {
        return;
    };
    kept.kept_by_methods.insert(statement.id, positions);
}

/// Which parameters the method `decl` of `owner`, registered as `method`,
/// keeps, in operand order; `None` when it keeps none or is called where no
/// call is written. A parameter declared `out` or optional is never kept: the
/// first is the caller's binding, the second hands on no reference of its
/// own.
///
/// The escape summary of a method numbers its receiver 0 and each parameter
/// a caller supplies from 1, a static method's included.
fn kept_method_positions(
    owner: &str,
    (decl, method): (&FunctionDeclarationData, &MethodInfo),
    facts: &KeepFacts<'_>,
) -> Option<Vec<bool>> {
    if !is_called_by_written_call(&decl.name, method) || !has_one_body(decl) {
        return None;
    }
    let function = FunctionId::method(owner, &decl.name);
    let summary = facts.escape_summaries.get(&function)?;
    let receiver = (!method.is_static).then(|| summary.directly_escapes(0));
    let params = (decl.explicit_params().iter().enumerate())
        .map(|(index, param)| summary.directly_escapes(index + 1) && is_held_whole(param))
        .collect();
    let kept = KeptSlots { receiver, params }.in_operand_order();
    kept.contains(&true).then_some(kept)
}

/// Which positions a copy of a method keeping `kept` owns, compiled with its
/// operands at `operand_kinds`, one per position in operand order with the
/// receiver first: each it keeps whose type `hands_on` says hands its
/// reference on. Empty when it owns none.
///
/// The one rule every reader of a method's convention applies: the body
/// binding what it owns, each call naming the body, and each vtable slot
/// the body fills, each passing the types it reads the body at.
pub fn owned_at<'k>(
    kept: &[bool],
    operand_kinds: impl IntoIterator<Item = &'k TypeKind>,
    hands_on: impl Fn(&TypeKind) -> bool,
) -> Vec<bool> {
    let owned: Vec<bool> = (kept.iter().zip(operand_kinds))
        .map(|(&is_kept, kind)| is_kept && hands_on(kind))
        .collect();
    if owned.contains(&true) {
        owned
    } else {
        Vec::new()
    }
}

/// The statement declaring the body an `owner` receiver runs for `method`,
/// with its signature: the nearest class giving the method a body, or the
/// trait default the class inherits. `owner` may also name the trait whose
/// shared default a body is lowered for. `None` for an abstract declaration,
/// and for a method no one statement declares.
pub(crate) fn method_run_by<'d>(
    kept: &KeptParameters,
    type_definitions: &'d HashMap<String, TypeDefinition>,
    (owner, method): (&str, &str),
) -> Option<(usize, &'d MethodInfo)> {
    let (declaring, info) = match resolve_method_source(type_definitions, owner, method) {
        Some(MethodSource::Declared { class, info }) => (class, info),
        Some(MethodSource::Default(default)) => (default.trait_name, default.info),
        Some(MethodSource::AbstractOnly { .. }) => return None,
        None => (owner, slot_method(owner, method, type_definitions)?),
    };
    Some((kept.method_declaration(declaring, method)?, info))
}

/// The signature the class or trait `owner` registers for its method
/// `name`.
fn slot_method<'d>(
    owner: &str,
    name: &str,
    type_definitions: &'d HashMap<String, TypeDefinition>,
) -> Option<&'d MethodInfo> {
    match type_definitions.get(owner)? {
        TypeDefinition::Class(class) => class.methods.get(name),
        TypeDefinition::Trait(definition) => definition.methods.get(name),
        TypeDefinition::Struct(_)
        | TypeDefinition::Enum(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_) => None,
    }
}

/// Whether every call to the method `name` is written, naming it or reading
/// a vtable slot, rather than made by a runtime thunk, an operator or a loop:
/// no constructor, no generic method, and a name the compiler calls nothing
/// by.
fn is_called_by_written_call(name: &str, method: &MethodInfo) -> bool {
    !method.is_constructor
        && !method.declares_type_parameters()
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
    if !has_one_body(decl) {
        return None;
    }
    let summary = (facts.escape_summaries).get(&FunctionId::Declared(function.clone()))?;
    let is_owned = |(index, param): (usize, &Parameter)| {
        summary.directly_escapes(index)
            && is_handed_on(param, facts.type_definitions, facts.resources)
    };
    (decl.params.iter().enumerate().any(is_owned))
        .then(|| decl.params.iter().enumerate().map(is_owned).collect())
}

/// Whether a value `param` declares hands its reference on when a call keeps
/// it: held whole ([`is_held_whole`]), and of a type [`hands_on_a_reference`]
/// says hands it on.
fn is_handed_on(
    param: &Parameter,
    type_definitions: &HashMap<String, TypeDefinition>,
    resources: &ResourceTypes,
) -> bool {
    let ExpressionKind::Type(declared, false) = &param.typ.node else {
        return false;
    };
    is_held_whole(param) && hands_on_a_reference(&declared.kind, type_definitions, resources)
}

/// Whether `param` holds a whole value a call can hand over: declared
/// neither `out`, the caller's own binding, nor optional, whose payload
/// hands on no reference of its own.
fn is_held_whole(param: &Parameter) -> bool {
    matches!(&param.typ.node, ExpressionKind::Type(_, false)) && !param.is_out
}

/// Which argument positions a call through a function value of the type
/// `function` owns, in the order the call passes its arguments: each one
/// whose declared type hands its reference on, the predicate a parameter a
/// function keeps reads. `function` is read at the caller's substitution, so
/// a value of a generic body's type owns what the instantiation's types hand
/// on.
///
/// The one convention every call through a value, every lambda and nested
/// function body, and every forwarding body of a function named as a value
/// follows.
pub fn function_value_convention(
    function: &FunctionTypeData,
    type_definitions: &HashMap<String, TypeDefinition>,
    resources: &ResourceTypes,
) -> Vec<bool> {
    (function.params.iter())
        .map(|param| is_handed_on(param, type_definitions, resources))
        .collect()
}
