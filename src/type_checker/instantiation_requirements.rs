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

use super::context::{Context, TypeDefinition};
use super::operators::missing_ordering_at_instantiation_message;
use super::TypeChecker;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeDeclarationKind, TypeKind};
use crate::ast::BinaryOp;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use std::collections::HashMap;

/// The declaration a requirement was recorded against: the type that declares
/// the method, or [`FREE_FUNCTION_OWNER`] for a plain function, paired with the
/// function's own name.
pub(crate) type GenericBodyId = (String, String);

/// The owner half of a [`GenericBodyId`] for a function that no type declares.
pub(crate) const FREE_FUNCTION_OWNER: &str = "";

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
    /// The body applies an arithmetic operator to operands at least one of
    /// which spells one of its own parameters.
    Arithmetic(WrittenArithmetic),
}

/// An arithmetic operator a body applied, with both operands as that body
/// wrote them.
///
/// Both types are carried because the operator's meaning is not a property of
/// one of them: an operand written as a literal of another type, a parameter
/// wrapped in a vector, and the two sides of a mixed integer-and-float
/// operation are all invisible to a requirement that named a parameter alone.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WrittenArithmetic {
    left: Type,
    op: BinaryOp,
    right: Type,
}

impl WrittenArithmetic {
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
    fn delegated_through(&self, pins: &HashMap<String, Pin>) -> Option<WrittenArithmetic> {
        if !hands_on_a_parameter(&self.left, pins) && !hands_on_a_parameter(&self.right, pins) {
            return None;
        }
        Some(WrittenArithmetic {
            left: delegated_operand(&self.left, pins)?,
            op: self.op,
            right: delegated_operand(&self.right, pins)?,
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
    written: &WrittenArithmetic,
    pins: &'p HashMap<String, Pin>,
) -> Option<&'p str> {
    let mut parameters: Vec<&str> = pins.keys().map(String::as_str).collect();
    parameters.sort_unstable();
    parameters.into_iter().find(|parameter| {
        let spelled = |kind: &TypeKind| generic_parameter_name(kind) == Some(*parameter);
        spells_a_type(&written.left.kind, &spelled) || spells_a_type(&written.right.kind, &spelled)
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
            Obligation::Arithmetic(written) => {
                written.delegated_through(pins).map(Obligation::Arithmetic)
            }
        }
    }
}

/// What a site pins one generic parameter to.
#[derive(Debug)]
pub(crate) enum Pin {
    /// One of the pinning body's own generic parameters, named as that body
    /// declares it. Such a pin names no type to judge: it hands the requirement
    /// on to the sites that pin the pinning body.
    CallerParameter(String),
    /// A type the site names directly, judged against the requirement.
    Concrete(Type),
}

/// One place the program pins a generic body's parameters: a call to a generic
/// function, or a method reached through a receiver whose type arguments are
/// known.
#[derive(Debug)]
pub(crate) struct PinningSite {
    /// The body the site is written in, or `None` outside a function body.
    caller: Option<GenericBodyId>,
    /// The body whose parameters the site pins.
    callee: GenericBodyId,
    /// Each pinned parameter of `callee`, by the name `callee` declares it with.
    pins: HashMap<String, Pin>,
    span: Span,
}

/// Grow `requirements` until every body that pins another body's required
/// parameter to one of its own parameters carries that obligation too.
///
/// A requirement can only be added, and the obligations that can be added form
/// a finite set. An [`Obligation::Ordering`] names a generic parameter of the
/// body it is recorded against, and the program declares finitely many. An
/// [`Obligation::Arithmetic`] carries operand types, which could grow around a
/// delegation cycle; [`WrittenArithmetic::delegated_through`] answers that by
/// handing on only operands that are a bare parameter name or a type written in
/// the program, and dropping any that would be built larger. Both sets being
/// finite, the loop ends, including when bodies delegate to each other in a
/// cycle.
fn settle_requirements(requirements: &mut InstantiationRequirements, sites: &[PinningSite]) {
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

/// The body identifier for the declaration currently being checked, or `None`
/// outside a function body.
fn current_body(context: &Context) -> Option<GenericBodyId> {
    let function = context.current_function.clone()?;
    let owner = context
        .current_class
        .clone()
        .unwrap_or_else(|| FREE_FUNCTION_OWNER.to_string());
    Some((owner, function))
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

/// The parameter name a bare generic-parameter spelling refers to.
fn generic_parameter_name(kind: &TypeKind) -> Option<&str> {
    super::generics::generic_parameter_name(kind)
}

/// True when `kind`, or any type it spells at any depth, satisfies `applies`.
///
/// A parameter reaches an operator from below the surface as readily as from
/// it — `Vec3<T> * f32` states as much about `T` as `a * b` does — so a
/// requirement is decided over the whole type rather than its head alone.
fn spells_a_type(kind: &TypeKind, applies: &dyn Fn(&TypeKind) -> bool) -> bool {
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
        let Some(body) = current_body(context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::Ordering {
                parameter: parameter.to_string(),
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
    pub(crate) fn record_arithmetic_requirement(
        &mut self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
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
        let Some(body) = current_body(context) else {
            return;
        };
        state_obligation(
            self.instantiation_requirements.entry(body).or_default(),
            Obligation::Arithmetic(WrittenArithmetic {
                left: left.clone(),
                op: *op,
                right: right.clone(),
            }),
        );
    }

    /// Record that the body being checked hands a container to a call that
    /// orders that container's elements, when the element type is one of the
    /// body's own generic parameters.
    ///
    /// The runtime knows an element only by its size, so a sort it performs has
    /// nothing but the element's bytes to order by — which for a reference is
    /// where the value lives rather than what it is. A body written against a
    /// parameter states the need here, and the sites that pin the parameter
    /// answer it, the same way an ordering operator written in Miri does.
    pub(crate) fn record_elements_a_call_orders(
        &mut self,
        callee: &str,
        container: &Type,
        context: &Context,
    ) {
        if !crate::runtime_fns::orders_its_elements(callee) {
            return;
        }
        let Some(element) = self.sorted_element_type(container) else {
            return;
        };
        self.record_ordering_requirement(&element, context);
    }

    /// The element type of a container handed to such a call.
    ///
    /// A call written inside the container's own class names the receiver
    /// without type arguments — `List`, not `List<T>` — so the element is read
    /// from the class's own parameter list, in the position a written argument
    /// would occupy.
    fn sorted_element_type(&self, container: &Type) -> Option<Type> {
        if let Some(element) = container.kind.sequence_element_kind() {
            return Some(Type::new(element.clone(), container.span));
        }
        let TypeKind::Custom(name, None) = &container.kind else {
            return None;
        };
        if !matches!(
            BuiltinCollectionKind::from_name(name),
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
        ) {
            return None;
        }
        let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(name.as_str())
        else {
            return None;
        };
        let parameter = class_def.generics.as_ref()?.first()?;
        Some(Type::new(
            TypeKind::Generic(parameter.name.clone(), None, TypeDeclarationKind::None),
            container.span,
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
        if substitution.is_empty() || self.suppress_diagnostics {
            return;
        }
        let pins = substitution
            .iter()
            .map(|(parameter, pinned)| {
                let pin = match generic_parameter_in_scope(pinned, context) {
                    Some(name) => Pin::CallerParameter(name.to_string()),
                    None => Pin::Concrete(pinned.clone()),
                };
                (parameter.clone(), pin)
            })
            .collect();
        self.pinning_sites.push(PinningSite {
            caller: current_body(context),
            callee: body,
            pins,
            span,
        });
    }

    /// Record the sites a call to `method`, reached through a receiver of type
    /// `class_name` whose generic parameters `substitution` pins, stands for.
    ///
    /// A method's requirement is recorded against whichever type declares it,
    /// which for an inherited default method is a trait rather than the
    /// receiver's own class. The receiver's substitution is therefore re-keyed
    /// into each declaring type's parameter names, one site per declaring type.
    pub(crate) fn record_method_pinning_sites(
        &mut self,
        class_name: &str,
        method: &str,
        substitution: &HashMap<String, Type>,
        span: Span,
        context: &Context,
    ) {
        if substitution.is_empty() {
            return;
        }
        self.record_pinning_site(
            (class_name.to_string(), method.to_string()),
            substitution,
            span,
            context,
        );
        let bindings = self.class_trait_param_bindings(class_name);
        for declaring in self.declaring_types_above(class_name) {
            let rekeyed = self.rekeyed_into(&declaring, substitution, &bindings);
            self.record_pinning_site((declaring, method.to_string()), &rekeyed, span, context);
        }
    }

    /// Settle every requirement, then report each site that pins a required
    /// parameter to a type that cannot meet the requirement.
    ///
    /// Runs once the body pass has recorded every requirement and every site,
    /// so a site is answered the same wherever it is written relative to the
    /// body it pins.
    /// `context` carries the program's global scope. An obligation is answered
    /// only once every type in it is concrete — a parameter pinned to another
    /// parameter is handed on instead — so nothing named there is body-local,
    /// and the global scope is the whole scope the answer needs.
    pub(crate) fn answer_pinning_sites(&mut self, context: &Context) {
        let sites = std::mem::take(&mut self.pinning_sites);
        let mut requirements = std::mem::take(&mut self.instantiation_requirements);
        settle_requirements(&mut requirements, &sites);
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
    fn answer_pinning_site(
        &mut self,
        stated: &[Obligation],
        site: &PinningSite,
        context: &Context,
    ) {
        for obligation in stated {
            match obligation {
                Obligation::Ordering { parameter } => self.answer_ordering(parameter, site),
                Obligation::Arithmetic(written) => self.answer_arithmetic(written, site, context),
            }
        }
    }

    /// Report `site` when the operator the body wrote has no meaning at the
    /// types the site pins its operands to.
    ///
    /// The judgment is the body's own arithmetic check, replayed against the
    /// pinned operands. Asking the same question a second way would have to
    /// reproduce every ground on which arithmetic is admitted — a trait the
    /// operand's class implements, a vector broadcast over a scalar — and
    /// would refuse working programs on the ones it missed.
    fn answer_arithmetic(
        &mut self,
        written: &WrittenArithmetic,
        site: &PinningSite,
        context: &Context,
    ) {
        let pinned = concretely_pinned(&site.pins);
        let left = self.substitute_type(&written.left, &pinned);
        let right = self.substitute_type(&written.right, &pinned);
        if self.is_unsettled(&left) || self.is_unsettled(&right) {
            return;
        }
        let Err(message) = self.check_arithmetic_op(&left, &written.op, &right, context) else {
            return;
        };
        let Some(parameter) = pinned_parameter_spelled_in(written, &site.pins) else {
            return;
        };
        let help = format!(
            "'{}' applies '{}' to its '{}' parameter, so the type it is instantiated with has to \
             support it",
            site.callee.1,
            crate::ast::formatter::helpers::binary_operator(written.op),
            parameter
        );
        self.report_error_with_help(DiagnosticCode::TypTypeMismatch, message, site.span, help);
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

    /// Report `site` when it pins `parameter` to a type carrying no ordering.
    fn answer_ordering(&mut self, parameter: &str, site: &PinningSite) {
        let Some(Pin::Concrete(pinned)) = site.pins.get(parameter) else {
            return;
        };
        if self.orders_its_values(pinned) {
            return;
        }
        self.report_error_with_help(
            DiagnosticCode::TypOrderingNotSupported,
            missing_ordering_at_instantiation_message(pinned),
            site.span,
            format!(
                "'{}' orders its '{}' parameter, so the type it is instantiated with has to \
                 define 'compare'",
                site.callee.1, parameter
            ),
        );
    }

    /// Re-key a receiver's substitution into `declaring`'s own parameter names.
    ///
    /// `bindings` maps each directly-implemented trait's parameter to the
    /// argument the class binds it to, written in the class's parameter terms;
    /// resolving that argument through the receiver's substitution yields the
    /// concrete type. A parameter with no binding keeps its own name, which is
    /// how a trait whose parameter the class names identically still resolves.
    fn rekeyed_into(
        &self,
        declaring: &str,
        substitution: &HashMap<String, Type>,
        bindings: &HashMap<String, Type>,
    ) -> HashMap<String, Type> {
        let Some(TypeDefinition::Trait(trait_def)) =
            self.type_table.global_type_definitions.get(declaring)
        else {
            return substitution.clone();
        };
        let Some(generics) = trait_def.generics.as_ref() else {
            return HashMap::new();
        };
        let mut rekeyed = HashMap::new();
        for generic in generics {
            let class_side = bindings.get(&generic.name).cloned().unwrap_or_else(|| {
                Type::new(
                    TypeKind::Generic(generic.name.clone(), None, TypeDeclarationKind::None),
                    Span::new(0, 0),
                )
            });
            let resolved = self.substitute_type(&class_side, substitution);
            rekeyed.insert(generic.name.clone(), resolved);
        }
        rekeyed
    }

    /// The traits a receiver of type `class_name` inherits methods from,
    /// each named once.
    fn declaring_types_above(&self, class_name: &str) -> Vec<String> {
        let Some(TypeDefinition::Class(class_def)) =
            self.type_table.global_type_definitions.get(class_name)
        else {
            return Vec::new();
        };
        let mut pending: Vec<String> = class_def.traits.clone();
        let mut seen: Vec<String> = Vec::new();
        while let Some(name) = pending.pop() {
            if seen.contains(&name) {
                continue;
            }
            if let Some(TypeDefinition::Trait(trait_def)) =
                self.type_table.global_type_definitions.get(&name)
            {
                pending.extend(trait_def.parent_traits.iter().cloned());
            }
            seen.push(name);
        }
        seen
    }
}
