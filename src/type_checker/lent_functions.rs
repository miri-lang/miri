// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The function-typed parameters a method lends its arguments through,
//! inferred from the method's body.
//!
//! A call through a function value owns every resource argument it is
//! handed, as the value's body may keep any of them. A method whose body
//! calls its function-typed parameter `f` with a value it does not own
//! whole — an element of its receiver, a `for` binding, a borrowed place,
//! or a local bound to one ([`super::place_reads`]) — lends `f` the
//! argument at that position: the method still holds the value after the
//! call, so `f` may only read it. Every call through `f` then lends that
//! position, and a caller may pass for `f` only a function keeping nothing
//! there; that is checked where the method is called. A position the body
//! hands `f` only values it owns keeps owning them. The body handing `f` to
//! another method's lending parameter, or to a runtime entry point that
//! calls it with what its container holds, lends what that callee lends.
//!
//! A method with no body owns every argument. A method implementing a
//! declaration — an abstract one, a trait's default, a base class's —
//! lends only where that declaration lends, since a caller holding the
//! declared type is checked against the declaration.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::ast::expression::{Expression, ExpressionKind, LambdaData};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{
    FunctionDeclarationData, Statement, StatementKind, RECEIVER_PARAM_NAME,
};
use crate::ast::types::TypeKind;
use crate::runtime_effects::RuntimeEffects;

use super::context::{
    class_ancestry, inherited_trait_requirement, resolve_method_source, trait_lineage,
    MethodSource, TypeDefinition,
};
use super::escape_analysis::{collect_pattern_bindings, returned_tails, FunctionId};
use super::place_reads::{place_spelling, BorrowedPlace, PlaceBindings, PlaceFacts};
use super::returned_places::{receiver_type_name, Handed, Returned, ReturnedPlaces};
use super::type_identities::DeclaredNames;
use super::use_after_move::walked_value;

/// How a call through a lending parameter treats the argument at one
/// position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lending {
    /// The position is lent wherever the method is called.
    Always,
    /// The position is lent where the receiver's class gives one of the
    /// abstract methods named a body handing back a place of its receiver:
    /// the body hands the position what that method returns on `self`.
    WhereReturned(BTreeSet<String>),
}

/// The positions, among its arguments, a call through one function-typed
/// parameter lends.
pub(crate) type LentArguments = BTreeMap<usize, Lending>;

/// The lending function-typed parameters of one method, by name.
pub(crate) type LentParameters = BTreeMap<String, LentArguments>;

/// The lending parameters of every method of the program and its modules
/// that has any, keyed as the method's escape summary is.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LendingMethods {
    methods: HashMap<FunctionId, LentParameters>,
    /// The positions each method's body may treat as lent: those lent
    /// wherever it is called, and those lent where a class settles it when
    /// every class running the body settles it so.
    in_body: HashMap<FunctionId, HashMap<String, BTreeSet<usize>>>,
}

/// What inferring lending reads of the checked program.
pub(crate) struct LendingFacts<'a> {
    pub place: PlaceFacts<'a>,
    pub effects: &'a RuntimeEffects,
    pub names: DeclaredNames<'a>,
}

/// One lending parameter of a method, as a call site reads it.
pub(crate) struct LendingParameter<'m> {
    /// Its position among the method's declared parameters.
    pub position: usize,
    pub name: &'m str,
    /// The argument positions a call through it lends.
    pub lent: BTreeSet<usize>,
}

impl LendingMethods {
    /// The methods handing back a place of their receiver, and the lending
    /// parameters, of the method bodies among `statements`, the program's
    /// and every module's top level, worked out together to a fixpoint: one
    /// walk of each body reads both tables as the previous round left them.
    pub(crate) fn infer<'s>(
        statements: impl Iterator<Item = &'s Statement> + Clone,
        facts: &LendingFacts<'_>,
    ) -> (ReturnedPlaces, Self) {
        let mut returned = ReturnedPlaces::default();
        let mut table = LendingMethods::default();
        loop {
            let round = LendingFacts {
                place: PlaceFacts {
                    returned: &returned,
                    ..facts.place
                },
                effects: facts.effects,
                names: facts.names,
            };
            let mut next = (ReturnedPlaces::default(), LendingMethods::default());
            for statement in statements.clone() {
                table.collect_declared(statement, &round, &mut next);
            }
            if next.0 == returned && next.1 == table {
                break;
            }
            (returned, table) = next;
        }
        let definitions = facts.place.type_definitions;
        table.cap_implementations(definitions);
        table.in_body = table.settled_in_bodies(definitions, &returned);
        (returned, table)
    }

    /// The positions each lending parameter of the method `function` lends,
    /// inside its body: a call through it there keeps no argument at one.
    pub(crate) fn lent_in_body(&self, function: &FunctionId) -> HashMap<String, BTreeSet<usize>> {
        self.in_body.get(function).cloned().unwrap_or_default()
    }

    /// Every position of each lending parameter of `function`, however it
    /// lends.
    fn lent_positions(&self, function: &FunctionId) -> HashMap<String, BTreeSet<usize>> {
        (self.methods.get(function).into_iter().flatten())
            .map(|(name, lent)| (name.clone(), lent.keys().copied().collect()))
            .collect()
    }

    /// The positions each method's body may treat as lent
    /// ([`Self::lent_in_body`]). A position lent where a class settles it
    /// is lent in the body only when every class running the body settles
    /// it as lent: elsewhere a caller may hand a function keeping it, and
    /// the body is one for every class.
    fn settled_in_bodies(
        &self,
        definitions: &HashMap<String, TypeDefinition>,
        returned: &ReturnedPlaces,
    ) -> HashMap<FunctionId, HashMap<String, BTreeSet<usize>>> {
        (self.methods.iter())
            .map(|(function, parameters)| {
                let lent = (parameters.iter())
                    .map(|(name, arguments)| {
                        let positions = (arguments.iter())
                            .filter(|(_, lending)| match lending {
                                Lending::Always => true,
                                Lending::WhereReturned(methods) => {
                                    every_class_lends(definitions, returned, function, methods)
                                }
                            })
                            .map(|(position, _)| *position)
                            .collect();
                        (name.clone(), positions)
                    })
                    .collect();
                (function.clone(), lent)
            })
            .collect()
    }

    /// The lending parameters of `method` called on a receiver read at the
    /// type named `type_name`: those of the declaration the receiver's type
    /// reaches, each position lent where the receiver's class settles it.
    pub(crate) fn lending_at<'d>(
        &self,
        place: &PlaceFacts<'d>,
        (type_name, method): (&str, &str),
    ) -> Vec<LendingParameter<'d>> {
        let definitions = place.type_definitions;
        let Some((owner, info)) = declaration_reached(definitions, type_name, method) else {
            return Vec::new();
        };
        let Some(lent) = self.methods.get(&FunctionId::method(owner, method)) else {
            return Vec::new();
        };
        let is_lent_here = |lending: &Lending| match lending {
            Lending::Always => true,
            Lending::WhereReturned(methods) => {
                let returned = (place.returned).any_at(definitions, type_name, methods);
                settles_as_lent(returned, definitions, type_name)
            }
        };
        (info.params.iter().enumerate())
            .filter_map(|(position, (name, _))| {
                let arguments = lent.get(name)?;
                let lent: BTreeSet<usize> = (arguments.iter())
                    .filter(|(_, lending)| is_lent_here(lending))
                    .map(|(position, _)| *position)
                    .collect();
                Some(LendingParameter {
                    position,
                    name: name.as_str(),
                    lent,
                })
            })
            .collect()
    }

    /// Adds to `next` the lending parameters of each method among the
    /// declarations `statement` holds, reading this table for the methods
    /// their bodies hand a function-typed parameter on to.
    fn collect_declared(
        &self,
        statement: &Statement,
        facts: &LendingFacts<'_>,
        next: &mut (ReturnedPlaces, LendingMethods),
    ) {
        let (name, methods) = match &statement.node {
            StatementKind::Class(class) => (class.name.as_ref(), class.body.as_slice()),
            StatementKind::Struct(name, _, _, methods, _, _)
            | StatementKind::Enum(name, _, _, methods, _, _)
            | StatementKind::Trait(name, _, _, methods, _) => (name.as_ref(), methods.as_slice()),
            StatementKind::Block(inner) => {
                for statement in inner {
                    self.collect_declared(statement, facts, next);
                }
                return;
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
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..) => return,
        };
        let Some(owner) = facts.names.declared(name) else {
            return;
        };
        for member in methods {
            let StatementKind::FunctionDeclaration(decl) = &member.node else {
                continue;
            };
            let (lent, handed) = BodyWalk::walk(decl.as_ref(), facts, self);
            if let Some(handed) = handed {
                next.0.insert((owner, &decl.name), handed);
            }
            if !lent.is_empty() {
                (next.1.methods).insert(FunctionId::method(owner, &decl.name), lent);
            }
        }
    }

    /// Narrows each method implementing a declaration to the positions
    /// that declaration lends, until none narrows further.
    fn cap_implementations(&mut self, definitions: &HashMap<String, TypeDefinition>) {
        loop {
            let mut narrowed = false;
            let keys: Vec<FunctionId> = self.methods.keys().cloned().collect();
            for key in keys {
                let FunctionId::Method { owner, method } = &key else {
                    continue;
                };
                let Some(declared) = implemented_declaration(definitions, owner, method) else {
                    continue;
                };
                let allowed = self.lent_positions(&declared);
                let Some(lent) = self.methods.get_mut(&key) else {
                    continue;
                };
                narrowed |= narrow_to(lent, &allowed);
            }
            self.methods.retain(|_, lent| !lent.is_empty());
            if !narrowed {
                return;
            }
        }
    }
}

/// Keeps in `lent` only the positions `allowed` holds for each parameter;
/// whether any was dropped.
fn narrow_to(lent: &mut LentParameters, allowed: &HashMap<String, BTreeSet<usize>>) -> bool {
    let mut narrowed = false;
    for (name, arguments) in lent.iter_mut() {
        let before = arguments.len();
        let kept = allowed.get(name);
        arguments.retain(|position, _| kept.is_some_and(|kept| kept.contains(position)));
        narrowed |= arguments.len() != before;
    }
    lent.retain(|_, arguments| !arguments.is_empty());
    narrowed
}

/// The declaration a method `method` of the type `owner` implements: the
/// nearest base class's, or a trait's the type or a parent trait lists.
/// `None` for a method implementing nothing. An abstract declaration lends
/// nothing, as it has no entry in the table.
fn implemented_declaration(
    definitions: &HashMap<String, TypeDefinition>,
    owner: &str,
    method: &str,
) -> Option<FunctionId> {
    let found = match definitions.get(owner)? {
        TypeDefinition::Class(_) => (class_ancestry(owner, definitions).skip(1))
            .find_map(|(class, def)| def.methods.get(method).map(|_| class))
            .or_else(|| {
                inherited_trait_requirement(definitions, owner, method)
                    .map(|requirement| requirement.trait_name)
            }),
        TypeDefinition::Trait(_) => (trait_lineage(definitions, owner).skip(1))
            .find_map(|(name, def)| def.methods.get(method).map(|_| name)),
        TypeDefinition::Struct(_)
        | TypeDefinition::Enum(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_) => None,
    }?;
    Some(FunctionId::method(found, method))
}

/// The declaration a call of `method` on a receiver read at the type named
/// `type_name` is checked against: the body the class reaches, the trait's
/// own declaration for a receiver held at a trait, and the type's own
/// method otherwise. An abstract declaration reached lends nothing.
fn declaration_reached<'d>(
    definitions: &'d HashMap<String, TypeDefinition>,
    type_name: &str,
    method: &str,
) -> Option<(&'d str, &'d super::context::MethodInfo)> {
    let (owner, info) = match definitions.get_key_value(type_name)? {
        (_, TypeDefinition::Class(_)) => {
            match resolve_method_source(definitions, type_name, method)? {
                MethodSource::Declared { class, info } => (class, info),
                MethodSource::Default(default) => (default.trait_name, default.info),
                MethodSource::AbstractOnly { .. } => return None,
            }
        }
        (_, TypeDefinition::Trait(_)) => trait_lineage(definitions, type_name)
            .find_map(|(owner, def)| def.methods.get(method).map(|info| (owner, info)))?,
        (name, TypeDefinition::Struct(_) | TypeDefinition::Enum(_)) => (
            name.as_str(),
            definitions.get(type_name)?.methods()?.get(method)?,
        ),
        (_, TypeDefinition::Generic(_) | TypeDefinition::Alias(_)) => return None,
    };
    (!info.is_abstract).then_some((owner, info))
}

/// Whether the receiver's type, named `type_name`, settles a
/// [`Lending::WhereReturned`] position as lent, `returned` being whether the
/// body it reaches hands back a place: a class whose body does lends, and
/// one whose body does not owns. A receiver held at a trait or an abstract
/// class reaches bodies of either kind, so it lends.
fn settles_as_lent(
    returned: Option<Returned>,
    definitions: &HashMap<String, TypeDefinition>,
    type_name: &str,
) -> bool {
    let is_concrete_class = matches!(definitions.get(type_name),
        Some(TypeDefinition::Class(class)) if !class.is_abstract);
    returned.is_some() || !is_concrete_class
}

/// Whether every concrete class running the body of `function` gives one
/// of `methods` a body handing back a place of its receiver.
fn every_class_lends(
    definitions: &HashMap<String, TypeDefinition>,
    returned: &ReturnedPlaces,
    function: &FunctionId,
    methods: &BTreeSet<String>,
) -> bool {
    (classes_running(definitions, function))
        .all(|class| returned.any_at(definitions, class, methods).is_some())
}

/// Whether some concrete class running the body of `function` gives one of
/// `methods` a body handing back a place of its receiver; true for a body
/// that is no method, whose classes are not known.
pub(crate) fn some_class_hands_back(
    definitions: &HashMap<String, TypeDefinition>,
    returned: &ReturnedPlaces,
    function: Option<&FunctionId>,
    methods: &BTreeSet<String>,
) -> bool {
    let Some(function) = function.filter(|function| matches!(function, FunctionId::Method { .. }))
    else {
        return true;
    };
    (classes_running(definitions, function))
        .any(|class| returned.any_at(definitions, class, methods).is_some())
}

/// The concrete classes whose call of the method `function` runs its body.
fn classes_running<'d>(
    definitions: &'d HashMap<String, TypeDefinition>,
    function: &'d FunctionId,
) -> impl Iterator<Item = &'d str> {
    let (owner, method) = match function {
        FunctionId::Method { owner, method } => (Some(owner.as_str()), method.as_str()),
        FunctionId::Free(_) | FunctionId::Declared(_) => (None, ""),
    };
    (definitions.iter())
        .filter(|(_, definition)| {
            matches!(definition, TypeDefinition::Class(class) if !class.is_abstract)
        })
        .filter(move |(class, _)| {
            owner.is_some_and(|owner| {
                declaration_reached(definitions, class, method).is_some_and(|(body, _)| body == owner)
            })
        })
        .map(|(class, _)| class.as_str())
}

/// The walk of one method body inferring its lending parameters and
/// whether it hands back a place of its receiver.
struct BodyWalk<'w> {
    facts: &'w LendingFacts<'w>,
    table: &'w LendingMethods,
    /// The bindings read out of a place, in the scope walked.
    bindings: PlaceBindings,
    /// The method's function-typed parameters no binding of the scope
    /// walked shadows, each with how many arguments a call through it
    /// takes.
    lendable: HashMap<String, usize>,
    found: LentParameters,
    /// The ids of the values the body ends on and so returns.
    tails: HashSet<usize>,
    /// When a value the body returns is borrowed from a place of `self`.
    handed: Option<Handed>,
}

impl<'w> BodyWalk<'w> {
    /// The lending parameters of the method `decl`, reading `table` for the
    /// methods its body hands one on to, and when the body hands back a
    /// place of its receiver.
    fn walk(
        decl: &FunctionDeclarationData,
        facts: &'w LendingFacts<'w>,
        table: &'w LendingMethods,
    ) -> (LentParameters, Option<Handed>) {
        let Some(body) = decl.body.as_deref() else {
            return (LentParameters::new(), None);
        };
        let lendable = (decl.params.iter())
            .filter(|param| !param.is_out)
            .filter_map(|param| Some((param.name.clone(), function_arity(&param.typ)?)))
            .collect::<HashMap<_, _>>();
        let tails = (returned_tails(decl.return_type.as_deref(), body).iter())
            .map(|tail| tail.id)
            .collect();
        let mut walk = BodyWalk {
            facts,
            table,
            bindings: PlaceBindings::default(),
            lendable,
            found: LentParameters::new(),
            tails,
            handed: None,
        };
        walk.statement(body);
        (walk.found, walk.handed)
    }

    /// Records the body returning `value`: a value borrowed from a place of
    /// `self` hands that place back, at every class or at those the place
    /// depends on.
    fn returns(&mut self, value: &Expression) {
        let Some(place) = self.facts.place.borrowed_place(value, &self.bindings) else {
            return;
        };
        if !place.is_rooted_at(RECEIVER_PARAM_NAME) {
            return;
        }
        let handed = match place.per_copy() {
            Some(methods) => Handed::AnyOf(methods.clone()),
            None => Handed::Always,
        };
        self.handed = Some(match self.handed.take() {
            Some(before) => before.join(handed),
            None => handed,
        });
    }

    /// Records the argument at `position` of a call through the lending
    /// parameter `parameter` as lent, `lending` as the value handed there
    /// settles it. Lending is decided per argument position: a position
    /// some call hands a borrowed place lends at every call, and one only
    /// ever handed owned values owns. A position lent wherever the method
    /// is called outranks one lent where a class settles it.
    fn record(&mut self, parameter: &str, position: usize, lending: Lending) {
        let arguments = self.found.entry(parameter.to_string()).or_default();
        match arguments.get(&position) {
            Some(Lending::Always) => {}
            Some(Lending::WhereReturned(_)) | None => {
                arguments.insert(position, lending);
            }
        }
    }

    /// Runs `walk` in a scope of its own: what it binds and shadows ends
    /// with it.
    fn scoped(&mut self, walk: impl FnOnce(&mut Self)) {
        let mark = self.bindings.mark();
        let lendable = self.lendable.clone();
        walk(self);
        self.lendable = lendable;
        self.bindings.restore(mark);
    }

    /// `name` bound afresh in the scope walked: to `place` when it is read
    /// out of one, and never a lending parameter.
    fn bind(&mut self, name: &str, place: Option<BorrowedPlace>) {
        self.lendable.remove(name);
        match place {
            Some(place) => self.bindings.bind_borrowed(name, place),
            None => self.bindings.forget(name),
        }
    }

    fn statement(&mut self, statement: &Statement) {
        match &statement.node {
            StatementKind::Expression(value) => self.expression(value),
            StatementKind::Return(Some(value)) => {
                self.expression(value);
                self.returns(value);
            }
            StatementKind::Block(inner) => {
                self.scoped(|walk| inner.iter().for_each(|s| walk.statement(s)))
            }
            StatementKind::Variable(decls, _) => {
                for decl in decls {
                    let initializer = decl.initializer.as_deref();
                    initializer
                        .into_iter()
                        .for_each(|value| self.expression(value));
                    let place = initializer
                        .and_then(|value| self.facts.place.borrowed_place(value, &self.bindings));
                    self.bind(&decl.name, place);
                }
            }
            StatementKind::If(condition, then, else_, _) => {
                self.expression(condition);
                self.scoped(|walk| walk.statement(then));
                if let Some(else_) = else_ {
                    self.scoped(|walk| walk.statement(else_));
                }
            }
            StatementKind::While(condition, body, _) => {
                self.expression(condition);
                self.scoped(|walk| walk.statement(body));
            }
            StatementKind::For(decls, walked, body) => {
                self.expression(walked);
                let element = self.facts.place.loop_element(walked_value(walked));
                self.scoped(|walk| {
                    decls
                        .iter()
                        .for_each(|decl| walk.bind(&decl.name, element.clone()));
                    walk.statement(body);
                });
            }
            StatementKind::Forall {
                vars: decls,
                iterable: walked,
                body,
                ..
            }
            | StatementKind::GpuFrame(decls, walked, body) => {
                self.expression(walked);
                self.scoped(|walk| {
                    decls.iter().for_each(|decl| walk.bind(&decl.name, None));
                    walk.statement(body);
                });
            }
            StatementKind::GpuFrameBlock(inner) => self.statement(inner),
            StatementKind::Return(None)
            | StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::FunctionDeclaration(_)
            | StatementKind::Use(..)
            | StatementKind::Type(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..)
            | StatementKind::Class(_)
            | StatementKind::Struct(..)
            | StatementKind::Enum(..)
            | StatementKind::Trait(..) => {}
        }
    }

    fn expression(&mut self, expr: &Expression) {
        if self.tails.contains(&expr.id) {
            self.returns(expr);
        }
        match &expr.node {
            ExpressionKind::Call(callee, args) => self.call(callee, args),
            ExpressionKind::Lambda(lambda) => self.lambda(lambda),
            ExpressionKind::Match(scrutinee, branches) => {
                self.match_expression(scrutinee, branches)
            }
            ExpressionKind::Block(statements, value) => self.scoped(|walk| {
                statements
                    .iter()
                    .for_each(|statement| walk.statement(statement));
                walk.expression(value);
            }),
            ExpressionKind::Binary(left, _, right)
            | ExpressionKind::Logical(left, _, right)
            | ExpressionKind::Index(left, right)
            | ExpressionKind::Cast(left, right)
            | ExpressionKind::StructMember(left, right) => {
                self.expression(left);
                self.expression(right);
            }
            ExpressionKind::Unary(_, inner)
            | ExpressionKind::Guard(_, inner)
            | ExpressionKind::NamedArgument(_, inner)
            | ExpressionKind::Member(inner, _) => self.expression(inner),
            ExpressionKind::Assignment(target, _, value) => {
                self.expression(target.expression());
                self.expression(value);
            }
            ExpressionKind::Conditional(condition, then, else_, _) => {
                self.expression(condition);
                self.expression(then);
                else_.iter().for_each(|else_| self.expression(else_));
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start);
                end.iter().for_each(|end| self.expression(end));
            }
            ExpressionKind::EnumValue(_, values)
            | ExpressionKind::List(values)
            | ExpressionKind::Tuple(values)
            | ExpressionKind::Set(values)
            | ExpressionKind::FormattedString(values, _) => {
                values.iter().for_each(|v| self.expression(v))
            }
            ExpressionKind::Array(values, size) => {
                values.iter().for_each(|value| self.expression(value));
                self.expression(size);
            }
            ExpressionKind::Map(entries) => entries.iter().for_each(|(key, value)| {
                self.expression(key);
                self.expression(value);
            }),
            ExpressionKind::Literal(_)
            | ExpressionKind::Identifier(..)
            | ExpressionKind::ImportPath(..)
            | ExpressionKind::Type(..)
            | ExpressionKind::GenericType(..)
            | ExpressionKind::TypeDeclaration(..)
            | ExpressionKind::Super => {}
        }
    }

    /// Walks a lambda's body, whose parameters shadow the method's.
    fn lambda(&mut self, lambda: &LambdaData) {
        self.scoped(|walk| {
            lambda
                .params
                .iter()
                .for_each(|param| walk.bind(&param.name, None));
            walk.statement(&lambda.body);
        });
    }

    /// Walks a `match`, each arm binding its patterns' names out of what
    /// the scrutinee is read out of.
    fn match_expression(&mut self, scrutinee: &Expression, branches: &[MatchBranch]) {
        self.expression(scrutinee);
        let origin = (self.facts.place).scrutinee_origin(scrutinee, &self.bindings, |_| false);
        for branch in branches {
            self.scoped(|walk| {
                let mut names = std::collections::HashSet::new();
                branch
                    .patterns
                    .iter()
                    .for_each(|pattern| collect_pattern_bindings(pattern, &mut names));
                names.iter().for_each(|name| {
                    walk.lendable.remove(name);
                });
                walk.bindings.bind_arm(&names, &origin);
                branch.guard.iter().for_each(|guard| walk.expression(guard));
                walk.statement(&branch.body);
            });
        }
    }

    /// Walks a call: one through a lending parameter records each argument
    /// position it hands a borrowed place, and one handed a lending
    /// parameter records what the callee lends through it.
    fn call(&mut self, callee: &Expression, args: &[Expression]) {
        self.expression(callee);
        args.iter().for_each(|arg| self.expression(arg));
        if let ExpressionKind::Identifier(name, _) = &callee.node {
            if self.lendable.contains_key(name) {
                self.record_borrowed_arguments(name, args);
            }
            self.forget_taken_out(name, args);
        }
        for (position, arg) in args.iter().enumerate() {
            let ExpressionKind::Identifier(name, _) = &argument_value(arg).node else {
                continue;
            };
            if let Some(&arity) = self.lendable.get(name) {
                let name = name.clone();
                for lent in self.handed_on_lends(callee, position, arity) {
                    self.record(&name, lent, Lending::Always);
                }
            }
        }
    }

    /// Forgets the bindings read out of a collection the runtime entry point
    /// `entry`, called with `args`, writes while storing nothing: it takes
    /// elements out, so a binding read out of it before holds its own.
    ///
    /// TODO: an entry point writing a collection and storing nothing may also
    /// keep every element (a sort, a reverse); the declaration does not say
    /// which, so a binding read before such a call is read as owned after it.
    /// A mark stating that an entry point takes elements out would settle it.
    fn forget_taken_out(&mut self, entry: &str, args: &[Expression]) {
        let Some(effect) = self.facts.effects.get(entry) else {
            return;
        };
        let stores = (0..args.len()).any(|position| {
            matches!(
                effect.param(position),
                crate::runtime_effects::ParamUse::Stores(_)
            )
        });
        if stores {
            return;
        }
        let written = (args.iter().enumerate())
            .filter(|(position, _)| {
                effect.param(*position) == crate::runtime_effects::ParamUse::Writes
            })
            .filter_map(|(_, arg)| place_spelling(argument_value(arg)));
        for root in written.collect::<Vec<_>>() {
            self.bindings.forget_rooted_at(&root);
        }
    }

    /// Records each position of a call through the lending parameter
    /// `parameter` that `args` hands a value borrowed from a place.
    fn record_borrowed_arguments(&mut self, parameter: &str, args: &[Expression]) {
        for (position, arg) in args.iter().enumerate() {
            let place = (self.facts.place).borrowed_place(argument_value(arg), &self.bindings);
            if let Some(place) = place {
                self.record(parameter, position, lending_of(&place));
            }
        }
    }

    /// The argument positions of a function value, taking `arity`
    /// arguments, that the call to `callee` handing it at `position` lends:
    /// those a method's lending parameter there lends, or every one for a
    /// runtime entry point calling it with what its container holds.
    fn handed_on_lends(&self, callee: &Expression, position: usize, arity: usize) -> Vec<usize> {
        if let ExpressionKind::Identifier(name, _) = &callee.node {
            let calls_it = (self.facts.effects.get(name))
                .is_some_and(|effect| effect.closure_contract(position).is_some());
            return if calls_it {
                (0..arity).collect()
            } else {
                Vec::new()
            };
        }
        let ExpressionKind::Member(receiver, method) = &callee.node else {
            return Vec::new();
        };
        let ExpressionKind::Identifier(method, _) = &method.node else {
            return Vec::new();
        };
        let Some(type_name) =
            (self.facts.place.types.get(&receiver.id)).and_then(receiver_type_name)
        else {
            return Vec::new();
        };
        (self
            .table
            .lending_at(&self.facts.place, (type_name, method))
            .into_iter())
        .filter(|parameter| parameter.position == position)
        .flat_map(|parameter| parameter.lent)
        .collect()
    }
}

/// How a position handed the value read out of `place` lends.
fn lending_of(place: &BorrowedPlace) -> Lending {
    match place {
        BorrowedPlace::Returned {
            per_copy: Some(methods),
            ..
        } => Lending::WhereReturned(methods.clone()),
        BorrowedPlace::Payload { of, .. } => lending_of(of),
        BorrowedPlace::Returned { per_copy: None, .. }
        | BorrowedPlace::Optional { .. }
        | BorrowedPlace::Element { .. }
        | BorrowedPlace::LoopElement { .. }
        | BorrowedPlace::OutParameter { .. }
        | BorrowedPlace::RestoredReceiver => Lending::Always,
    }
}

/// How many arguments a call through a parameter of the written type `typ`
/// takes, when it is a function.
fn function_arity(typ: &Expression) -> Option<usize> {
    let ExpressionKind::Type(ty, _) = &typ.node else {
        return None;
    };
    let TypeKind::Function(function) = &ty.kind else {
        return None;
    };
    Some(function.params.len())
}

/// The value an argument passes: a named argument's value, or the argument.
fn argument_value(arg: &Expression) -> &Expression {
    if let ExpressionKind::NamedArgument(_, value) = &arg.node {
        return value;
    }
    arg
}
