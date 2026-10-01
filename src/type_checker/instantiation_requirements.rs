// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which of its own generic parameters a body places requirements on, and the
//! check that applies those requirements where a parameter is pinned to a
//! concrete type.
//!
//! A generic body is checked once, against its own parameters. An operation
//! written there on a value of a parameter type has no type to ask yet, so the
//! check is deferred: the body records what it needs of the parameter, and
//! every site that pins it — a call to a generic function, a method reached
//! through a generic receiver — answers for the type it pins it to. Without
//! the deferral the operation would reach code generation with nothing but the
//! operands' bytes to work on.
//!
//! Bodies are checked in source order, so a site can be checked before the body
//! it pins has stated anything. Sites are therefore only recorded during the
//! body pass. After it, requirements are settled — a body that pins another
//! body's required parameter to one of its own parameters carries that
//! requirement too — and every site is answered against the settled set.
//!
//! Two requirements are carried here. A body that compares values of a
//! parameter states that it orders them, and a site pinning that parameter to a
//! type without `compare` is refused. A body that applies an arithmetic
//! operator states the operator with both operand types as it wrote them, and a
//! site answers by substituting what it pins into those operands and replaying
//! the operator check against the result — so an instantiation is refused
//! exactly where the written operation is, through every way arithmetic is
//! admitted rather than through a second opinion about which types have it.

use super::context::{Context, GenericDefinition, TypeDefinition};
use super::function_analysis::ModuleId;
use super::operators::{is_ordering_op, missing_ordering_at_instantiation_message};
use super::TypeChecker;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeDeclarationKind, TypeKind};
use crate::ast::{BinaryOp, UnaryOp};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use std::collections::HashMap;

/// The declaration a requirement was recorded against: the type that declares
/// the method, or the [`free_function_owner`] of the module declaring a plain
/// function, paired with the function's own name — the name it is declared
/// under, which an import alias does not change.
pub(crate) type GenericBodyId = (String, String);

/// The owner half of a [`GenericBodyId`] for a function that no type declares:
/// the module declaring it, so two modules' functions of one name are two
/// bodies. The program's own file is the empty owner, and a loaded module's
/// path is bracketed so that it can never spell a type's name.
pub(crate) fn free_function_owner(module: &ModuleId) -> String {
    match module {
        ModuleId::Program => String::new(),
        ModuleId::Imported(path) => format!("<{}>", path.join(".")),
    }
}

/// Every requirement recorded across the program, keyed by the body that stated
/// it.
///
/// A body's obligations are held in the order they were stated: one of them
/// carries operand types, which have no ordering to key a sorted set by, and a
/// body states a handful at most, so membership is a scan. Stating order is
/// what the pass produces, so the diagnostics answered from it are stable.
pub(crate) type InstantiationRequirements = HashMap<GenericBodyId, Vec<Obligation>>;

/// Add `obligation` to what a body has stated, unless it already states it.
/// Reports whether the set grew.
fn state_obligation(stated: &mut Vec<Obligation>, obligation: Obligation) -> bool {
    if stated.contains(&obligation) {
        return false;
    }
    stated.push(obligation);
    true
}

/// What a body needs of its own generic parameters, stated as the operation
/// the body wrote.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Obligation {
    /// The body compares values of this parameter.
    Ordering { parameter: String },
    /// The body applies an arithmetic or comparison operator to operands at
    /// least one of which spells one of its own parameters.
    Binary(WrittenBinary),
    /// The body applies a unary operator to an operand that spells one of its
    /// own parameters.
    Unary(WrittenUnary),
    /// The body casts a value whose type spells one of its own parameters.
    Cast(WrittenCast),
    /// The body calls `method` on a value of `parameter` — a trait-bounded
    /// type parameter, or [`SELF_PIN`] for `self` in a trait default — which
    /// runs the method of whichever class the parameter is bound to.
    ParameterMethod { parameter: String, method: String },
    /// A default of `trait_name` hands its `self` on as a value — an
    /// argument, an initializer, a returned value — rather than calling a
    /// method on it. The value is then an instance of whichever class the
    /// default is compiled for, stored as the trait, so each method the
    /// program calls through that trait, or one above it, runs that class's
    /// own method, as a conversion of the instance to the trait would.
    SelfConversion { trait_name: String },
}

/// A unary operator a body applied, with its operand as that body wrote it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WrittenUnary {
    op: UnaryOp,
    operand: Type,
}

/// A cast a body wrote, with its source as that body wrote it.
///
/// Only the source is carried. A cast's target is written in the body as a type
/// name, and a target spelling a parameter is refused where it is written
/// rather than deferred — so the target an instantiation sees is the one the
/// body already stated, and the question left for the site is whether the value
/// it supplies is a number.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WrittenCast {
    source: Type,
    target: Type,
}

/// An arithmetic or comparison operator a body applied, with both operands as
/// that body wrote them.
///
/// Both types are carried because the operator's meaning is not a property of
/// one of them: an operand written as a literal of another type, a parameter
/// wrapped in a vector, a parameter compared with a second one, and the two
/// sides of a mixed integer-and-float operation are all invisible to a
/// requirement that named a parameter alone.
///
/// The type the body gave the operation is carried too: the body typed the
/// expression before either operand was known, so a site whose operands give a
/// different result would run code compiled for one type over a value laid out
/// as another.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WrittenBinary {
    left: Type,
    op: BinaryOp,
    right: Type,
    result: Type,
}

impl WrittenBinary {
    /// The same operation restated in the pinning body's own parameter names.
    ///
    /// An operand is restated only when it is a bare parameter the site pins,
    /// or a type that spells no parameter the site pins: each yields a name or
    /// a type the program already writes, so delegation hands on one of
    /// finitely many operations however long the chain. An operand that wraps a
    /// pinned parameter is dropped instead — `List<T>` handed to a body that
    /// pins `T` to `List<U>` would grow on every hop around a delegation cycle,
    /// which is the growth [`settle_requirements`] cannot terminate over.
    ///
    /// An operation whose parameters the site all pins to concrete types is
    /// answered at the site itself, so it too hands nothing on.
    fn delegated_through(&self, pins: &HashMap<String, Pin>) -> Option<WrittenBinary> {
        if !hands_on_a_parameter(&self.left, pins) && !hands_on_a_parameter(&self.right, pins) {
            return None;
        }
        Some(WrittenBinary {
            left: delegated_operand(&self.left, pins)?,
            op: self.op,
            right: delegated_operand(&self.right, pins)?,
            result: delegated_operand(&self.result, pins)?,
        })
    }
}

/// Each parameter the site pins to a type it names directly.
fn concretely_pinned(pins: &HashMap<String, Pin>) -> HashMap<String, Type> {
    pins.iter()
        .filter_map(|(parameter, pin)| match pin {
            Pin::Concrete(pinned) => Some((parameter.clone(), pinned.clone())),
            Pin::CallerParameter(_) => None,
        })
        .collect()
}

/// The pinned parameter the operands spell, which the help names as the one the
/// site chose a type for. Parameters are considered in name order, so a body
/// written against several is always reported against the same one.
fn pinned_parameter_spelled_in<'p>(
    operands: &[&Type],
    pins: &'p HashMap<String, Pin>,
) -> Option<&'p str> {
    let mut parameters: Vec<&str> = pins.keys().map(String::as_str).collect();
    parameters.sort_unstable();
    parameters.into_iter().find(|parameter| {
        let spelled = |kind: &TypeKind| generic_parameter_name(kind) == Some(*parameter);
        operands
            .iter()
            .any(|operand| spells_a_type(&operand.kind, &spelled))
    })
}

/// True when `ty` is a bare parameter the site pins to one of its own.
fn hands_on_a_parameter(ty: &Type, pins: &HashMap<String, Pin>) -> bool {
    generic_parameter_name(&ty.kind)
        .and_then(|name| pins.get(name))
        .is_some_and(|pin| matches!(pin, Pin::CallerParameter(_)))
}

/// The operand as the pinning body would write it, or `None` where restating it
/// would build a type larger than the program writes.
fn delegated_operand(ty: &Type, pins: &HashMap<String, Pin>) -> Option<Type> {
    if let Some(pin) = generic_parameter_name(&ty.kind).and_then(|name| pins.get(name)) {
        return Some(match pin {
            Pin::CallerParameter(own) => Type::new(
                TypeKind::Generic(own.clone(), None, TypeDeclarationKind::None),
                ty.span,
            ),
            Pin::Concrete(pinned) => pinned.clone(),
        });
    }
    let pinned_here =
        |kind: &TypeKind| generic_parameter_name(kind).is_some_and(|name| pins.contains_key(name));
    (!spells_a_type(&ty.kind, &pinned_here)).then(|| ty.clone())
}

impl Obligation {
    /// The same obligation restated in the pinning body's own parameter names,
    /// when every parameter it mentions is pinned to one of those.
    ///
    /// A parameter pinned to a concrete type is answered at the site itself
    /// rather than handed on, so such an obligation delegates nothing.
    fn delegated_through(&self, pins: &HashMap<String, Pin>) -> Option<Obligation> {
        match self {
            Obligation::Ordering { parameter } => match pins.get(parameter) {
                Some(Pin::CallerParameter(own)) => Some(Obligation::Ordering {
                    parameter: own.clone(),
                }),
                Some(Pin::Concrete(_)) | None => None,
            },
            Obligation::Binary(written) => written.delegated_through(pins).map(Obligation::Binary),
            Obligation::Unary(written) => {
                delegated_operand_of(&written.operand, pins).map(|operand| {
                    Obligation::Unary(WrittenUnary {
                        op: written.op,
                        operand,
                    })
                })
            }
            Obligation::Cast(written) => {
                delegated_operand_of(&written.source, pins).map(|source| {
                    Obligation::Cast(WrittenCast {
                        source,
                        target: written.target.clone(),
                    })
                })
            }
            Obligation::ParameterMethod { parameter, method } => match pins.get(parameter) {
                Some(Pin::CallerParameter(own)) => Some(Obligation::ParameterMethod {
                    parameter: own.clone(),
                    method: method.clone(),
                }),
                Some(Pin::Concrete(_)) | None => None,
            },
            // `self` is pinned to the instance at every site that pins a
            // default, so the conversion is answered there, never handed on.
            Obligation::SelfConversion { .. } => None,
        }
    }
}

/// The single operand of a one-operand obligation, restated in the pinning
/// body's own parameter names, or `None` when the site pins it to a concrete
/// type and so answers it itself rather than handing it on.
fn delegated_operand_of(operand: &Type, pins: &HashMap<String, Pin>) -> Option<Type> {
    if !hands_on_a_parameter(operand, pins) {
        return None;
    }
    delegated_operand(operand, pins)
}

/// What a site pins one generic parameter to.
#[derive(Debug, Clone)]
pub(crate) enum Pin {
    /// One of the pinning body's own generic parameters, named as that body
    /// declares it. Such a pin names no type to judge: it hands the requirement
    /// on to the sites that pin the pinning body.
    CallerParameter(String),
    /// A type the site names directly, judged against the requirement.
    Concrete(Type),
}

/// One place the program pins a generic body's parameters: a call to a generic
/// function, a method reached through a receiver whose type arguments are
/// known, or a method the program runs without writing a call to it.
#[derive(Debug, Clone)]
pub(crate) struct PinningSite {
    /// The body the site is written in, or `None` outside a function body.
    pub(super) caller: Option<GenericBodyId>,
    /// The body whose parameters the site pins.
    pub(super) callee: GenericBodyId,
    /// Each pinned parameter of `callee`, by the name `callee` declares it with.
    pub(super) pins: HashMap<String, Pin>,
    pub(super) span: Span,
    /// Why the program reaches `callee` here when no call to it is written at
    /// `span`, said in a sentence a refusal at this site adds to its help.
    pub(super) reached_because: Option<String>,
    /// The generic parameters of `caller` the site's pinned types spell, read
    /// while `caller`'s scope was the one being checked. A site derived from
    /// this one while answering reads a type argument as one of these
    /// parameters, and so hands its requirement on to `caller`'s own sites.
    pub(super) caller_parameters: Vec<String>,
}

impl PinningSite {
    /// The same site pinning `method` of the body's owner instead.
    pub(super) fn for_method(&self, method: &str) -> PinningSite {
        PinningSite {
            callee: (self.callee.0.clone(), method.to_string()),
            ..self.clone()
        }
    }
}

/// The parameter a trait default's site pins to the instance the default runs
/// on. A default's body is compiled once per class with a static `self`, so
/// each method it calls on `self` is that class's own.
pub(crate) const SELF_PIN: &str = crate::ast::types::SELF_TYPE_NAME;

impl Obligation {
    /// The method an instance pinned to `parameter` runs for this obligation,
    /// when the obligation is an operation on that parameter which a class
    /// answers with a method of its own, or a call on a value of it.
    ///
    /// A method call names its receiver's parameter outright. An operator
    /// dispatches on its left operand, so only a parameter written
    /// there — or, for an equality, as the payload of an optional there —
    /// reaches a method; an ordering reaches `compare` from either side, and
    /// [`Obligation::Ordering`] is recorded for each.
    pub(super) fn method_run_on(&self, parameter: &str) -> Option<&str> {
        match self {
            Obligation::Ordering { parameter: ordered } => {
                (ordered == parameter).then_some(crate::ast::implicit_methods::ORDERING_METHOD_NAME)
            }
            Obligation::Binary(written) => {
                let receiver =
                    crate::ast::implicit_methods::operator_receiver(&written.op, &written.left);
                let receiver = generic_parameter_name(&receiver.kind)?;
                (receiver == parameter)
                    .then(|| crate::ast::implicit_methods::operator_method_name(&written.op))
                    .flatten()
            }
            Obligation::ParameterMethod {
                parameter: called,
                method,
            } => (called == parameter).then_some(method.as_str()),
            Obligation::Unary(_) | Obligation::Cast(_) | Obligation::SelfConversion { .. } => None,
        }
    }
}

/// Grow `requirements` until every body that pins another body's required
/// parameter to one of its own parameters carries that obligation too.
///
/// A requirement can only be added, and the obligations that can be added form
/// a finite set. An [`Obligation::Ordering`] names a generic parameter of the
/// body it is recorded against, and the program declares finitely many; an
/// [`Obligation::ParameterMethod`] names one of those and a method the program
/// writes, and an [`Obligation::SelfConversion`] a trait it declares. The
/// three that carry types — [`Obligation::Binary`], [`Obligation::Unary`]
/// and [`Obligation::Cast`] — carry operands that could grow around a
/// delegation cycle; every one of them hands an operand on through
/// [`delegated_operand`], which passes only a bare parameter name or a type
/// written in the program and drops any that would be built larger. A cast's
/// target is never restated at all, because a body writes it as a type name
/// rather than deferring it. Each set being finite, the loop ends, including
/// when bodies delegate to each other in a cycle.
fn settle_requirements(requirements: &mut InstantiationRequirements, sites: &[&PinningSite]) {
    loop {
        let mut inherited: Vec<(GenericBodyId, Obligation)> = Vec::new();
        for site in sites {
            let (Some(caller), Some(required)) = (&site.caller, requirements.get(&site.callee))
            else {
                continue;
            };
            let delegated = required
                .iter()
                .filter_map(|obligation| obligation.delegated_through(&site.pins));
            inherited.extend(delegated.map(|obligation| (caller.clone(), obligation)));
        }
        let mut grew = false;
        for (caller, obligation) in inherited {
            grew |= state_obligation(requirements.entry(caller).or_default(), obligation);
        }
        if !grew {
            return;
        }
    }
}

/// The kind of a container spelled with its own type syntax (`[T]`, `{K: V}`,
/// `{T}`), with its element — a map's key — as written.
fn canonical_container_element(kind: &TypeKind) -> Option<(BuiltinCollectionKind, &Expression)> {
    if let TypeKind::List(element) = kind {
        return Some((BuiltinCollectionKind::List, element));
    }
    if let TypeKind::Array(element, _) = kind {
        return Some((BuiltinCollectionKind::Array, element));
    }
    if let TypeKind::Set(element) = kind {
        return Some((BuiltinCollectionKind::Set, element));
    }
    if let TypeKind::Map(key, _) = kind {
        return Some((BuiltinCollectionKind::Map, key));
    }
    None
}

/// The type a written type argument names, or `None` for a value argument.
pub(super) fn written_type(argument: &Expression) -> Option<Type> {
    let ExpressionKind::Type(ty, _) = &argument.node else {
        return None;
    };
    Some((**ty).clone())
}

/// The generic-parameter name `ty` spells, when that name is in scope as a
/// parameter rather than as a declared type.
fn generic_parameter_in_scope<'t>(ty: &'t Type, context: &Context) -> Option<&'t str> {
    parameter_in_scope(&ty.kind, context)
}

/// [`generic_parameter_in_scope`] against a bare kind, for the walk over a
/// type's components, which holds kinds rather than types.
fn parameter_in_scope<'k>(kind: &'k TypeKind, context: &Context) -> Option<&'k str> {
    let name = generic_parameter_name(kind)?;
    let is_parameter = matches!(
        context.resolve_type_definition(name),
        Some(TypeDefinition::Generic(_))
    );
    is_parameter.then_some(name)
}

/// The in-scope generic parameters `types` spell, each named once, in the
/// order they are first spelled.
fn parameters_spelled<'t>(types: impl Iterator<Item = &'t Type>, context: &Context) -> Vec<String> {
    let spelled = std::cell::RefCell::new(Vec::<String>::new());
    for ty in types {
        spells_a_type(&ty.kind, &|kind| {
            if let Some(name) = parameter_in_scope(kind, context) {
                let mut spelled = spelled.borrow_mut();
                if !spelled.iter().any(|known| known == name) {
                    spelled.push(name.to_string());
                }
            }
            false
        });
    }
    spelled.into_inner()
}

/// The parameter name a bare generic-parameter spelling refers to.
fn generic_parameter_name(kind: &TypeKind) -> Option<&str> {
    super::generics::generic_parameter_name(kind)
}

/// True when `kind`, or any type it spells at any depth, satisfies `applies`.
///
/// A parameter reaches an operator from below the surface as readily as from
/// it — `Vec3<T> * f32` states as much about `T` as `a * b` does — so a
/// requirement is decided over the whole type rather than its head alone.
pub(crate) fn spells_a_type(kind: &TypeKind, applies: &dyn Fn(&TypeKind) -> bool) -> bool {
    if applies(kind) {
        return true;
    }
    let spelled_by = |expr: &Expression| matches!(&expr.node, ExpressionKind::Type(ty, _) if spells_a_type(&ty.kind, applies));
    match kind {
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
            spelled_by(element)
        }
        TypeKind::Array(element, size) => spelled_by(element) || spelled_by(size),
        TypeKind::Map(key, value) | TypeKind::Result(key, value) => {
            spelled_by(key) || spelled_by(value)
        }
        TypeKind::Tuple(elements) => elements.iter().any(spelled_by),
        TypeKind::Custom(_, Some(arguments)) => arguments.iter().any(spelled_by),
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            spells_a_type(&inner.kind, applies)
        }
        TypeKind::Generic(_, Some(bound), _) => spells_a_type(&bound.kind, applies),
        TypeKind::Function(signature) => {
            signature.params.iter().any(|param| spelled_by(&param.typ))
                || signature.return_type.as_deref().is_some_and(spelled_by)
        }
        TypeKind::Custom(_, None)
        | TypeKind::Generic(_, None, _)
        | TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => false,
    }
}

impl TypeChecker {
    /// Record that the body being checked orders values of `ty`, when `ty` is
    /// one of that body's own generic parameters.
    ///
    /// Called for the left operand of every ordering operator, so a body that
    /// never orders a parameter records nothing and every one of its
    /// instantiations stays unconstrained.
    pub(crate) fn record_ordering_requirement(&mut self, ty: &Type, context: &Context) {
        let Some(parameter) = generic_parameter_in_scope(ty, context) else {
            return;
        };
        let Some(body) = self.current_body(context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::Ordering {
                parameter: parameter.to_string(),
            },
        );
    }

    /// Record that the trait default being checked calls `method` through a
    /// receiver of the trait `trait_name` it belongs to, which in a default
    /// is `self`: the default is compiled once per class, and the call runs
    /// that class's own method.
    ///
    /// A call through another value of the same trait counts too — it can pin
    /// a method the default never reaches, never leave one it reaches
    /// unpinned — except one through a parameter the default declares. A
    /// parameter holds what its caller passed: an instance converted to the
    /// trait, whose conversion pins the method, or the caller's own `self`,
    /// which [`Obligation::SelfConversion`] records where the caller writes
    /// it.
    pub(crate) fn record_self_method_requirement(
        &mut self,
        trait_name: &str,
        method: &str,
        context: &Context,
    ) {
        if self.member_receiver_is_parameter {
            return;
        }
        let Some(body) = self.enclosing_default_of(trait_name, context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::ParameterMethod {
                parameter: SELF_PIN.to_string(),
                method: method.to_string(),
            },
        );
    }

    /// Record that the trait default being checked hands on a value of the
    /// trait `trait_name`, which may be its `self` when the default belongs
    /// to that trait or to one below it.
    pub(crate) fn record_self_conversion_of(&mut self, trait_name: &str, context: &Context) {
        let Some(owner) = context.current_class.clone() else {
            return;
        };
        let Some(body) = self.enclosing_default_of(trait_name, context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::SelfConversion { trait_name: owner },
        );
    }

    /// The body being checked, when it is compiled once per concrete class —
    /// a trait default, or a method an abstract class declares — and a value
    /// of `trait_name` may be its `self`: the body belongs to `trait_name` or
    /// to a type below it.
    fn enclosing_default_of(&self, trait_name: &str, context: &Context) -> Option<GenericBodyId> {
        let owner = context.current_class.as_deref()?;
        let is_own_type = owner == trait_name
            || self
                .declaring_types_above(owner, &HashMap::new())
                .iter()
                .any(|(above, _)| above == trait_name);
        if !self.is_copied_per_class(owner) || !is_own_type {
            return None;
        }
        self.current_body(context)
    }

    /// Whether the bodies `type_name` declares are compiled once per concrete
    /// class that runs them, with `self` that class: a trait's defaults, and
    /// an abstract class's methods.
    pub(super) fn is_copied_per_class(&self, type_name: &str) -> bool {
        match self.type_table.global_type_definitions.get(type_name) {
            Some(TypeDefinition::Trait(_)) => true,
            Some(TypeDefinition::Class(class_def)) => class_def.is_abstract,
            Some(
                TypeDefinition::Struct(_)
                | TypeDefinition::Enum(_)
                | TypeDefinition::Generic(_)
                | TypeDefinition::Alias(_),
            )
            | None => false,
        }
    }

    /// Record that the body being checked calls `method` on a value of type
    /// `receiver`, when `receiver` is one of the body's own trait-bounded
    /// parameters: the call runs the method of whichever class a site binds
    /// the parameter to, so each such site answers for that class's method.
    pub(crate) fn record_parameter_method_requirement(
        &mut self,
        receiver: &Type,
        method: &str,
        context: &Context,
    ) {
        if !matches!(receiver.kind, TypeKind::Generic(_, Some(_), _)) {
            return;
        }
        let Some(parameter) = generic_parameter_in_scope(receiver, context) else {
            return;
        };
        let Some(body) = self.current_body(context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::ParameterMethod {
                parameter: parameter.to_string(),
                method: method.to_string(),
            },
        );
    }

    /// Record that the body being checked applies `op` to operands of these
    /// types, when either of them spells one of that body's own generic
    /// parameters.
    ///
    /// Called for an operation the body's own check admitted, which for a
    /// parameter it admits on the grounds that the type is decided elsewhere.
    /// Recording what was admitted is what makes "elsewhere" a place: an
    /// operation the body already refused is not restated at every site.
    pub(crate) fn record_binary_requirement(
        &mut self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        result: &Type,
        context: &Context,
    ) {
        let spells_a_parameter = |ty: &Type| {
            spells_a_type(&ty.kind, &|kind| {
                parameter_in_scope(kind, context).is_some()
            })
        };
        if !spells_a_parameter(left) && !spells_a_parameter(right) {
            return;
        }
        let Some(body) = self.current_body(context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::Binary(WrittenBinary {
                left: left.clone(),
                op: *op,
                right: right.clone(),
                result: result.clone(),
            }),
        );
    }

    /// Record that the body being checked applies `op` to an operand of this
    /// type, when the type spells one of that body's own generic parameters.
    ///
    /// Called for an operator the body's own check admitted, which for a
    /// parameter it admits on the grounds that the type is decided elsewhere.
    pub(crate) fn record_unary_requirement(
        &mut self,
        op: &UnaryOp,
        operand: &Type,
        context: &Context,
    ) {
        let Some(body) = self.body_stating_a_requirement_about(operand, context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::Unary(WrittenUnary {
                op: *op,
                operand: operand.clone(),
            }),
        );
    }

    /// Record that the body being checked casts a value of this type to
    /// `target`, when the source spells one of that body's own generic
    /// parameters.
    pub(crate) fn record_cast_requirement(
        &mut self,
        source: &Type,
        target: &Type,
        context: &Context,
    ) {
        let Some(body) = self.body_stating_a_requirement_about(source, context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::Cast(WrittenCast {
                source: source.clone(),
                target: target.clone(),
            }),
        );
    }

    /// The body a requirement about `ty` would be recorded against, when `ty`
    /// spells one of that body's own generic parameters and there is a body to
    /// record against at all.
    fn body_stating_a_requirement_about(
        &self,
        ty: &Type,
        context: &Context,
    ) -> Option<GenericBodyId> {
        let spells_a_parameter = spells_a_type(&ty.kind, &|kind| {
            parameter_in_scope(kind, context).is_some()
        });
        spells_a_parameter
            .then(|| self.current_body(context))
            .flatten()
    }

    /// The body identifier for the declaration currently being checked, or
    /// `None` outside a function body.
    fn current_body(&self, context: &Context) -> Option<GenericBodyId> {
        let function = context.current_function.clone()?;
        let owner = context.current_class.clone().unwrap_or_else(|| {
            free_function_owner(&ModuleId::checked_as(&self.modules.current_module))
        });
        Some((owner, function))
    }

    /// Record what a call handed a container asks of that container's
    /// elements, when the element type is one of the body's own generic
    /// parameters: an ordering for a call that sorts them, `equals` for one
    /// that matches a value against them.
    ///
    /// The runtime knows an element only by its size, so a sort it performs has
    /// nothing but the element's bytes to order by — which for a reference is
    /// where the value lives rather than what it is — and it matches a class
    /// element only through the class's own `equals`. A body written against a
    /// parameter states the need here, and the sites that pin the parameter
    /// answer it, the same way an operator written in Miri does.
    pub(crate) fn record_elements_a_call_uses(
        &mut self,
        callee: &str,
        container: &Type,
        context: &Context,
    ) {
        if crate::runtime_fns::orders_its_elements(callee) {
            let sequences = [BuiltinCollectionKind::List, BuiltinCollectionKind::Array];
            if let Some(element) = self.container_element_type(container, &sequences) {
                self.record_ordering_requirement(&element, context);
            }
        }
        if crate::runtime_fns::matches_its_elements(callee) {
            let keyed = [BuiltinCollectionKind::Set, BuiltinCollectionKind::Map];
            if let Some(element) = self.container_element_type(container, &keyed) {
                let equals = crate::ast::implicit_methods::EQUALS_METHOD_NAME;
                self.record_element_method_requirement(&element, equals, context);
            }
        }
    }

    /// Record that the body being checked runs `method` on values of `ty`,
    /// when `ty` is one of that body's own generic parameters.
    pub(super) fn record_element_method_requirement(
        &mut self,
        ty: &Type,
        method: &str,
        context: &Context,
    ) {
        let Some(parameter) = generic_parameter_in_scope(ty, context) else {
            return;
        };
        let Some(body) = self.current_body(context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::ParameterMethod {
                parameter: parameter.to_string(),
                method: method.to_string(),
            },
        );
    }

    /// The element type — a map's key type — of a container of one of
    /// `kinds` handed to such a call.
    ///
    /// A call written inside the container's own class names the receiver
    /// without type arguments — `List`, not `List<T>` — so the element is read
    /// from the class's own parameter list, in the position a written argument
    /// would occupy.
    pub(super) fn container_element_type(
        &self,
        container: &Type,
        kinds: &[BuiltinCollectionKind],
    ) -> Option<Type> {
        if let TypeKind::Custom(name, arguments) = &container.kind {
            let kind = BuiltinCollectionKind::from_name(name)?;
            if !kinds.contains(&kind) {
                return None;
            }
            return match arguments {
                Some(arguments) => written_type(arguments.first()?),
                None => self.class_first_parameter(name, container.span),
            };
        }
        let (kind, element) = canonical_container_element(&container.kind)?;
        kinds
            .contains(&kind)
            .then(|| written_type(element))
            .flatten()
    }

    /// The first generic parameter the class `name` declares, as a type.
    fn class_first_parameter(&self, name: &str, span: Span) -> Option<Type> {
        let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(name)
        else {
            return None;
        };
        let parameter = class_def.generics.as_ref()?.first()?;
        Some(Type::new(
            TypeKind::Generic(parameter.name.clone(), None, TypeDeclarationKind::None),
            span,
        ))
    }

    /// Record that the body being checked pins `body`'s generic parameters as
    /// `substitution` spells them, to be answered by
    /// [`answer_pinning_sites`](Self::answer_pinning_sites).
    ///
    /// Nothing is judged here, because the body being pinned may be declared
    /// further down the source and not yet have stated what it requires. Whether
    /// each pin names the checking body's own parameter is decided now, while
    /// that body's scope is the one in `context`.
    pub(crate) fn record_pinning_site(
        &mut self,
        body: GenericBodyId,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) {
        if self.suppress_diagnostics {
            return;
        }
        let site = self.pinning_site(body, substitution, span, context);
        self.pinning_sites.extend(site);
    }

    /// The site pinning `body`'s generic parameters as `substitution` spells
    /// them, or `None` when it pins nothing.
    ///
    /// Whether each pin names the checking body's own parameter is decided
    /// now, while that body's scope is the one in `context`; so is which of
    /// that body's parameters the pinned types spell, which a site derived
    /// from this one reads.
    pub(super) fn pinning_site(
        &self,
        body: GenericBodyId,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) -> Option<PinningSite> {
        if substitution.is_empty() {
            return None;
        }
        let pins: HashMap<String, Pin> = substitution
            .iter()
            .map(|(parameter, pinned)| {
                let pin = match generic_parameter_in_scope(pinned, context) {
                    Some(name) => Pin::CallerParameter(name.to_string()),
                    None => Pin::Concrete(pinned.clone()),
                };
                (parameter.clone(), pin)
            })
            .collect();
        let caller_parameters = parameters_spelled(substitution.values(), context);
        Some(PinningSite {
            caller: self.current_body(context),
            callee: body,
            pins,
            span,
            caller_parameters,
            reached_because: None,
        })
    }

    /// Record the sites a call to `method`, reached through a receiver of type
    /// `class_name` whose generic parameters `substitution` pins, stands for.
    ///
    /// A method's requirement is recorded against whichever type declares it:
    /// the receiver's own class, a base class it `extends`, or a trait whose
    /// default it inherits. The receiver's substitution is therefore carried
    /// up every edge into each type's own parameter names, one site per type.
    /// A receiver with no parameters of its own still pins what it inherits —
    /// `class Sub extends Base<int>` pins `Base`'s `T` to `int`.
    pub(crate) fn record_method_pinning_sites(
        &mut self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) {
        if self.suppress_diagnostics {
            return;
        }
        let sites = self.method_pinning_sites(class_name, method, substitution, span, context);
        self.pinning_sites.extend(sites);
    }

    /// The sites [`record_method_pinning_sites`](Self::record_method_pinning_sites)
    /// records.
    pub(super) fn method_pinning_sites(
        &self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) -> Vec<PinningSite> {
        let own = (class_name.to_string(), method.to_string());
        let above = self.declaring_types_above(class_name, substitution);
        std::iter::once((own, substitution.clone()))
            .chain(
                above
                    .into_iter()
                    .map(|(declaring, rekeyed)| ((declaring, method.to_string()), rekeyed)),
            )
            .filter_map(|(body, pins)| self.pinning_site(body, &pins, span, context))
            .collect()
    }

    /// Settle every requirement, then report each site that pins a required
    /// parameter to a type that cannot meet the requirement.
    ///
    /// Runs once the body pass has recorded every requirement and every site,
    /// so a site is answered the same wherever it is written relative to the
    /// body it pins. The sites a trait slot reaches join here, once every
    /// conversion and every call through a trait is known.
    /// `context` carries the program's global scope. An obligation is answered
    /// only once every type in it is concrete — a parameter pinned to another
    /// parameter is handed on instead — so nothing named there is body-local,
    /// and the global scope is the whole scope the answer needs.
    pub(crate) fn answer_pinning_sites(&mut self, context: &Context) {
        let mut sites = std::mem::take(&mut self.pinning_sites);
        sites.extend(self.sites_reached_through_trait_slots());
        let mut requirements = std::mem::take(&mut self.instantiation_requirements);
        let mut derivation = super::used_methods::SiteDerivation::rooted_at(sites.len());
        loop {
            settle_requirements(&mut requirements, &sites.iter().collect::<Vec<_>>());
            if !self.derive_used_method_sites(&requirements, &mut sites, &mut derivation) {
                break;
            }
        }
        for site in &sites {
            let stated = requirements
                .get(&site.callee)
                .map_or(&[][..], Vec::as_slice);
            self.answer_pinning_site(stated, site, context);
        }
        self.instantiation_requirements = requirements;
    }

    /// Answer every obligation the body `site` pins has stated.
    ///
    /// The obligations are held outside the checker for the length of the
    /// answering pass, so a site reads what its body stated rather than copying
    /// it — the operands an arithmetic obligation carries make that copy a
    /// deep one.
    pub(super) fn answer_pinning_site(
        &mut self,
        stated: &[Obligation],
        site: &PinningSite,
        context: &Context,
    ) {
        for obligation in stated {
            match obligation {
                Obligation::Ordering { parameter } => self.answer_ordering(parameter, site),
                Obligation::Binary(written) => self.answer_binary(written, site, context),
                Obligation::Unary(written) => self.answer_unary(written, site),
                Obligation::Cast(written) => self.answer_cast(written, site),
                Obligation::ParameterMethod { .. } | Obligation::SelfConversion { .. } => {}
            }
        }
    }

    /// Report `site` when the unary operator the body wrote has no meaning at
    /// the type the site pins its operand to.
    ///
    /// The judgment is the body's own unary check, replayed against the pinned
    /// operand, for the reason [`answer_binary`](Self::answer_binary)
    /// replays the arithmetic one: a second opinion about which types a unary
    /// operator applies to would have to reproduce every ground on which it is
    /// admitted, and would refuse working programs on the ones it missed.
    fn answer_unary(&mut self, written: &WrittenUnary, site: &PinningSite) {
        let pinned = concretely_pinned(&site.pins);
        let operand = self.substitute_type(&written.operand, &pinned);
        if self.is_unsettled(&operand) {
            return;
        }
        let Err(message) = self.check_unary_op_types(&written.op, &operand) else {
            return;
        };
        let Some(parameter) = pinned_parameter_spelled_in(&[&written.operand], &site.pins) else {
            return;
        };
        let help = format!(
            "'{}' applies '{}' to its '{}' parameter, so the type it is instantiated with has to \
             support it",
            site.callee.1,
            crate::ast::formatter::helpers::unary_operator(written.op),
            parameter
        );
        self.report_at_site(DiagnosticCode::TypTypeMismatch, message, site, help);
    }

    /// Report `site` when it pins the parameter a cast reads to a type that is
    /// not a number, which is the whole of what a numeric cast asks of its
    /// source.
    fn answer_cast(&mut self, written: &WrittenCast, site: &PinningSite) {
        let pinned = concretely_pinned(&site.pins);
        let source = self.substitute_type(&written.source, &pinned);
        if self.is_unsettled(&source) || self.casts_from(&source) {
            return;
        }
        let Some(parameter) = pinned_parameter_spelled_in(&[&written.source], &site.pins) else {
            return;
        };
        let help = format!(
            "'{}' casts its '{}' parameter to '{}', so the type it is instantiated with has to be \
             a number",
            site.callee.1, parameter, written.target
        );
        self.report_at_site(
            DiagnosticCode::TypInvalidCast,
            format!(
                "cannot cast from non-numeric type '{}' to '{}'",
                source, written.target
            ),
            site,
            help,
        );
    }

    /// Report `site` when the operator the body wrote has no meaning at the
    /// types the site pins its operands to.
    ///
    /// The judgment is the body's own operator check, replayed against the
    /// pinned operands. Asking the same question a second way would have to
    /// reproduce every ground on which an operator is admitted — a trait the
    /// operand's class implements, a vector broadcast over a scalar — and
    /// would refuse working programs on the ones it missed. An ordering the
    /// pinned type lacks is left to [`Obligation::Ordering`], which names
    /// that capability.
    fn answer_binary(&mut self, written: &WrittenBinary, site: &PinningSite, context: &Context) {
        let pinned = concretely_pinned(&site.pins);
        let left = self.substitute_type(&written.left, &pinned);
        let right = self.substitute_type(&written.right, &pinned);
        if self.is_unsettled(&left) || self.is_unsettled(&right) {
            return;
        }
        if self.ordering_answered_elsewhere(written, &left, &right) {
            return;
        }
        let Some(message) = self.binary_refusal(written, &left, &right, &pinned, context) else {
            return;
        };
        let Some(parameter) =
            pinned_parameter_spelled_in(&[&written.left, &written.right], &site.pins)
        else {
            return;
        };
        let help = format!(
            "'{}' applies '{}' to its '{}' parameter, so the type it is instantiated with has to \
             support it",
            site.callee.1,
            crate::ast::formatter::helpers::binary_operator(written.op),
            parameter
        );
        self.report_at_site(DiagnosticCode::TypTypeMismatch, message, site, help);
    }

    /// Whether an ordering operator is refused at these pinned operands because
    /// an operand the body wrote as a bare parameter is bound to a type with
    /// no ordering. The body recorded an [`Obligation::Ordering`] for every
    /// such operand, and that obligation names the missing capability, so the
    /// operator is not reported a second time.
    fn ordering_answered_elsewhere(
        &self,
        written: &WrittenBinary,
        left: &Type,
        right: &Type,
    ) -> bool {
        is_ordering_op(&written.op)
            && [(&written.left, left), (&written.right, right)]
                .into_iter()
                .any(|(as_written, pinned)| {
                    matches!(as_written.kind, TypeKind::Generic(..))
                        && !self.orders_its_values(pinned)
                })
    }

    /// Why the operator the body wrote is refused at these pinned operands:
    /// it has no meaning at them, or it gives a type other than the one the
    /// body read the expression at. `None` when the site answers it.
    fn binary_refusal(
        &mut self,
        written: &WrittenBinary,
        left: &Type,
        right: &Type,
        pinned: &HashMap<String, Type>,
        context: &Context,
    ) -> Option<String> {
        let given = match self.check_binary_op_types(left, &written.op, right, context) {
            Ok(given) => given,
            Err(message) => return Some(message),
        };
        let read = self.substitute_type(&written.result, pinned);
        if self.is_unsettled(&read) || self.names_the_same_type(&given, &read, context) {
            return None;
        }
        Some(format!(
            "'{}' applied to {} and {} gives {}, but the generic body reads the result as {}",
            crate::ast::formatter::helpers::binary_operator(written.op),
            left,
            right,
            given,
            read
        ))
    }

    /// Whether two settled types are one type. Two numeric types are one only
    /// when they are the same width and kind; compatibility between numbers
    /// is a conversion, not an identity.
    fn names_the_same_type(&self, first: &Type, second: &Type, context: &Context) -> bool {
        if first.kind == second.kind {
            return true;
        }
        if self.is_numeric(first) || self.is_numeric(second) {
            return false;
        }
        self.type_arguments_agree(first, second, context)
    }

    /// True when `ty` still spells something this site did not settle: a
    /// parameter it does not pin, or a type an earlier error stands in for.
    ///
    /// Such a type is nobody's answer to give — the sites that pin the pinning
    /// body answer it, or the error already reported does — so the operator is
    /// left unjudged rather than refused against a name.
    fn is_unsettled(&self, ty: &Type) -> bool {
        let unsettled = |kind: &TypeKind| {
            matches!(kind, TypeKind::Generic(..) | TypeKind::Error)
                || matches!(kind, TypeKind::Custom(name, None)
                    if !self.type_table.global_type_definitions.contains_key(name.as_str()))
        };
        spells_a_type(&ty.kind, &unsettled)
    }

    /// Report a refusal at `site`, adding to `help` why the program reaches
    /// the site's body when no call to it is written there.
    fn report_at_site(
        &mut self,
        code: DiagnosticCode,
        message: String,
        site: &PinningSite,
        help: String,
    ) {
        let help = match &site.reached_because {
            Some(reason) => format!("{help}; {reason}"),
            None => help,
        };
        self.report_error_with_help(code, message, site.span, help);
    }

    /// Report `site` when it pins `parameter` to a type carrying no ordering.
    fn answer_ordering(&mut self, parameter: &str, site: &PinningSite) {
        let Some(Pin::Concrete(pinned)) = site.pins.get(parameter) else {
            return;
        };
        if self.orders_its_values(pinned) {
            return;
        }
        self.report_at_site(
            DiagnosticCode::TypOrderingNotSupported,
            missing_ordering_at_instantiation_message(pinned),
            site,
            format!(
                "'{}' orders its '{}' parameter, so the type it is instantiated with has to \
                 define 'compare'",
                site.callee.1, parameter
            ),
        );
    }

    /// Re-key a substitution into `declaring`'s own parameter names.
    ///
    /// `written` holds the arguments an `extends`, `implements` or parent-trait
    /// clause passes `declaring`, spelled in the terms of the type that wrote
    /// the clause; resolving each through that type's `substitution` yields
    /// what `declaring`'s parameter is pinned to. A parameter the clause passes
    /// nothing keeps its own name, which is how a trait whose parameter the
    /// class names identically still resolves. A parameter whose argument names
    /// a parameter the substitution does not pin is left out: it is pinned by
    /// nothing here, and naming it would read as the caller's own parameter.
    fn rekeyed_into(
        &self,
        declaring: &str,
        written: &[Type],
        writer_parameters: &[GenericDefinition],
        substitution: &HashMap<String, Type>,
    ) -> HashMap<String, Type> {
        let names_an_unpinned_parameter = |kind: &TypeKind| {
            generic_parameter_name(kind).is_some_and(|name| {
                !substitution.contains_key(name)
                    && (matches!(kind, TypeKind::Generic(..))
                        || writer_parameters.iter().any(|p| p.name == name))
            })
        };
        let mut rekeyed = HashMap::new();
        for (position, generic) in self.generics_of(declaring).iter().enumerate() {
            let argument = written.get(position).cloned().unwrap_or_else(|| {
                Type::new(
                    TypeKind::Generic(generic.name.clone(), None, TypeDeclarationKind::None),
                    Span::new(0, 0),
                )
            });
            if !spells_a_type(&argument.kind, &names_an_unpinned_parameter) {
                let resolved = self.substitute_type(&argument, substitution);
                rekeyed.insert(generic.name.clone(), resolved);
            }
        }
        rekeyed
    }

    /// The generic parameters a class or trait declares, in order.
    pub(crate) fn generics_of(&self, type_name: &str) -> &[GenericDefinition] {
        let generics = match self.type_table.global_type_definitions.get(type_name) {
            Some(TypeDefinition::Class(def)) => def.generics.as_deref(),
            Some(TypeDefinition::Trait(def)) => def.generics.as_deref(),
            Some(
                TypeDefinition::Struct(_)
                | TypeDefinition::Enum(_)
                | TypeDefinition::Generic(_)
                | TypeDefinition::Alias(_),
            )
            | None => None,
        };
        generics.unwrap_or_default()
    }

    /// Every type a receiver of type `type_name` inherits methods from — base
    /// classes, the traits each implements and their parent traits — each
    /// named once, with `substitution` carried up into its own parameters.
    ///
    /// Each entry's substitution is what the clauses on the path from
    /// `type_name` to it bind that type's own parameters to, read through
    /// `substitution` and re-keyed link by link as [`Self::rekeyed_into`]
    /// states. The pinning sites recorded here, every copy of an inherited
    /// trait default the pipeline lowers ([`Self::trait_default_substitution`]),
    /// the return type static dispatch gives a call to one, and the signature
    /// member access types an inherited trait method at all read the clauses
    /// through it.
    pub(crate) fn declaring_types_above(
        &self,
        type_name: &str,
        substitution: &HashMap<String, Type>,
    ) -> Vec<(String, HashMap<String, Type>)> {
        let mut pending = self.direct_supertypes(type_name, substitution);
        let mut seen: Vec<(String, HashMap<String, Type>)> = Vec::new();
        while let Some((name, rekeyed)) = pending.pop() {
            if seen.iter().any(|(known, _)| *known == name) {
                continue;
            }
            pending.extend(self.direct_supertypes(&name, &rekeyed));
            seen.push((name, rekeyed));
        }
        seen
    }

    /// The substitution a copy of `trait_name`'s default compiled under
    /// `class_name` reads its body through: `class_substitution` — the class's
    /// own parameters at one instantiation, or nothing for the class's bare
    /// copy — with the trait's parameters laid over it at what the clauses
    /// from `class_name` up to the trait pin them to.
    ///
    /// The trait's pins win where a trait parameter shares a class
    /// parameter's name: the default's body is written in the trait's.
    pub(crate) fn trait_default_substitution(
        &self,
        class_name: &str,
        trait_name: &str,
        class_substitution: &HashMap<String, Type>,
    ) -> HashMap<String, Type> {
        let supertypes = self.declaring_types_above(class_name, class_substitution);
        let mut substitution = class_substitution.clone();
        if let Some(pins) = pins_of(&supertypes, trait_name) {
            substitution.extend(pins.iter().map(|(param, ty)| (param.clone(), ty.clone())));
        }
        substitution
    }

    /// The arguments `sup` is reached at from `sub` written with the type
    /// arguments `sub_arguments`, in `sup`'s declaration order, read through
    /// the `extends`, `implements` and parent-trait clauses on the way up.
    ///
    /// `None` when `sup` is not above `sub`. An entry is `None` where the
    /// clauses leave that parameter to one `sub_arguments` does not bind.
    pub(crate) fn supertype_arguments(
        &self,
        sub: &str,
        sub_arguments: Option<&[Expression]>,
        sup: &str,
    ) -> Option<Vec<Option<Type>>> {
        let substitution: HashMap<String, Type> = self
            .generics_of(sub)
            .iter()
            .zip(sub_arguments.unwrap_or_default())
            .filter_map(|(generic, argument)| {
                let ty = self.extract_type_from_expression(argument).ok()?;
                Some((generic.name.clone(), ty))
            })
            .collect();
        let supertypes = self.declaring_types_above(sub, &substitution);
        let pins = pins_of(&supertypes, sup)?;
        Some(
            self.generics_of(sup)
                .iter()
                .map(|generic| pins.get(&generic.name).cloned())
                .collect(),
        )
    }

    /// The types `type_name` names in its own `extends`, `implements` or
    /// parent-trait clauses, each with `substitution` re-keyed into it.
    fn direct_supertypes(
        &self,
        type_name: &str,
        substitution: &HashMap<String, Type>,
    ) -> Vec<(String, HashMap<String, Type>)> {
        let edges: Vec<(&String, &[Type])> =
            match self.type_table.global_type_definitions.get(type_name) {
                Some(TypeDefinition::Class(def)) => def
                    .base_class
                    .iter()
                    .map(|base| (base, def.base_class_args.as_deref().unwrap_or_default()))
                    .chain(def.traits.iter().map(|name| {
                        let written = def.trait_args.get(name).map_or(&[][..], Vec::as_slice);
                        (name, written)
                    }))
                    .collect(),
                Some(TypeDefinition::Trait(def)) => def
                    .parent_traits
                    .iter()
                    .map(|name| {
                        let written = def
                            .parent_trait_args
                            .get(name)
                            .map_or(&[][..], Vec::as_slice);
                        (name, written)
                    })
                    .collect(),
                Some(
                    TypeDefinition::Struct(_)
                    | TypeDefinition::Enum(_)
                    | TypeDefinition::Generic(_)
                    | TypeDefinition::Alias(_),
                )
                | None => Vec::new(),
            };
        let writer_parameters = self.generics_of(type_name);
        edges
            .into_iter()
            .map(|(name, written)| {
                let rekeyed = self.rekeyed_into(name, written, writer_parameters, substitution);
                (name.clone(), rekeyed)
            })
            .collect()
    }
}

/// What `supertypes` — as [`TypeChecker::declaring_types_above`] lists them —
/// pins `type_name`'s own parameters to, or `None` when `type_name` is not
/// among them.
pub(crate) fn pins_of<'s>(
    supertypes: &'s [(String, HashMap<String, Type>)],
    type_name: &str,
) -> Option<&'s HashMap<String, Type>> {
    supertypes
        .iter()
        .find_map(|(declaring, pins)| (declaring == type_name).then_some(pins))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn body(owner: &str, function: &str) -> GenericBodyId {
        (owner.to_string(), function.to_string())
    }

    fn named(name: &str) -> Type {
        Type::new(TypeKind::Custom(name.to_string(), None), Span::new(0, 0))
    }

    fn ordering(parameter: &str) -> Obligation {
        Obligation::Ordering {
            parameter: parameter.to_string(),
        }
    }

    fn site(
        caller: Option<GenericBodyId>,
        callee: GenericBodyId,
        pins: Vec<(&str, Pin)>,
    ) -> PinningSite {
        PinningSite {
            caller,
            callee,
            pins: pins
                .into_iter()
                .map(|(parameter, pin)| (parameter.to_string(), pin))
                .collect(),
            span: Span::new(0, 0),
            caller_parameters: Vec::new(),
            reached_because: None,
        }
    }

    /// A checker that has checked `source`, so its type table holds every
    /// declaration the source makes.
    fn checked(source: &str) -> TypeChecker {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let program = parser.parse().expect("the test source parses");
        let mut checker = TypeChecker::new();
        checker
            .check(&program)
            .expect("the test source type-checks");
        checker
    }

    #[test]
    fn a_body_pinning_a_required_parameter_to_its_own_inherits_the_requirement() {
        let mut requirements = InstantiationRequirements::new();
        requirements.insert(body("", "inner"), vec![ordering("T")]);
        let sites = [site(
            Some(body("", "outer")),
            body("", "inner"),
            vec![("T", Pin::CallerParameter("U".into()))],
        )];
        settle_requirements(&mut requirements, &sites.iter().collect::<Vec<_>>());
        assert_eq!(
            requirements.get(&body("", "outer")),
            Some(&vec![ordering("U")])
        );
    }

    #[test]
    fn a_body_pinning_a_required_parameter_to_a_concrete_type_inherits_nothing() {
        let mut requirements = InstantiationRequirements::new();
        requirements.insert(body("", "inner"), vec![ordering("T")]);
        let sites = [site(
            Some(body("", "outer")),
            body("", "inner"),
            vec![("T", Pin::Concrete(named("Pt")))],
        )];
        settle_requirements(&mut requirements, &sites.iter().collect::<Vec<_>>());
        assert_eq!(requirements.get(&body("", "outer")), None);
    }

    #[test]
    fn requirements_settle_across_a_delegation_cycle() {
        let mut requirements = InstantiationRequirements::new();
        requirements.insert(body("", "a"), vec![ordering("T")]);
        let sites = [
            site(
                Some(body("", "b")),
                body("", "a"),
                vec![("T", Pin::CallerParameter("U".into()))],
            ),
            site(
                Some(body("", "a")),
                body("", "b"),
                vec![("U", Pin::CallerParameter("T".into()))],
            ),
        ];
        settle_requirements(&mut requirements, &sites.iter().collect::<Vec<_>>());
        assert_eq!(requirements.get(&body("", "a")), Some(&vec![ordering("T")]));
        assert_eq!(requirements.get(&body("", "b")), Some(&vec![ordering("U")]));
    }

    fn calls(parameter: &str, method: &str) -> Obligation {
        Obligation::ParameterMethod {
            parameter: parameter.to_string(),
            method: method.to_string(),
        }
    }

    #[test]
    fn a_method_called_on_a_bounded_parameter_is_handed_on_to_the_caller() {
        let mut requirements = InstantiationRequirements::new();
        requirements.insert(body("", "go"), vec![calls("X", "lt")]);
        let sites = [site(
            Some(body("", "outer")),
            body("", "go"),
            vec![("X", Pin::CallerParameter("Y".into()))],
        )];
        settle_requirements(&mut requirements, &sites.iter().collect::<Vec<_>>());
        assert_eq!(
            requirements.get(&body("", "outer")),
            Some(&vec![calls("Y", "lt")])
        );
    }

    #[test]
    fn a_method_called_on_a_parameter_is_run_on_that_parameter_only() {
        let obligation = calls("X", "lt");
        assert_eq!(obligation.method_run_on("X"), Some("lt"));
        assert_eq!(obligation.method_run_on("Y"), None);
    }

    #[test]
    fn a_written_clause_argument_is_resolved_through_the_writers_substitution() {
        let checker = checked("class Base<T>\n    fn get(a T) T\n        return a\n");
        let substitution = HashMap::from([("U".to_string(), named("Pt"))]);
        let written = [named("U")];
        let writer = [GenericDefinition {
            name: "U".into(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }];
        let rekeyed = checker.rekeyed_into("Base", &written, &writer, &substitution);
        assert_eq!(
            rekeyed.get("T").map(|t| t.kind.clone()),
            Some(named("Pt").kind)
        );
    }

    #[test]
    fn a_clause_argument_naming_an_unpinned_parameter_pins_nothing() {
        let checker = checked("class Base<T>\n    fn get(a T) T\n        return a\n");
        let written = [named("U")];
        let writer = [GenericDefinition {
            name: "U".into(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }];
        let rekeyed = checker.rekeyed_into("Base", &written, &writer, &HashMap::new());
        assert!(rekeyed.is_empty(), "{rekeyed:?}");
    }

    #[test]
    fn the_types_above_a_class_include_its_base_classes_and_their_traits() {
        let checker = checked(
            "trait Op<V>\n    fn run(a V) V\n        return a\n\n\
             class Base<T> implements Op<T>\n\n\
             class Mid<U> extends Base<U>\n\n\
             class Sub extends Mid<int>\n",
        );
        let mut above: Vec<(String, Option<TypeKind>)> = checker
            .declaring_types_above("Sub", &HashMap::new())
            .into_iter()
            .map(|(name, rekeyed)| {
                let pinned = rekeyed.into_values().next().map(|ty| ty.kind);
                (name, pinned)
            })
            .collect();
        above.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            above,
            vec![
                ("Base".to_string(), Some(TypeKind::Int)),
                ("Mid".to_string(), Some(TypeKind::Int)),
                ("Op".to_string(), Some(TypeKind::Int)),
            ]
        );
    }

    fn prim(kind: TypeKind) -> Type {
        Type::new(kind, Span::new(0, 0))
    }

    fn kind_of(substitution: &HashMap<String, Type>, param: &str) -> Option<TypeKind> {
        substitution.get(param).map(|ty| ty.kind.clone())
    }

    #[test]
    fn a_default_a_base_trait_supplies_reads_its_parameter_at_the_instantiation() {
        let checker = checked(
            "trait Op<T>\n    fn keep(a T) T\n        return a\n\n\
             abstract class Base<U> implements Op<U>\n\n\
             class Box<V> extends Base<V>\n",
        );
        let class = HashMap::from([("V".to_string(), prim(TypeKind::Float))]);
        let substitution = checker.trait_default_substitution("Box", "Op", &class);
        assert_eq!(kind_of(&substitution, "T"), Some(TypeKind::Float));
        assert_eq!(kind_of(&substitution, "V"), Some(TypeKind::Float));
    }

    #[test]
    fn a_trait_parameter_sharing_a_class_parameter_name_reads_the_trait_pin() {
        let checker = checked(
            "trait Op<T>\n    fn keep(a T) T\n        return a\n\n\
             class Box<T> implements Op<int>\n    v T\n",
        );
        let class = HashMap::from([("T".to_string(), prim(TypeKind::Float))]);
        let substitution = checker.trait_default_substitution("Box", "Op", &class);
        assert_eq!(kind_of(&substitution, "T"), Some(TypeKind::Int));
    }

    #[test]
    fn a_trait_outside_the_chain_leaves_the_class_substitution_alone() {
        let checker =
            checked("trait Op<T>\n    fn keep(a T) T\n        return a\n\nclass Box<V>\n    v V\n");
        let class = HashMap::from([("V".to_string(), prim(TypeKind::Float))]);
        let substitution = checker.trait_default_substitution("Box", "Op", &class);
        assert_eq!(substitution.len(), 1);
        assert_eq!(kind_of(&substitution, "V"), Some(TypeKind::Float));
    }
}
