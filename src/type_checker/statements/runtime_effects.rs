// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading what a `runtime` declaration states its entry point does, and
//! holding its closure parameters to it.
//!
//! The runtime has no body the compiler could read, so its declaration
//! states, as attributes, what it does with each parameter that holds a
//! reference and what it hands back. A declaration that leaves one of those
//! unstated is refused: a fact assumed for a missing statement is how a
//! stored argument ends up released while the runtime still holds it.
//!
//! The runtime calls a closure it is handed through a trampoline the compiler
//! generates from the closure's declared type, and calls it in exactly one
//! shape: the arity, the `int` it reads the answer as, and the collection
//! whose element slots it passes, which the declaration's `@closure` states.
//! A declared closure type that promises another shape would have the
//! trampoline read past an element slot or write past the answer, so it is
//! refused where it is written.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::common::Parameter;
use crate::ast::expression::Expression;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::ast::{
    Attribute, ALLOCATES_ATTRIBUTE, CLOSURE_ATTRIBUTE, COPIES_ATTRIBUTE, DIVERGES_ATTRIBUTE,
    ELEMENT_ATTRIBUTE, KEY_ATTRIBUTE, MATCHES_ATTRIBUTE, ORDERS_ATTRIBUTE, READS_ATTRIBUTE,
    RETURNS_ELEMENT_ATTRIBUTE, STORES_ATTRIBUTE, VALUE_ATTRIBUTE, WRITES_ATTRIBUTE,
};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::runtime_effects::{ClosureContract, ElementRole, ParamUse, ResultUse, RuntimeEffect};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::{resolve_type_name, TypeChecker};

/// A `runtime` declaration as the type checker reads its effects from it.
pub(crate) struct RuntimeDeclaration<'a> {
    pub name: &'a str,
    pub params: &'a [Parameter],
    pub return_type: Option<&'a Expression>,
    pub attributes: &'a [Attribute],
    pub span: Span,
}

/// One refusal: what is wrong, and where.
type Refusal = (String, Span);

/// The attributes that state a runtime entry point's effects.
#[derive(Debug, Clone, Copy)]
enum Stated {
    Reads,
    Writes,
    Element(ElementRole),
    Stores,
    Closure,
    Orders,
    Matches,
    Copies,
    ReturnsElement,
    Allocates,
    Diverges,
}

impl Stated {
    const ALL: [(&'static str, Stated); 13] = [
        (READS_ATTRIBUTE, Stated::Reads),
        (WRITES_ATTRIBUTE, Stated::Writes),
        (ELEMENT_ATTRIBUTE, Stated::Element(ElementRole::Element)),
        (KEY_ATTRIBUTE, Stated::Element(ElementRole::Key)),
        (VALUE_ATTRIBUTE, Stated::Element(ElementRole::Value)),
        (STORES_ATTRIBUTE, Stated::Stores),
        (CLOSURE_ATTRIBUTE, Stated::Closure),
        (ORDERS_ATTRIBUTE, Stated::Orders),
        (MATCHES_ATTRIBUTE, Stated::Matches),
        (COPIES_ATTRIBUTE, Stated::Copies),
        (RETURNS_ELEMENT_ATTRIBUTE, Stated::ReturnsElement),
        (ALLOCATES_ATTRIBUTE, Stated::Allocates),
        (DIVERGES_ATTRIBUTE, Stated::Diverges),
    ];

    /// The effect attribute spelled `name`; `None` for any other attribute,
    /// which the attribute registry has already judged.
    fn named(name: &str) -> Option<(&'static str, Stated)> {
        Self::ALL
            .into_iter()
            .find(|(spelling, _)| *spelling == name)
    }
}

impl TypeChecker {
    /// Record what the runtime declaration `declaration` states its entry
    /// point does, refusing every statement it is missing or gets wrong, and
    /// refuse each closure parameter whose declared type breaks the stated
    /// contract.
    pub(crate) fn check_runtime_effects(&mut self, declaration: &RuntimeDeclaration) {
        let (effect, refusals) = stated_effect(declaration, self.type_definitions());
        let refused = !refusals.is_empty();
        for (message, span) in refusals {
            self.report_error(DiagnosticCode::TypFunctionSignature, message, span);
        }
        let name = declaration.name;
        match self.runtime_effects.get(name) {
            // A refused declaration states no record to compare.
            _ if refused => {}
            Some(recorded) if **recorded != effect => self.report_error(
                DiagnosticCode::TypFunctionSignature,
                format!("`{name}` is declared twice with different effects"),
                declaration.span,
            ),
            Some(_) => {}
            None => {
                self.runtime_effects
                    .insert(name.to_string(), Rc::new(effect.clone()));
            }
        }
        self.check_runtime_closure_params(name, declaration.params, &effect);
    }

    /// Refuse each parameter of the runtime function `name` whose declared
    /// type disagrees with the closure `effect` says the runtime calls
    /// through it, or calls none through.
    fn check_runtime_closure_params(
        &mut self,
        name: &str,
        params: &[Parameter],
        effect: &RuntimeEffect,
    ) {
        for (position, param) in params.iter().enumerate() {
            let function = resolve_type_name(&param.typ).and_then(|ty| {
                if let TypeKind::Function(data) = ty.kind {
                    Some(*data)
                } else {
                    None
                }
            });
            let refusal = match (effect.closure_contract(position), function) {
                (None, None) => None,
                (None, Some(_)) => Some(format!(
                    "`{name}` calls no closure through `{}`, so it cannot be declared as a function",
                    param.name
                )),
                (Some(_), None) => Some(format!(
                    "`{name}` calls `{}` as a closure, so it must be declared as a function",
                    param.name
                )),
                (Some(contract), Some(function)) => {
                    closure_refusal(name, params, (param, &function), contract)
                }
            };
            if let Some(message) = refusal {
                self.report_error(
                    DiagnosticCode::TypFunctionSignature,
                    message,
                    param.typ.span,
                );
            }
        }
    }
}

/// What `declaration` states its entry point does, with a refusal for each
/// statement that is missing, names no parameter, or contradicts another.
fn stated_effect(
    declaration: &RuntimeDeclaration,
    definitions: &HashMap<String, TypeDefinition>,
) -> (RuntimeEffect, Vec<Refusal>) {
    let mut statements = Statements::new(declaration);
    for attribute in declaration.attributes {
        statements.read(attribute);
    }
    let params = (0..declaration.params.len())
        .map(|position| statements.param_use(position))
        .collect();
    let mut effect = RuntimeEffect::new(params);
    effect.result = statements.result(definitions);
    effect.diverges = statements.diverges;
    effect.orders = statements.orders;
    effect.matches = statements.matches;
    (effect, statements.refusals)
}

/// The statements one declaration's attributes make, gathered per parameter
/// before they are checked against each other.
struct Statements<'a> {
    declaration: &'a RuntimeDeclaration<'a>,
    /// Each parameter's stated uses, with the attribute that stated each.
    uses: Vec<Vec<(&'static str, ParamUse)>>,
    stores: Vec<bool>,
    results: Vec<(&'static str, ResultUse)>,
    diverges: bool,
    orders: Option<usize>,
    matches: Option<usize>,
    refusals: Vec<Refusal>,
}

impl<'a> Statements<'a> {
    fn new(declaration: &'a RuntimeDeclaration<'a>) -> Self {
        let count = declaration.params.len();
        Statements {
            declaration,
            uses: vec![Vec::new(); count],
            stores: vec![false; count],
            results: Vec::new(),
            diverges: false,
            orders: None,
            matches: None,
            refusals: Vec::new(),
        }
    }

    fn read(&mut self, attribute: &Attribute) {
        let Some((spelling, stated)) = Stated::named(&attribute.name) else {
            return;
        };
        let argument = attribute.argument.as_deref().unwrap_or_default();
        let span = attribute.span;
        match stated {
            Stated::Reads => self.state(spelling, argument, span, ParamUse::Reads),
            Stated::Writes => self.state(spelling, argument, span, ParamUse::Writes),
            Stated::Element(role) => self.state(spelling, argument, span, ParamUse::LooksUp(role)),
            Stated::Stores => {
                for position in self.positions(argument, span) {
                    self.stores[position] = true;
                }
            }
            Stated::Closure => self.state_closure(argument, span),
            Stated::Orders => self.orders = self.position(argument, span),
            Stated::Matches => self.matches = self.position(argument, span),
            Stated::Copies => {
                if let Some(position) = self.position(argument, span) {
                    self.results.push((spelling, ResultUse::Copies(position)));
                }
            }
            Stated::ReturnsElement => self.results.push((spelling, ResultUse::Element)),
            Stated::Allocates => self.results.push((spelling, ResultUse::Allocates)),
            Stated::Diverges => self.diverges = true,
        }
    }

    fn state(&mut self, attribute: &'static str, argument: &str, span: Span, use_: ParamUse) {
        for position in self.positions(argument, span) {
            self.uses[position].push((attribute, use_));
        }
    }

    /// `@closure("f, 2, l")`: the closure parameter, the arity the runtime
    /// calls it with, and the collection whose elements it passes, or `none`.
    fn state_closure(&mut self, argument: &str, span: Span) {
        let parts: Vec<&str> = argument.split(',').map(str::trim).collect();
        let [closure, arity, elements_of] = parts.as_slice() else {
            return self.refuse(
                format!("`@closure` takes a parameter, an arity and a collection or `none`, not `{argument}`"),
                span,
            );
        };
        let Ok(arity) = arity.parse::<usize>() else {
            return self.refuse(format!("`@closure` arity `{arity}` is not a count"), span);
        };
        let elements_of = match *elements_of {
            "none" => None,
            collection => match self.position(collection, span) {
                Some(position) => Some(position),
                None => return,
            },
        };
        let contract = ClosureContract { arity, elements_of };
        self.state(
            CLOSURE_ATTRIBUTE,
            closure,
            span,
            ParamUse::Closure(contract),
        );
    }

    fn positions(&mut self, argument: &str, span: Span) -> Vec<usize> {
        (argument.split(',').map(str::trim))
            .filter_map(|name| self.position(name, span))
            .collect()
    }

    fn position(&mut self, name: &str, span: Span) -> Option<usize> {
        let found = (self.declaration.params.iter()).position(|param| param.name == name);
        if found.is_none() {
            let declared = self.declaration.name;
            self.refuse(format!("`{declared}` has no parameter `{name}`"), span);
        }
        found
    }

    fn refuse(&mut self, message: String, span: Span) {
        self.refusals.push((message, span));
    }

    /// The one use stated for the parameter at `position`, refusing none
    /// stated for a parameter that holds a reference, two stated, and
    /// `@stores` on a parameter that is no element.
    fn param_use(&mut self, position: usize) -> ParamUse {
        let param = &self.declaration.params[position];
        let declared = self.declaration.name;
        let stated = self.uses[position].clone();
        let use_ = match stated.as_slice() {
            [] if holds_no_reference(param) => ParamUse::Scalar,
            [] => {
                self.refuse(
                    format!("`{declared}` states nothing about what it does with `{}`: name it in `@reads`, `@writes`, `@element`, `@key`, `@value` or `@closure`", param.name),
                    self.param_span(position),
                );
                ParamUse::Writes
            }
            [(_, use_)] => *use_,
            [(first, _), (second, _), ..] => {
                self.refuse(
                    format!(
                        "`{declared}` states both `@{first}` and `@{second}` for `{}`",
                        param.name
                    ),
                    self.param_span(position),
                );
                ParamUse::Writes
            }
        };
        self.stored(position, use_)
    }

    /// Where the parameter at `position` is written: its name, or the whole
    /// declaration for a parameter with no source text of its own.
    fn param_span(&self, position: usize) -> Span {
        let param = &self.declaration.params[position];
        if param.name_span == Span::default() {
            self.declaration.span
        } else {
            param.name_span
        }
    }

    /// `use_` with `@stores` applied: a looked-up element becomes a stored
    /// one; any other use cannot be stored.
    fn stored(&mut self, position: usize, use_: ParamUse) -> ParamUse {
        if !self.stores[position] {
            return use_;
        }
        if let ParamUse::LooksUp(role) = use_ {
            return ParamUse::Stores(role);
        }
        let param = &self.declaration.params[position].name;
        self.refuse(
            format!("`@stores` names `{param}`, which is not an `@element`, `@key` or `@value`"),
            self.declaration.span,
        );
        use_
    }

    /// The one result stated, refusing two, and none for a result whose
    /// type is `Self` or a type parameter.
    fn result(&mut self, definitions: &HashMap<String, TypeDefinition>) -> ResultUse {
        let declared = self.declaration.name;
        match self.results.clone().as_slice() {
            [] => {
                if self.result_needs_statement(definitions) {
                    self.refuse(
                        format!("`{declared}` states nothing about what it hands back: add `@returns_element`, `@copies` or `@allocates`"),
                        self.declaration.span,
                    );
                }
                ResultUse::Owned
            }
            [(_, result)] => *result,
            [(first, result), (second, _), ..] => {
                self.refuse(
                    format!("`{declared}` states both `@{first}` and `@{second}`"),
                    self.declaration.span,
                );
                *result
            }
        }
    }

    /// Whether the declared result is `Self` or a type parameter, which says
    /// nothing about whether the caller owns it.
    fn result_needs_statement(&self, definitions: &HashMap<String, TypeDefinition>) -> bool {
        (self.declaration.return_type)
            .and_then(resolve_type_name)
            .is_some_and(|ty| is_open_type(&ty, definitions))
    }
}

/// Whether `param` is a plain word: a number, a flag or a raw pointer.
fn holds_no_reference(param: &Parameter) -> bool {
    resolve_type_name(&param.typ).is_some_and(|ty| ty.kind.is_copy())
}

/// Whether `ty` is `Self` or a bare name no definition gives: a type
/// parameter of the declaring class.
fn is_open_type(ty: &Type, definitions: &HashMap<String, TypeDefinition>) -> bool {
    matches!(&ty.kind, TypeKind::Custom(name, None) if name == "Self" || !definitions.contains_key(name))
}

/// Why the closure parameter `param`, declared as `function`, breaks the
/// runtime's `contract` for it; `None` when it keeps it.
fn closure_refusal(
    name: &str,
    params: &[Parameter],
    (param, function): (&Parameter, &FunctionTypeData),
    contract: ClosureContract,
) -> Option<String> {
    let closure = &param.name;
    if function.params.len() != contract.arity {
        return Some(format!(
            "`{name}` calls `{closure}` with {} arguments, but `{closure}` is declared to take {}",
            contract.arity,
            function.params.len()
        ));
    }
    let answer = function.return_type.as_deref().and_then(resolve_type_name);
    if !answer.as_ref().is_some_and(|ty| ty.kind == TypeKind::Int) {
        let declared = answer.map_or_else(|| "nothing".to_string(), |ty| ty.to_string());
        return Some(format!(
            "`{name}` reads what `{closure}` answers as an `int`, but `{closure}` is declared to answer `{declared}`"
        ));
    }
    let collection = params.get(contract.elements_of?)?;
    let element =
        resolve_type_name(&collection.typ).and_then(|ty| ty.kind.sequence_element_kind().cloned());
    let mismatch = function
        .params
        .iter()
        .filter_map(|argument| resolve_type_name(&argument.typ))
        .find(|argument| element.as_ref() != Some(&argument.kind))?;
    Some(element_refusal(
        name,
        (closure, &mismatch),
        collection,
        element,
    ))
}

/// The refusal of a closure `closure` taking `argument` where the runtime
/// passes it elements of `collection`, which hold `element`.
fn element_refusal(
    name: &str,
    (closure, argument): (&str, &Type),
    collection: &Parameter,
    element: Option<TypeKind>,
) -> String {
    let holds = element.map_or_else(
        || "not a sequence's elements".to_string(),
        |kind| format!("`{kind}`"),
    );
    format!(
        "`{name}` calls `{closure}` with elements of `{}`, which are {holds}, but `{closure}` takes `{argument}`",
        collection.name
    )
}
