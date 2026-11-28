// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A resource read out of a place is borrowed, never handed to a keeper.
//!
//! A place the caller does not own whole — an `out` parameter, a receiver
//! the method restores, an optional field or element, a list element, a
//! loop's element, the payload a pattern binds from one of these, and a
//! local bound to any of them — keeps holding its value after a read. Were a
//! position keeping what it is handed (a function keeping it, a method
//! keeping its receiver, a call through a function value, a trait slot
//! owning it) to take the value, the value would have two holders and its
//! hook would run where the later of them ends. So the hand-over is refused,
//! with the repair that compiles for that place: an operation that empties
//! the place hands its value over owned.
//!
//! This module names the places and spells the refusal; the consumption
//! check ([`super::use_after_move`]) classifies the value each keeping
//! position is handed, and the owned-parameter decision
//! ([`super::kept_parameters`]) refuses a body keeping its own `out`
//! parameter or restored receiver.

use std::collections::{BTreeSet, HashMap};

use crate::ast::common::MemberVisibility;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};

use super::context::TypeDefinition;
use super::emptying_methods::{emptied_by, Emptied, EmptyingMethods};
use super::escape_analysis::{EscapeNextHop, EscapeSummary, FunctionId};
use super::function_analysis::ModuleId;
use super::optional_enum::OptionalEnum;
use super::returned_places::{Returned, ReturnedPlaces};
use super::use_after_move::consumed_because_chain;
use crate::runtime_effects::{ParamUse, RuntimeEffects};

/// A place a value handed to a keeper is borrowed from.
#[derive(Debug, Clone)]
pub(crate) enum BorrowedPlace {
    /// A field or an element of optional type, written `path`, of the value
    /// written `holder`.
    Optional { path: String, holder: String },
    /// The element `path` of the collection `collection`, at `index`, which
    /// the collection's `method` takes out.
    Element {
        path: String,
        collection: String,
        index: String,
        method: String,
    },
    /// An element of the collection `collection` a `for` loop walks, which
    /// the collection's `method` takes out by index.
    LoopElement { collection: String, method: String },
    /// The payload a pattern binds from the place `scrutinee` is written as.
    Payload {
        scrutinee: String,
        of: Box<BorrowedPlace>,
    },
    /// An `out` parameter, restored to the caller's place on every exit.
    OutParameter { name: String },
    /// The receiver of a method that replaces it, restored to the caller's
    /// place on every exit.
    RestoredReceiver,
    /// The value the call written `call` hands back out of its receiver,
    /// written `receiver` ([`super::returned_places`]), which the receiver's
    /// method `taking` takes out, when its type declares one. `per_copy`
    /// names the abstract methods the call reaches on `self`, which hand
    /// back a place at some classes inheriting the body and not at others.
    Returned {
        call: String,
        receiver: String,
        taking: Option<String>,
        per_copy: Option<BTreeSet<String>>,
    },
}

impl BorrowedPlace {
    /// How the refusal names the place the value written `value` is
    /// borrowed from: the value holding it when the value is the place
    /// itself.
    fn origin(&self, value: &str) -> String {
        match self {
            BorrowedPlace::Optional { path, holder } if path == value => format!("'{holder}'"),
            BorrowedPlace::Optional { path, .. } => format!("'{path}'"),
            BorrowedPlace::Element {
                path, collection, ..
            } if path == value => format!("'{collection}'"),
            BorrowedPlace::Element { path, .. } => format!("the element '{path}'"),
            BorrowedPlace::LoopElement { collection, .. } => {
                format!("'{collection}', which the loop walks")
            }
            BorrowedPlace::Payload { scrutinee, .. } => format!("the payload of '{scrutinee}'"),
            BorrowedPlace::OutParameter { .. } | BorrowedPlace::RestoredReceiver => {
                "the caller's place, which gets it back".to_string()
            }
            BorrowedPlace::Returned { call, receiver, .. } if call == value => {
                format!("'{receiver}', which still holds it after the call")
            }
            BorrowedPlace::Returned { call, .. } => format!("'{call}'"),
        }
    }

    /// The abstract methods whose body, at the class a body compiled once
    /// per inheriting class runs on, decides whether the value is borrowed;
    /// `None` when it is borrowed at every class.
    pub(crate) fn per_copy(&self) -> Option<&BTreeSet<String>> {
        match self {
            BorrowedPlace::Returned { per_copy, .. } => per_copy.as_ref(),
            BorrowedPlace::Payload { of, .. } => of.per_copy(),
            BorrowedPlace::Optional { .. }
            | BorrowedPlace::Element { .. }
            | BorrowedPlace::LoopElement { .. }
            | BorrowedPlace::OutParameter { .. }
            | BorrowedPlace::RestoredReceiver => None,
        }
    }

    /// Whether the place the value is borrowed from is `root` or a field or
    /// an element of it.
    pub(crate) fn is_rooted_at(&self, root: &str) -> bool {
        let holder = match self {
            BorrowedPlace::Optional { holder, .. } => holder,
            BorrowedPlace::Element { collection, .. }
            | BorrowedPlace::LoopElement { collection, .. } => collection,
            BorrowedPlace::Returned { receiver, .. } => receiver,
            BorrowedPlace::Payload { of, .. } => return of.is_rooted_at(root),
            BorrowedPlace::OutParameter { .. } | BorrowedPlace::RestoredReceiver => return false,
        };
        is_spelled_under(holder, root)
    }

    /// The repair that compiles for a value borrowed from this place, in
    /// the spellings `repairs` found declared; `None` when the program
    /// declares none.
    fn repair(&self, repairs: &PlaceRepairs) -> Option<String> {
        match self {
            BorrowedPlace::Optional { path, .. } => repairs.taking_out(path),
            BorrowedPlace::Element {
                collection,
                index,
                method,
                ..
            } => Some(format!(
                "remove the element and hand that on: `match {collection}.{method}({index})` \
                 gives its arm the element, owned"
            )),
            BorrowedPlace::LoopElement { collection, method } => {
                repairs.removing_each(collection, method)
            }
            BorrowedPlace::Payload { scrutinee, of } => match of.as_ref() {
                BorrowedPlace::Optional { .. } => repairs.taking_before_matching(scrutinee),
                BorrowedPlace::Element { .. }
                | BorrowedPlace::LoopElement { .. }
                | BorrowedPlace::Payload { .. }
                | BorrowedPlace::OutParameter { .. }
                | BorrowedPlace::RestoredReceiver
                | BorrowedPlace::Returned { .. } => of.repair(repairs),
            },
            BorrowedPlace::OutParameter { name } => repairs.replacing(Some(name)),
            BorrowedPlace::RestoredReceiver => repairs.replacing(None),
            BorrowedPlace::Returned {
                call,
                receiver,
                taking,
                ..
            } => Some(copying_or_taking_out(
                call,
                receiver,
                taking.as_deref(),
                repairs.copy_trait.as_deref(),
            )),
        }
    }
}

/// The repair for a value a call hands back out of `receiver`: keep a copy
/// of it, through the copy trait `copy_trait` where one is declared, or take
/// it out with the receiver's emptying method `taking`.
fn copying_or_taking_out(
    call: &str,
    receiver: &str,
    taking: Option<&str>,
    copy_trait: Option<&str>,
) -> String {
    let copying = match copy_trait {
        Some(copy_trait) => {
            format!("keep a copy, `{call}.clone()`, when its type implements `{copy_trait}`")
        }
        None => format!("keep a copy, `{call}.clone()`, when its type declares one"),
    };
    match taking {
        Some(method) => format!(
            "take it out instead, with `{receiver}.{method}`, which hands it over owned, \
             or {copying}"
        ),
        None => format!("{copying}: '{receiver}' keeps holding the value the call hands back"),
    }
}

/// The spellings the repairs of a refusal name, found by what the program
/// and the modules it loads declare rather than by name.
#[derive(Debug, Clone, Default)]
pub(crate) struct PlaceRepairs {
    /// The enum `T?` spells.
    pub optional: Option<OptionalEnum>,
    /// The method of the optional that empties it and hands its value back,
    /// with the optional.
    pub emptying_optional: Option<(String, OptionalEnum)>,
    /// The function that puts a new value in a place and hands back the one
    /// it held.
    pub replacing: Option<ReplacingFunction>,
    /// The copy trait, whose `clone` keeps a copy of a value.
    pub copy_trait: Option<String>,
}

/// A function putting a new value in the place its `out` parameter names
/// and handing back the value the place held: a public function of a module
/// declaring a runtime entry point that overwrites an `out` place, taking
/// the parameters that entry point takes.
#[derive(Debug, Clone)]
pub(crate) struct ReplacingFunction {
    pub name: String,
    /// The path a `use` loads the declaring module by; empty for the
    /// program's own file.
    pub module: Vec<String>,
}

impl ReplacingFunction {
    /// The `use` line a program needs to call the function, as a note after
    /// a repair; empty for a function of the program's own file.
    fn import_note(&self) -> String {
        if self.module.is_empty() {
            return String::new();
        }
        format!(" (use {})", self.module.join("."))
    }
}

impl PlaceRepairs {
    /// The repair taking the optional written `path` out.
    fn taking_out(&self, path: &str) -> Option<String> {
        let (method, optional) = self.emptying_optional.as_ref()?;
        Some(format!(
            "take the value out, leaving {} behind, and hand that on: \
             `{path}.{method}()`",
            optional.absent_variant
        ))
    }

    /// The repair taking the optional written `scrutinee` out before a
    /// `match` binds its payload.
    fn taking_before_matching(&self, scrutinee: &str) -> Option<String> {
        let (method, optional) = self.emptying_optional.as_ref()?;
        Some(format!(
            "take the value out before matching, leaving {} behind: \
             `match {scrutinee}.{method}()`",
            optional.absent_variant
        ))
    }

    /// The repair removing each element of `collection` by its `method`
    /// rather than walking them.
    fn removing_each(&self, collection: &str, method: &str) -> Option<String> {
        let optional = self.optional.as_ref()?;
        Some(format!(
            "remove the elements instead of walking them: in a `while` loop, \
             `match {collection}.{method}(0)`, hand on the element of `{}`, and \
             `break` on `{}`",
            optional.present_variant, optional.absent_variant
        ))
    }

    /// The repair replacing the value of the `out` parameter `name`, or of
    /// a restored receiver when none is given.
    fn replacing(&self, name: Option<&str>) -> Option<String> {
        let replacing = self.replacing.as_ref()?;
        let (function, note) = (&replacing.name, replacing.import_note());
        Some(match name {
            Some(name) => format!(
                "give the caller a new value and hand on the old one: \
                 `{function}({name}, <new value>)`{note}"
            ),
            None => format!(
                "hand back the old value and leave a new one in self: \
                 `return {function}(self, <new value>)`{note}"
            ),
        })
    }

    /// The repairs the declarations of `modules`, each a module's
    /// top-level statements, provide: `optional` and its methods, and the
    /// copy trait, among `type_definitions`, a runtime entry point's effects
    /// among `effects`, and each function's module among `declaring`.
    pub(crate) fn declared<'s>(
        modules: impl IntoIterator<Item = &'s [Statement]>,
        (optional, type_definitions, emptying): (
            Option<&OptionalEnum>,
            &HashMap<String, TypeDefinition>,
            &EmptyingMethods,
        ),
        (effects, declaring): (&RuntimeEffects, &HashMap<usize, ModuleId>),
    ) -> Self {
        let emptying_optional = optional.and_then(|optional| {
            let method = optional_emptying_method(optional, type_definitions, emptying)?;
            Some((method.to_string(), optional.clone()))
        });
        PlaceRepairs {
            optional: optional.cloned(),
            emptying_optional,
            replacing: (modules.into_iter())
                .find_map(|module| replacing_function(module, effects, declaring)),
            copy_trait: super::context::copy_trait(type_definitions).map(str::to_string),
        }
    }
}

/// The method of `optional` that empties it whole and hands back what it
/// held ([`emptied_by`]).
fn optional_emptying_method<'d>(
    optional: &OptionalEnum,
    type_definitions: &'d HashMap<String, TypeDefinition>,
    emptying: &EmptyingMethods,
) -> Option<&'d str> {
    let Some(TypeDefinition::Enum(definition)) = type_definitions.get(&optional.name) else {
        return None;
    };
    (definition.methods.iter())
        .find(|(method, info)| {
            emptied_by(emptying, (&optional.name, method), info) == Some(&Emptied::Receiver)
        })
        .map(|(method, _)| method.as_str())
}

/// The [`ReplacingFunction`] among the top-level statements of one module.
fn replacing_function(
    module: &[Statement],
    effects: &RuntimeEffects,
    declaring: &HashMap<usize, ModuleId>,
) -> Option<ReplacingFunction> {
    let (statement, decl, _) = replacing_functions(module, effects).next()?;
    Some(ReplacingFunction {
        name: decl.name.clone(),
        module: (declaring.get(&statement.id)).map_or_else(Vec::new, |m| m.path().to_vec()),
    })
}

/// Each function a call hands back the value of a place through, putting a
/// new one in it, by name, with the position of the argument naming the
/// place: every runtime entry point among `effects` that overwrites an `out`
/// place, and every [`ReplacingFunction`] among `modules`, each a module's
/// top-level statements.
pub(crate) fn replacing_calls<'s>(
    modules: impl IntoIterator<Item = &'s [Statement]>,
    effects: &RuntimeEffects,
) -> HashMap<String, usize> {
    let entry_points = effects.iter().filter_map(|(name, effect)| {
        let position = (0..effect.params.len()).find(|&at| effect.overwrites(at))?;
        Some((name.clone(), position))
    });
    let declared: Vec<(String, usize)> = (modules.into_iter())
        .flat_map(|module| {
            replacing_functions(module, effects).map(|(_, decl, at)| (decl.name.clone(), at))
        })
        .collect();
    entry_points.chain(declared).collect()
}

/// Each public function among the top-level statements of one module that
/// takes the parameters of a runtime entry point overwriting an `out` place,
/// its own one `out` parameter where the entry point takes the place, with
/// its statement and the position of that parameter.
fn replacing_functions<'m>(
    module: &'m [Statement],
    effects: &RuntimeEffects,
) -> impl Iterator<Item = (&'m Statement, &'m FunctionDeclarationData, usize)> {
    let statements = || {
        module.iter().flat_map(|statement| {
            if let StatementKind::Block(inner) = &statement.node {
                inner.as_slice()
            } else {
                std::slice::from_ref(statement)
            }
        })
    };
    let overwriting: Vec<(usize, usize)> = statements()
        .filter_map(|statement| {
            let StatementKind::RuntimeFunctionDeclaration(_, name, _, params, ..) = &statement.node
            else {
                return None;
            };
            let effect = effects.get(name)?;
            let position = (0..params.len()).find(|&at| effect.overwrites(at))?;
            Some((params.len(), position))
        })
        .collect();
    statements().filter_map(move |statement| {
        let StatementKind::FunctionDeclaration(decl) = &statement.node else {
            return None;
        };
        let mut outs = (decl.params.iter().enumerate()).filter(|(_, param)| param.is_out);
        let (position, _) = outs.next()?;
        let takes_the_entry_points_parameters =
            outs.next().is_none() && overwriting.contains(&(decl.params.len(), position));
        let is_public = decl.properties.visibility == MemberVisibility::Public;
        (takes_the_entry_points_parameters && is_public).then_some((
            statement,
            decl.as_ref(),
            position,
        ))
    })
}

/// The refusal of handing `value`, borrowed from `place`, to `keeper`, at
/// `span`, with the repair `repairs` spell.
pub(crate) fn handed_out_of_a_place(
    value: &str,
    place: &BorrowedPlace,
    (keeper, repairs): (&str, &PlaceRepairs),
    span: Span,
) -> TypeError {
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnResourceHandedOutOfAPlace,
            message: format!(
                "'{value}' is borrowed from {}, so {keeper} cannot keep it",
                place.origin(value)
            ),
            help: place.repair(repairs),
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related: Vec::new(),
    }
}

/// Whether the place spelled `spelled` is `root` or a field or an element
/// of it.
pub(crate) fn is_spelled_under(spelled: &str, root: &str) -> bool {
    let rest = spelled.strip_prefix(root);
    rest.is_some_and(|rest| rest.is_empty() || rest.starts_with('.') || rest.starts_with('['))
}

/// `expr` as the program writes it, when it is a binding, a field path or
/// an element of one: `h.slot`, `l[0]`, `rows[i].cell`.
pub(crate) fn place_spelling(expr: &Expression) -> Option<String> {
    if let ExpressionKind::Member(base, field) = &expr.node {
        let ExpressionKind::Identifier(field, _) = &field.node else {
            return None;
        };
        return Some(format!("{}.{field}", place_spelling(base)?));
    }
    if let ExpressionKind::Index(base, index) = &expr.node {
        return Some(format!(
            "{}[{}]",
            place_spelling(base)?,
            index_spelling(index)
        ));
    }
    if matches!(expr.node, ExpressionKind::Super) {
        return Some(crate::ast::statement::RECEIVER_PARAM_NAME.to_string());
    }
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        return None;
    };
    Some(name.clone())
}

/// `expr` as a refusal names it: a place as [`place_spelling`] writes it,
/// or a method call on one, `l.element_at(i)`; `value` for any other.
pub(crate) fn value_spelling(expr: &Expression) -> String {
    if let Some(spelled) = place_spelling(expr) {
        return spelled;
    }
    let ExpressionKind::Call(callee, args) = &expr.node else {
        return "value".to_string();
    };
    let ExpressionKind::Member(receiver, method) = &callee.node else {
        return "value".to_string();
    };
    let ExpressionKind::Identifier(method, _) = &method.node else {
        return "value".to_string();
    };
    let args: Vec<String> = args.iter().map(index_spelling).collect();
    format!("{}.{method}({})", value_spelling(receiver), args.join(", "))
}

/// An index as the program writes it: a literal, a binding or a field path;
/// `i` stands for any other expression.
pub(crate) fn index_spelling(index: &Expression) -> String {
    if let ExpressionKind::Literal(literal) = &index.node {
        return literal.to_string();
    }
    place_spelling(index).unwrap_or_else(|| "i".to_string())
}

/// The method of the collection type `collection` that takes an element
/// out by its index and hands it back ([`emptied_by`]). `None` when the type
/// has none, so no repair taking an element out compiles.
pub(crate) fn emptying_method<'d>(
    collection: &TypeKind,
    type_definitions: &'d HashMap<String, TypeDefinition>,
    emptying: &EmptyingMethods,
) -> Option<&'d str> {
    let TypeKind::Custom(name, _) = collection else {
        return None;
    };
    let Some(TypeDefinition::Class(class)) = type_definitions.get(name) else {
        return None;
    };
    (class.methods.iter())
        .find(|(method, info)| {
            matches!(
                emptied_by(emptying, (name, method), info),
                Some(Emptied::Element { .. })
            )
        })
        .map(|(method, _)| method.as_str())
}

/// A scrutinee the scope owns: a local, a whole parameter, or a temporary.
/// Keeping a payload a pattern binds out of it consumes it, by `key` in the
/// consumption state, and ends it where the payload is handed over.
#[derive(Debug, Clone)]
pub(crate) struct OwnedScrutinee {
    pub key: String,
    /// The scrutinee expression, which MIR lowering ends at the hand-over.
    pub expression: usize,
    /// The payload bindings of this scrutinee matched again on the way to
    /// the payload, outermost first: a keeper taking the payload consumes
    /// each one with the scrutinee.
    pub rematched: Vec<Rematched>,
}

/// A payload binding of an owned scrutinee that a `match` matches again.
#[derive(Debug, Clone)]
pub(crate) struct Rematched {
    pub name: String,
    /// The subject expression of the inner `match`, which MIR lowering
    /// matches in the binding itself rather than in a copy of it.
    pub subject: usize,
}

impl OwnedScrutinee {
    /// The same scrutinee, reached through its payload binding `name`
    /// matched again by the `match` whose subject is `subject`.
    fn matched_again(&self, name: &str, subject: usize) -> Self {
        let mut rematched = self.rematched.clone();
        rematched.push(Rematched {
            name: name.to_string(),
            subject,
        });
        Self {
            key: self.key.clone(),
            expression: self.expression,
            rematched,
        }
    }
}

/// What the bindings a `match` arm's patterns bind are read out of.
pub(crate) enum ScrutineeOrigin {
    /// A place the scope does not own whole.
    Borrowed(BorrowedPlace),
    /// A value the scope owns.
    Owned(OwnedScrutinee),
    /// A value neither: a plain field, an element of a collection offering
    /// no way to take it out.
    Unknown,
}

/// The bindings of the scope analysed that are read out of a place: those
/// borrowed from one, and the payloads of a scrutinee the scope owns.
///
/// A scope entered takes a [`PlacesMark`] and restores it on leaving: each
/// change records the binding's state before it, so leaving a scope undoes
/// what it bound rather than copying the whole table on entry.
#[derive(Debug, Default)]
pub(crate) struct PlaceBindings {
    borrowed: HashMap<String, BorrowedPlace>,
    payloads: HashMap<String, OwnedScrutinee>,
    /// Each changed binding's state before the change, the latest last.
    undo: Vec<(String, Option<BorrowedPlace>, Option<OwnedScrutinee>)>,
}

/// How many changes [`PlaceBindings`] had recorded where a scope began.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacesMark(usize);

impl PlaceBindings {
    /// The point a scope entered now restores on leaving.
    pub(crate) fn mark(&self) -> PlacesMark {
        PlacesMark(self.undo.len())
    }

    /// Undoes every change made since `mark`, the latest first.
    pub(crate) fn restore(&mut self, mark: PlacesMark) {
        while self.undo.len() > mark.0 {
            let Some((name, borrowed, payload)) = self.undo.pop() else {
                return;
            };
            match borrowed {
                Some(place) => self.borrowed.insert(name.clone(), place),
                None => self.borrowed.remove(&name),
            };
            match payload {
                Some(owner) => self.payloads.insert(name, owner),
                None => self.payloads.remove(&name),
            };
        }
    }

    /// `name` bound afresh, to no place.
    pub(crate) fn forget(&mut self, name: &str) {
        let borrowed = self.borrowed.remove(name);
        let payload = self.payloads.remove(name);
        self.undo.push((name.to_string(), borrowed, payload));
    }

    /// Every binding borrowed from `root`, or a field or an element of it,
    /// bound afresh to no place: what it was read out of no longer holds it.
    pub(crate) fn forget_rooted_at(&mut self, root: &str) {
        let rooted: Vec<String> = (self.borrowed.iter())
            .filter(|(_, place)| place.is_rooted_at(root))
            .map(|(name, _)| name.clone())
            .collect();
        for name in rooted {
            self.forget(&name);
        }
    }

    /// `name` bound to a value borrowed from `place`.
    pub(crate) fn bind_borrowed(&mut self, name: &str, place: BorrowedPlace) {
        self.forget(name);
        self.borrowed.insert(name.to_string(), place);
    }

    /// Each of `names`, bound by an arm's patterns out of `origin`.
    pub(crate) fn bind_arm<'n>(
        &mut self,
        names: impl IntoIterator<Item = &'n String>,
        origin: &ScrutineeOrigin,
    ) {
        for name in names {
            self.forget(name);
            match origin {
                ScrutineeOrigin::Borrowed(place) => {
                    self.borrowed.insert(name.clone(), place.clone());
                }
                ScrutineeOrigin::Owned(owner) => {
                    self.payloads.insert(name.clone(), owner.clone());
                }
                ScrutineeOrigin::Unknown => {}
            }
        }
    }

    /// The scrutinee the scope owns that `name` is a payload of.
    pub(crate) fn payload_owner(&self, name: &str) -> Option<&OwnedScrutinee> {
        self.payloads.get(name)
    }
}

/// What classifying a read reads of the checked program.
#[derive(Clone, Copy)]
pub(crate) struct PlaceFacts<'a> {
    pub types: &'a HashMap<usize, Type>,
    pub type_definitions: &'a HashMap<String, TypeDefinition>,
    /// The methods taking a value out of their receiver.
    pub emptying: &'a EmptyingMethods,
    /// The methods handing back a place of their receiver.
    pub returned: &'a ReturnedPlaces,
    /// What each runtime entry point does to its arguments.
    pub effects: &'a RuntimeEffects,
}

impl PlaceFacts<'_> {
    /// The place `value` is borrowed from: a binding of `bindings` borrowed
    /// from one, an optional field or element, an element of a collection
    /// offering a way to take it out, or what a call hands back out of a
    /// receiver still holding it. `None` for any other value, a plain field
    /// among them.
    pub(crate) fn borrowed_place(
        &self,
        value: &Expression,
        bindings: &PlaceBindings,
    ) -> Option<BorrowedPlace> {
        if let ExpressionKind::Identifier(name, _) = &value.node {
            return bindings.borrowed.get(name).cloned();
        }
        if let ExpressionKind::Call(callee, args) = &value.node {
            if let ExpressionKind::Identifier(entry, _) = &callee.node {
                return self.element_handed_back(value, (entry, args), bindings);
            }
            return self.returned_place(value, callee, bindings);
        }
        if !is_place_read(value) {
            return None;
        }
        if let Some(TypeKind::Option(_)) = self.types.get(&value.id).map(|ty| &ty.kind) {
            let (ExpressionKind::Member(holder, _) | ExpressionKind::Index(holder, _)) =
                &value.node
            else {
                return None;
            };
            return Some(BorrowedPlace::Optional {
                path: place_spelling(value)?,
                holder: place_spelling(holder)?,
            });
        }
        let ExpressionKind::Index(collection, index) = &value.node else {
            return None;
        };
        let method = self.emptying_method_of(collection)?;
        Some(BorrowedPlace::Element {
            path: place_spelling(value)?,
            collection: place_spelling(collection)?,
            index: index_spelling(index),
            method,
        })
    }

    /// The place the call `value`, calling `callee`, hands back: what a
    /// method handing back a place of its receiver returns, when the
    /// receiver is a place or a value borrowed from one, which keeps
    /// holding it after the call.
    fn returned_place(
        &self,
        value: &Expression,
        callee: &Expression,
        bindings: &PlaceBindings,
    ) -> Option<BorrowedPlace> {
        let ExpressionKind::Member(receiver, _) = &callee.node else {
            return None;
        };
        let is_held =
            place_spelling(receiver).is_some() || self.borrowed_place(receiver, bindings).is_some();
        if !is_held {
            return None;
        }
        let returned = (self.returned).of_call(value, (self.types, self.type_definitions))?;
        Some(BorrowedPlace::Returned {
            call: value_spelling(value),
            receiver: value_spelling(receiver),
            taking: self.emptying_method_of(receiver),
            per_copy: match returned {
                Returned::Always => None,
                Returned::PerCopy(methods) => Some(methods),
            },
        })
    }

    /// The place the call `value` of the runtime entry point `entry` hands
    /// back: an element of a collection it only reads, among `args`, which
    /// is a place or a value borrowed from one and keeps holding the element.
    fn element_handed_back(
        &self,
        value: &Expression,
        (entry, args): (&str, &[Expression]),
        bindings: &PlaceBindings,
    ) -> Option<BorrowedPlace> {
        let effect = self.effects.get(entry)?;
        if !effect.returns_element_value() {
            return None;
        }
        let held = (args.iter().enumerate())
            .filter(|(position, _)| effect.param(*position) == ParamUse::Reads)
            .map(|(_, arg)| arg)
            .find(|arg| {
                place_spelling(arg).is_some() || self.borrowed_place(arg, bindings).is_some()
            })?;
        Some(BorrowedPlace::Returned {
            call: value_spelling(value),
            receiver: value_spelling(held),
            taking: None,
            per_copy: None,
        })
    }

    /// The place each element a `for` loop walking `walked` binds is
    /// borrowed from, when the collection offers a way to take one out.
    pub(crate) fn loop_element(&self, walked: &Expression) -> Option<BorrowedPlace> {
        let method = self.emptying_method_of(walked)?;
        let collection = place_spelling(walked).unwrap_or_else(|| "list".to_string());
        Some(BorrowedPlace::LoopElement { collection, method })
    }

    /// What the payloads a `match` on `scrutinee` binds are read out of; a
    /// name `is_bound` says the scope binds is a value it owns.
    ///
    /// TODO: missing is moving an optional or an enum that holds a resource.
    /// Such a value runs no hook of its own, so MIR lowering copies it where
    /// the scope hands it on rather than moving it, and never owns a
    /// parameter of its type. A scrutinee bound from another binding of one
    /// (`let alias = o`), or a parameter of one, therefore shares the payload
    /// with another holder, and ending the scrutinee at the keep leaves the
    /// payload's hook to that holder; the same sharing lets a payload reach
    /// an owning trait slot as a second holder. The keeper holds the
    /// payload's only reference once lowering moves such a value and owns
    /// such a parameter as it does a resource with a hook.
    pub(crate) fn scrutinee_origin(
        &self,
        scrutinee: &Expression,
        bindings: &PlaceBindings,
        is_bound: impl Fn(&str) -> bool,
    ) -> ScrutineeOrigin {
        if let Some(place) = self.borrowed_place(scrutinee, bindings) {
            let spelled = place_spelling(scrutinee).unwrap_or_default();
            return ScrutineeOrigin::Borrowed(BorrowedPlace::Payload {
                scrutinee: spelled,
                of: Box::new(place),
            });
        }
        if is_place_read(scrutinee) {
            return ScrutineeOrigin::Unknown;
        }
        let owner = |key: String| {
            ScrutineeOrigin::Owned(OwnedScrutinee {
                key,
                expression: scrutinee.id,
                rematched: Vec::new(),
            })
        };
        let ExpressionKind::Identifier(name, _) = &scrutinee.node else {
            return owner(format!("{TEMPORARY_SCRUTINEE}{}", scrutinee.id));
        };
        if let Some(outer) = bindings.payloads.get(name) {
            return ScrutineeOrigin::Owned(outer.matched_again(name, scrutinee.id));
        }
        if !is_bound(name) {
            return ScrutineeOrigin::Unknown;
        }
        owner(name.clone())
    }

    /// [`emptying_method`] of the type `collection` is read at.
    fn emptying_method_of(&self, collection: &Expression) -> Option<String> {
        let ty = self.types.get(&collection.id)?;
        emptying_method(&ty.kind, self.type_definitions, self.emptying).map(str::to_string)
    }
}

/// The prefix of the key a temporary scrutinee is consumed under: no
/// binding is spelled with it.
const TEMPORARY_SCRUTINEE: &str = "match#";

/// Whether `value` reads a field or an element.
fn is_place_read(value: &Expression) -> bool {
    matches!(
        value.node,
        ExpressionKind::Member(..) | ExpressionKind::Index(..)
    )
}

/// A position of a body that holds a place of its caller's, restored to the
/// caller on every exit.
pub(crate) enum CallerPlace<'d> {
    /// The `out` parameter `name`, at `slot` of the body's escape summary.
    OutParameter { slot: usize, name: &'d str },
    /// The receiver of a method that replaces it, at slot 0.
    RestoredReceiver,
}

impl CallerPlace<'_> {
    /// The slot of the body's escape summary the place is held at.
    fn slot(&self) -> usize {
        match self {
            CallerPlace::OutParameter { slot, .. } => *slot,
            CallerPlace::RestoredReceiver => 0,
        }
    }

    /// The name the body reads the place by.
    fn name(&self) -> &str {
        match self {
            CallerPlace::OutParameter { name, .. } => name,
            CallerPlace::RestoredReceiver => crate::ast::statement::RECEIVER_PARAM_NAME,
        }
    }

    /// The place, as a refusal of handing a value out of it names it.
    fn borrowed(&self) -> BorrowedPlace {
        match self {
            CallerPlace::OutParameter { name, .. } => BorrowedPlace::OutParameter {
                name: name.to_string(),
            },
            CallerPlace::RestoredReceiver => BorrowedPlace::RestoredReceiver,
        }
    }
}

/// The refusal of the body `function` keeping the place `place`, at the
/// first place it keeps it, with the chain of callees it passes through;
/// `None` when its escape summary keeps no such position. A place outside
/// the program's own file is not one a report can point at, so the refusal
/// points at `declared` instead.
pub(crate) fn kept_place_refusal(
    function: &FunctionId,
    place: &CallerPlace<'_>,
    (summaries, repairs): (&HashMap<FunctionId, EscapeSummary>, &PlaceRepairs),
    declared: Span,
) -> Option<TypeError> {
    let summary = summaries.get(function)?;
    let slot = place.slot();
    if !summary.directly_escapes(slot) {
        return None;
    }
    let site = summary.escape_next_hops.get(&slot);
    let span = site.and_then(|site| site.span).unwrap_or(declared);
    let keeper = site.map_or_else(|| format!("'{function}'"), |site| keeper_of(&site.hop));
    let borrowed = place.borrowed();
    let mut error = handed_out_of_a_place(place.name(), &borrowed, (&keeper, repairs), span);
    error.related = consumed_because_chain(summaries, function, slot);
    Some(error)
}

/// How a refusal names what keeps a value at the step `hop`.
fn keeper_of(hop: &EscapeNextHop) -> String {
    match hop {
        EscapeNextHop::Call { callee, .. } => format!("'{callee}'"),
        EscapeNextHop::Return | EscapeNextHop::ReturnAggregate => "the value returned".to_string(),
        EscapeNextHop::FieldStore { field } => format!("the field '{field}'"),
        EscapeNextHop::ElementStore => "the element it is stored in".to_string(),
        EscapeNextHop::Built => "the value built of it".to_string(),
        EscapeNextHop::Bound { name } => format!("the local '{name}'"),
        EscapeNextHop::ClosureCapture { .. } => "the closure capturing it".to_string(),
        EscapeNextHop::DynamicFn { fn_name } => format!("dynamic fn '{fn_name}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_returned_repair_without_a_copy_trait_or_a_taking_method() {
        let definitions = HashMap::new();
        let copy_trait = crate::type_checker::context::copy_trait(&definitions);
        let repair = copying_or_taking_out("box.get()", "box", None, copy_trait);
        assert_eq!(
            repair,
            "keep a copy, `box.get().clone()`, when its type declares one: 'box' keeps \
             holding the value the call hands back"
        );
    }

    #[test]
    fn test_returned_repair_without_a_copy_trait_with_a_taking_method() {
        let definitions = HashMap::new();
        let copy_trait = crate::type_checker::context::copy_trait(&definitions);
        let repair = copying_or_taking_out("box.get()", "box", Some("take()"), copy_trait);
        assert_eq!(
            repair,
            "take it out instead, with `box.take()`, which hands it over owned, or keep a \
             copy, `box.get().clone()`, when its type declares one"
        );
    }
}
