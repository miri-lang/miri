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
//! - a constructor and a drop hook, which building and releasing an
//!   instance runs;
//! - a method a container's runtime thunk calls ([`ThunkBodies`]), which the
//!   runtime hands borrowed elements. One keeping a resource it is handed is
//!   refused where it is declared ([`thunk_keeper_refusal`]);
//! - a generic function, or a method declaring type parameters of its own;
//! - a `gpu`, `async` or `parallel` function, and one with no body.
//!
//! Where a function is declared does not change what it owns: one an
//! imported module declares, the standard library's included, owns what it
//! keeps as one the program declares does.
//!
//! An `out` parameter is the caller's binding itself and is never owned.
//!
//! A call the compiler makes where the program wrote none — an operator's,
//! a `for` loop's — is a call like any other, and the method it reaches
//! owns what it keeps.
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
use crate::ast::factory::make_type;
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::ast::type_arguments::type_arguments;
use crate::ast::types::{FunctionTypeData, TypeKind};
use crate::ast::Parameter;
use crate::diagnostics::DiagnosticCode;
use crate::error::type_error::{TypeError, TypeErrorKind};

use super::body_writes::ReceiverWrites;
use super::context::{resolve_method_source, MethodInfo, MethodSource, TypeDefinition};
use super::diagnostics::shown_type_name;
use super::escape_analysis::{EscapeSummary, FunctionId};
use super::function_analysis::{DeclaredFunction, ModuleId};
use super::hook_receivers::receiver_escape_refusal;
use super::open_consumption::{OpenBody, OpenRefusal};
use super::place_reads::{kept_place_refusal, CallerPlace, PlaceRepairs};
use super::thunk_methods::ThunkBodies;
use super::type_identities::DeclaredNames;
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
    /// The calls restoring their receiver, which never keep it.
    pub restored: &'a ReceiverWrites,
    pub type_definitions: &'a HashMap<String, TypeDefinition>,
    /// The named types that are resources, decided once for the program.
    pub resources: &'a ResourceTypes,
    pub escape_summaries: &'a HashMap<FunctionId, EscapeSummary>,
    /// The module declaring each top-level function, by statement id.
    pub modules: &'a HashMap<usize, ModuleId>,
    /// The identity each type declaration and written type resolved to,
    /// which a method is keyed by and a parameter's type is read at.
    pub names: DeclaredNames<'a>,
    /// The spellings a refusal of a body keeping its caller's place names
    /// its repair in.
    pub repairs: &'a PlaceRepairs,
}

/// What deciding the owned parameters finds: the table, and a refusal for
/// each method a runtime thunk calls that keeps a resource it is handed,
/// beside the id of the statement declaring the method.
pub(crate) struct KeptDecision {
    pub kept: KeptParameters,
    pub refusals: Vec<(usize, TypeError)>,
    /// The refusals of a generic owner's method keeping a place of its
    /// caller's, each made at the copies reading the place as a resource,
    /// beside the id of the statement declaring the method.
    pub open_refusals: Vec<(usize, OpenRefusal)>,
}

impl KeptDecision {
    /// Records the refusals of the declaration `statement`: one made for
    /// the program, and those made at the copies of its body.
    fn record(&mut self, statement: usize, (closed, open): (Option<TypeError>, Vec<OpenRefusal>)) {
        self.refusals.extend(closed.map(|error| (statement, error)));
        (self.open_refusals).extend(open.into_iter().map(|refusal| (statement, refusal)));
    }
}

/// The owned resource parameters of every top-level function, and of every
/// method of a class, a trait or an enum, among `statements`.
pub(crate) fn decide_kept_parameters<'a>(
    statements: impl IntoIterator<Item = &'a Statement>,
    facts: (&KeepFacts<'_>, &ThunkBodies),
) -> KeptDecision {
    let mut decision = KeptDecision {
        kept: KeptParameters::default(),
        refusals: Vec::new(),
        open_refusals: Vec::new(),
    };
    for statement in statements {
        collect_kept_parameters(statement, facts, &mut decision);
    }
    decision.kept.forget_ambiguous_methods();
    decision
}

fn collect_kept_parameters(
    statement: &Statement,
    facts: (&KeepFacts<'_>, &ThunkBodies),
    decision: &mut KeptDecision,
) {
    if let StatementKind::Block(statements) = &statement.node {
        for inner in statements {
            collect_kept_parameters(inner, facts, decision);
        }
        return;
    }
    if let Some((owner, methods)) = declared_methods(statement, &facts.0.names) {
        for method in methods {
            collect_kept_method(owner, method, facts, decision);
        }
        return;
    }
    let (facts, _) = facts;
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
    let summary_key = FunctionId::Declared(function.clone());
    let refusals = kept_caller_place(decl, (&summary_key, None), facts, statement);
    decision.record(statement.id, refusals);
    if let Some(owned) = owned_parameters(decl, &function, facts) {
        let kept = &mut decision.kept;
        kept.by_declaration.insert(statement.id, owned);
        kept.declarations.insert(function, statement.id);
    }
}

/// The identity of the class, trait or enum `statement` declares, with the
/// statements of its body; `None` for any other statement. A type a module
/// keeps private is keyed by its qualified identity, so it never shares a
/// method's key with a type of the same written name in another file.
fn declared_methods<'s>(
    statement: &'s Statement,
    names: &DeclaredNames<'s>,
) -> Option<(&'s str, &'s [Statement])> {
    if let StatementKind::Trait(name, _, _, methods, _)
    | StatementKind::Enum(name, _, _, methods, _, _) = &statement.node
    {
        return Some((names.declared(name)?, methods.as_slice()));
    }
    let StatementKind::Class(class) = &statement.node else {
        return None;
    };
    Some((names.declared(&class.name)?, class.body.as_slice()))
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
/// keeps, when it keeps any. A method a runtime thunk calls keeps none: one
/// whose body keeps a resource it is handed is refused instead. A drop hook
/// lends its receiver and keeps nothing; one whose body keeps its receiver is
/// refused ([`receiver_escape_refusal`]).
fn collect_kept_method(
    owner: &str,
    statement: &Statement,
    (facts, thunk_bodies): (&KeepFacts<'_>, &ThunkBodies),
    decision: &mut KeptDecision,
) {
    let StatementKind::FunctionDeclaration(decl) = &statement.node else {
        return;
    };
    let declarations = decision.kept.methods.entry(owner.to_string()).or_default();
    (declarations.entry(decl.name.clone()).or_default()).push(statement.id);
    let refusals = if decl.is_drop_hook() {
        let refusal = receiver_escape_refusal(owner, statement, facts.escape_summaries);
        (refusal, Vec::new())
    } else {
        let method = FunctionId::method(owner, &decl.name);
        kept_caller_place(decl, (&method, Some(owner)), facts, statement)
    };
    decision.record(statement.id, refusals);
    if decl.is_drop_hook() {
        return;
    }
    let Some(method) = slot_method(owner, &decl.name, facts.type_definitions) else {
        return;
    };
    let Some(positions) = kept_method_positions(owner, (decl, method), facts) else {
        return;
    };
    if thunk_bodies.contains(owner, &decl.name) {
        let refusal = thunk_keeper_refusal(owner, (decl, method), &positions, facts);
        decision
            .refusals
            .extend(refusal.map(|error| (statement.id, error.at(statement))));
        return;
    }
    decision
        .kept
        .kept_by_methods
        .insert(statement.id, positions);
}

/// The refusals of `decl`, summarised as `function`, keeping a place of its
/// caller's it holds ([`kept_place_refusal`]): an `out` parameter holding a
/// resource, or, for a method of `owner` replacing its receiver, a receiver
/// holding one. The summary numbers a method's receiver 0 and each
/// parameter a caller supplies from 1. A place that is a resource only at
/// the copies of a generic owner's method binding one of the owner's type
/// parameters to one, or a function or method binding one of its own to
/// one, is refused at those copies ([`OpenRefusal`]).
fn kept_caller_place(
    decl: &FunctionDeclarationData,
    (function, owner): (&FunctionId, Option<&str>),
    facts: &KeepFacts<'_>,
    statement: &Statement,
) -> (Option<TypeError>, Vec<OpenRefusal>) {
    let generic_owner = owner
        .and_then(|owner| OpenBody::of_generic_owner((owner, &decl.name), facts.type_definitions));
    let open_body = OpenBody::of_declaration(function, decl, generic_owner);
    let mut closed = None;
    let mut open = Vec::new();
    for (place, resource) in caller_places(decl, owner, open_body.as_ref(), facts) {
        let facts_read = (facts.escape_summaries, facts.repairs);
        let Some(error) = kept_place_refusal(function, &place, facts_read, statement.span) else {
            continue;
        };
        match (resource, &open_body) {
            (PlaceResource::Always, _) => {
                closed.get_or_insert(error);
            }
            (PlaceResource::AtCopies(parameters), Some(body)) => {
                open.extend(parameters.into_iter().map(|parameter| OpenRefusal {
                    error: error.clone(),
                    body: body.clone(),
                    parameter,
                }));
            }
            (PlaceResource::AtCopies(_), None) => {}
        }
    }
    (closed, open)
}

/// The places of its caller's `decl`, a method of `owner` when one is
/// given, holds, each with when it is a resource: the receiver of a method
/// replacing it, and each `out` parameter. The summary numbers a method's
/// receiver 0 and each parameter a caller supplies from 1.
fn caller_places<'d>(
    decl: &'d FunctionDeclarationData,
    owner: Option<&str>,
    open_body: Option<&OpenBody>,
    facts: &KeepFacts<'_>,
) -> Vec<(CallerPlace<'d>, PlaceResource)> {
    let receiver = owner
        .filter(|owner| facts.restored.replaces_receiver(owner, &decl.name))
        .and_then(|owner| restored_receiver_resource(owner, facts))
        .map(|resource| (CallerPlace::RestoredReceiver, resource));
    let (params, first_slot) = match owner {
        Some(_) => (decl.explicit_params(), 1),
        None => (decl.params.as_slice(), 0),
    };
    let out_params = (params.iter().enumerate())
        .filter(|(_, param)| param.is_out)
        .filter_map(|(index, param)| {
            let resource = out_place_resource(param, open_body, facts)?;
            let slot = index + first_slot;
            Some((
                CallerPlace::OutParameter {
                    slot,
                    name: &param.name,
                },
                resource,
            ))
        });
    receiver.into_iter().chain(out_params).collect()
}

/// When a place a body holds for its caller is a resource: at every copy of
/// the body, or at the copies binding one of the type parameters open in it,
/// by name, to a resource.
enum PlaceResource {
    Always,
    AtCopies(Vec<String>),
}

/// When the receiver of a method of `owner` that restores it is a resource
/// ([`ResourceTypes::holds`]); `None` when it never is.
fn restored_receiver_resource(owner: &str, facts: &KeepFacts<'_>) -> Option<PlaceResource> {
    let definitions = facts.type_definitions;
    if facts.resources.holds(&receiver_kind(owner, definitions)) {
        return Some(PlaceResource::Always);
    }
    let generics = definitions.get(owner).and_then(TypeDefinition::generics)?;
    let parameters: Vec<String> = (facts.resources.positions_making_a_resource(owner))
        .filter_map(|at| generics.get(at).map(|generic| generic.name.clone()))
        .collect();
    (!parameters.is_empty()).then_some(PlaceResource::AtCopies(parameters))
}

/// When the place the `out` parameter `param` holds is a resource: always
/// ([`ResourceTypes::holds`]), or, written as a type parameter open in
/// `open_body`, or an optional of one, at the copies binding that parameter
/// to a resource. `None` when it never is.
fn out_place_resource(
    param: &Parameter,
    open_body: Option<&OpenBody>,
    facts: &KeepFacts<'_>,
) -> Option<PlaceResource> {
    let ExpressionKind::Type(declared, _) = &param.typ.node else {
        return None;
    };
    let kind = facts.names.written_kind(&param.typ, &declared.kind);
    if facts.resources.holds(&kind) {
        return Some(PlaceResource::Always);
    }
    let payload = if let TypeKind::Option(payload) = kind.as_ref() {
        &payload.kind
    } else {
        kind.as_ref()
    };
    let parameter = open_body?.parameter_of(payload, facts.type_definitions)?;
    Some(PlaceResource::AtCopies(vec![parameter]))
}

/// The type a method of `owner` reads its receiver at: the owner at its own
/// type parameters, each with the bounds it declares.
fn receiver_kind(owner: &str, type_definitions: &HashMap<String, TypeDefinition>) -> TypeKind {
    let generics = (type_definitions.get(owner)).and_then(TypeDefinition::generics);
    let arguments = generics.map(|generics| {
        type_arguments(generics.iter().map(|generic| {
            make_type(TypeKind::Generic(
                generic.name.clone(),
                generic.bounds.clone(),
            ))
        }))
    });
    TypeKind::Custom(owner.to_string(), arguments)
}

/// The refusal of the method `decl` of `owner`, which a runtime thunk calls,
/// keeping a resource it is handed: the first position of `kept`, in operand
/// order, whose declared type hands its reference on. `None` when every
/// position it keeps holds a value of no such type, which the runtime may
/// lend it.
fn thunk_keeper_refusal(
    owner: &str,
    (decl, method): (&FunctionDeclarationData, &MethodInfo),
    kept: &[bool],
    facts: &KeepFacts<'_>,
) -> Option<ThunkKeeper> {
    let receiver_kind = TypeKind::Custom(owner.to_string(), None);
    let receiver = (!method.is_static).then(|| {
        let hands_on =
            hands_on_a_reference(&receiver_kind, facts.type_definitions, facts.resources);
        ("self", hands_on)
    });
    let params = (decl.explicit_params().iter())
        .map(|param| (param.name.as_str(), is_declared_handed_on(param, facts)));
    let (name, _) = (receiver.into_iter().chain(params).zip(kept))
        .find(|((_, hands_on), &is_kept)| is_kept && *hands_on)?
        .0;
    Some(ThunkKeeper {
        owner: shown_type_name(owner).to_string(),
        method: decl.name.clone(),
        kept: name.to_string(),
    })
}

/// A method a runtime thunk calls that keeps the resource it is handed as
/// `kept`.
struct ThunkKeeper {
    owner: String,
    method: String,
    kept: String,
}

impl ThunkKeeper {
    /// The refusal, reported at the declaration `statement`.
    fn at(self, statement: &Statement) -> TypeError {
        let ThunkKeeper {
            owner,
            method,
            kept,
        } = self;
        TypeError {
            kind: TypeErrorKind::Coded {
                code: DiagnosticCode::OwnThunkMethodKeepsValue,
                message: format!(
                    "'{owner}.{method}' keeps '{kept}', but a container calls '{method}' on \
                     elements it only lends, so it cannot keep one"
                ),
                help: Some(format!(
                    "make '{method}' only read '{kept}', and keep the value in a method the \
                     program calls by name"
                )),
                expected: None,
                actual: None,
            },
            span: statement.span,
            source_override: None,
            repair: None,
            related: Vec::new(),
        }
    }
}

/// Which parameters the method `decl` of `owner`, registered as `method`,
/// keeps, in operand order; `None` when it keeps none or has more than one
/// calling convention ([`has_one_convention`]). A parameter declared `out` or optional is never kept: the
/// first is the caller's binding, the second hands on no reference of its
/// own. A receiver the method restores is the caller's binding as well.
///
/// The escape summary of a method numbers its receiver 0 and each parameter
/// a caller supplies from 1, a static method's included.
fn kept_method_positions(
    owner: &str,
    (decl, method): (&FunctionDeclarationData, &MethodInfo),
    facts: &KeepFacts<'_>,
) -> Option<Vec<bool>> {
    if !has_one_convention(decl, method) || !has_one_body(decl) {
        return None;
    }
    let function = FunctionId::method(owner, &decl.name);
    let summary = facts.escape_summaries.get(&function)?;
    let receiver = (!method.is_static).then(|| {
        summary.directly_escapes(0) && !facts.restored.replaces_receiver(owner, &decl.name)
    });
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

/// The signature the class, trait or enum `owner` registers for its method
/// `name`.
fn slot_method<'d>(
    owner: &str,
    name: &str,
    type_definitions: &'d HashMap<String, TypeDefinition>,
) -> Option<&'d MethodInfo> {
    match type_definitions.get(owner)? {
        TypeDefinition::Class(class) => class.methods.get(name),
        TypeDefinition::Trait(definition) => definition.methods.get(name),
        TypeDefinition::Enum(definition) => definition.methods.get(name),
        TypeDefinition::Struct(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_) => None,
    }
}

/// Whether every call to the method `decl`, registered as `method`, passes
/// its operands one way: not a constructor or a drop hook, which building
/// and releasing an instance runs, and not a method declaring type
/// parameters of its own.
fn has_one_convention(decl: &FunctionDeclarationData, method: &MethodInfo) -> bool {
    !method.is_constructor && !decl.is_drop_hook() && !method.declares_type_parameters()
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
        summary.directly_escapes(index) && is_declared_handed_on(param, facts)
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
    held_kind(param).is_some_and(|kind| hands_on_a_reference(kind, type_definitions, resources))
}

/// [`is_handed_on`] for a parameter a declaration writes, its type read at
/// the identity checking resolved it to.
fn is_declared_handed_on(param: &Parameter, facts: &KeepFacts<'_>) -> bool {
    held_kind(param).is_some_and(|kind| {
        let kind = facts.names.written_kind(&param.typ, kind);
        hands_on_a_reference(&kind, facts.type_definitions, facts.resources)
    })
}

/// The type `param` declares, when it holds a whole value
/// ([`is_held_whole`]).
fn held_kind(param: &Parameter) -> Option<&TypeKind> {
    let ExpressionKind::Type(declared, false) = &param.typ.node else {
        return None;
    };
    is_held_whole(param).then_some(&declared.kind)
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
