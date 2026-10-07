// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which instantiation of a generic function a call reaches.
//!
//! A call binds the callee's type parameters from the arguments it writes out
//! (`make<String>()`), then from the values it passes. A parameter neither
//! names — one only the return type mentions — is left open, and the call is
//! parked until the location its result goes into supplies the type: a declared
//! binding, a parameter, a return type. A call still open when the statement
//! holding it is done is refused, because no body can be compiled for it: a
//! value laid out for the bare parameter holds nothing the caller reads it as.
//!
//! While a call is open, its result is typed with a slot standing for each
//! unbound parameter. A slot is named after the call, in a spelling no
//! identifier can take, so it is never read as a parameter the calling body
//! declares under the same name, and nothing it reaches can complete at it.

use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::TypeDeclarationKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::diagnostics::spelled;
use crate::type_checker::instantiation_requirements::{spells_a_type, GenericBodyId};
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};

/// A bound a generic function declares on one of its type parameters —
/// `T implements Named` — spelled in the callee's own parameters.
#[derive(Debug, Clone)]
pub(crate) struct ParameterBound {
    pub(crate) parameter: String,
    pub(crate) constraint: Type,
    pub(crate) kind: TypeDeclarationKind,
}

/// A call to a generic function, with what it has bound of the callee's type
/// parameters so far.
#[derive(Debug, Clone)]
pub(crate) struct GenericCall {
    /// The callee's name as the call writes it, for diagnostics; `None` for a
    /// callee written as an expression rather than a name.
    pub(crate) callee: Option<String>,
    /// The body whose requirements the call answers, when the call is to a
    /// free function; a method's requirements are answered through its
    /// receiver.
    pub(crate) body: Option<GenericBodyId>,
    /// The callee's type parameters, in declaration order.
    pub(crate) parameters: Vec<String>,
    /// The bounds the callee declares on its parameters.
    pub(crate) bounds: Vec<ParameterBound>,
    /// The callee's return type, spelled in its own parameters.
    pub(crate) declared_return: Type,
    /// What each bound parameter is bound to.
    pub(crate) bound: HashMap<String, Type>,
    pub(crate) span: Span,
}

/// A generic call still waiting for a parameter to be bound.
///
/// `opened` orders it, so a statement refuses only the calls written inside
/// it; `errors_at_open` is how many errors had been reported when it opened,
/// so its refusal withdraws only what was reported against its slots since.
#[derive(Debug)]
pub(crate) struct OpenGenericCall {
    call: GenericCall,
    opened: usize,
    errors_at_open: usize,
    /// Set once the call was stored where no binding of its parameters can
    /// fit: the mismatch reported there is the call's one error.
    mismatched: bool,
}

/// The generic calls opened so far and not yet bound, keyed by call id.
#[derive(Debug, Default)]
pub(crate) struct OpenGenericCalls {
    calls: HashMap<usize, OpenGenericCall>,
    opened: usize,
}

impl OpenGenericCalls {
    /// The position the next opened call takes. A statement reads it before
    /// its expressions are checked; every call opened inside it is at or past.
    pub(crate) fn mark(&self) -> usize {
        self.opened
    }

    pub(crate) fn contains(&self, call_id: usize) -> bool {
        self.calls.contains_key(&call_id)
    }

    /// The id of every call still open.
    fn ids(&self) -> Vec<usize> {
        self.calls.keys().copied().collect()
    }

    fn open(&mut self, call_id: usize, call: GenericCall, errors_at_open: usize) {
        let opened = self.opened;
        self.opened += 1;
        let open = OpenGenericCall {
            call,
            opened,
            errors_at_open,
            mismatched: false,
        };
        self.calls.insert(call_id, open);
    }

    fn take(&mut self, call_id: usize) -> Option<OpenGenericCall> {
        self.calls.remove(&call_id)
    }

    /// Mark the open call `call_id` as stored where it cannot fit.
    fn mark_mismatched(&mut self, call_id: usize) -> Option<&GenericCall> {
        let open = self.calls.get_mut(&call_id)?;
        open.mismatched = true;
        Some(&open.call)
    }

    /// Put back a call taken to be bound further, at the position it was
    /// first opened in.
    fn restore(&mut self, call_id: usize, open: OpenGenericCall) {
        self.calls.insert(call_id, open);
    }

    /// Remove and return every call opened at or after `mark`, in the order
    /// they were opened.
    fn take_since(&mut self, mark: usize) -> Vec<(usize, OpenGenericCall)> {
        let ids: Vec<usize> = self
            .calls
            .iter()
            .filter(|(_, open)| open.opened >= mark)
            .map(|(id, _)| *id)
            .collect();
        let mut taken: Vec<(usize, OpenGenericCall)> = ids
            .into_iter()
            .filter_map(|id| self.calls.remove(&id).map(|open| (id, open)))
            .collect();
        taken.sort_by_key(|(_, open)| open.opened);
        taken
    }
}

impl GenericCall {
    /// The parameters nothing has bound yet: those with no binding, and those
    /// bound to a type still holding another open call's slot.
    fn unbound_parameters(&self) -> Vec<&str> {
        self.parameters
            .iter()
            .filter(|name| {
                self.bound
                    .get(name.as_str())
                    .is_none_or(holds_an_inference_slot)
            })
            .map(String::as_str)
            .collect()
    }

    /// The slots standing for this call's parameters that have no binding.
    fn slot_names(&self, call_id: usize) -> Vec<String> {
        self.parameters
            .iter()
            .filter(|name| !self.bound.contains_key(name.as_str()))
            .map(|name| inference_slot_name(name, call_id))
            .collect()
    }

    /// Whether this call waits on a slot of one of the calls in `slots`.
    fn waits_on(&self, slots: &HashSet<String>) -> bool {
        self.bound.values().any(|ty| {
            spells_a_type(
                &ty.kind,
                &|kind| matches!(kind, TypeKind::Generic(name, _, _) if slots.contains(name)),
            )
        })
    }

    /// Whether the callee's return type mentions the parameter `name`.
    fn returns_parameter(&self, name: &str) -> bool {
        spells_a_type(&self.declared_return.kind, &|kind| {
            matches!(kind, TypeKind::Generic(spelled, _, _) | TypeKind::Custom(spelled, None)
                if spelled == name)
        })
    }
}

/// The slot standing for the parameter `parameter` of the open call `call_id`.
/// The quotes make it a spelling no identifier can take.
fn inference_slot_name(parameter: &str, call_id: usize) -> String {
    format!("{parameter}'{call_id}'")
}

/// Whether `name` spells a slot of an open generic call.
pub(crate) fn is_inference_slot(name: &str) -> bool {
    name.ends_with('\'')
}

/// Whether `ty` holds a slot of an open generic call anywhere in it.
pub(crate) fn holds_an_inference_slot(ty: &Type) -> bool {
    spells_a_type(
        &ty.kind,
        &|kind| matches!(kind, TypeKind::Generic(name, _, _) if is_inference_slot(name)),
    )
}

/// `type argument` or `type arguments`, as `count` needs.
pub(crate) fn type_arguments_noun(count: usize) -> &'static str {
    if count == 1 {
        "type argument"
    } else {
        "type arguments"
    }
}

impl TypeChecker {
    /// Settle `call` if every type parameter is bound; otherwise park it until
    /// the location its result goes into binds the rest. Returns the call's
    /// type as far as it is known, a slot standing for each unbound parameter.
    pub(crate) fn instantiate_generic_call(
        &mut self,
        call_id: usize,
        call: GenericCall,
        context: &Context,
    ) -> Type {
        if call.unbound_parameters().is_empty() {
            return self.complete_call_instantiation(call_id, &call, context);
        }
        let partial = self.partial_return_type(call_id, &call);
        let errors_now = self.diagnostics.errors.len();
        self.open_generic_calls.open(call_id, call, errors_now);
        partial
    }

    /// The return type of the open `call`, with each parameter nothing binds
    /// spelled as the call's own slot.
    fn partial_return_type(&self, call_id: usize, call: &GenericCall) -> Type {
        let mut substitution = call.bound.clone();
        for name in &call.parameters {
            if !substitution.contains_key(name) {
                let slot = TypeKind::Generic(
                    inference_slot_name(name, call_id),
                    None,
                    TypeDeclarationKind::None,
                );
                substitution.insert(name.clone(), make_type(slot));
            }
        }
        self.substitute_type(&call.declared_return, &substitution)
    }

    /// Record what a call to a variant constructor binds of its enum's
    /// parameters. The constructor compiles no body, so an argument left
    /// unbound is no instantiation missing: the value is laid out where it is
    /// stored, at the type that location declares.
    pub(crate) fn record_variant_constructor_call(
        &mut self,
        call_id: usize,
        parameters: &[String],
        declared_return: &Type,
        bound: &HashMap<String, Type>,
    ) -> Type {
        // A payload argument that is itself an open call binds nothing yet:
        // its slot is settled where the value is stored, never recorded.
        let settled: HashMap<String, Type> = bound
            .iter()
            .filter(|(_, ty)| !holds_an_inference_slot(ty))
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        self.record_call_mapping(call_id, parameters, &settled);
        self.substitute_type(declared_return, bound)
    }

    /// Record the instantiation a fully bound `call` reaches: the mapping MIR
    /// lowering mangles the callee with, the call's type at it, and the site
    /// the callee body's requirements are answered at. A binding that breaks
    /// a bound the callee declares is refused, and the call typed as an error.
    fn complete_call_instantiation(
        &mut self,
        call_id: usize,
        call: &GenericCall,
        context: &Context,
    ) -> Type {
        self.open_generic_calls.take(call_id);
        if !self.bounds_hold(call, context) {
            let error = make_type(TypeKind::Error);
            self.type_table.types.insert(call_id, error.clone());
            return error;
        }
        self.record_call_mapping(call_id, &call.parameters, &call.bound);
        if let Some(body) = &call.body {
            self.record_pinning_site(body.clone(), &call.bound, call.span, context);
        }
        let return_type = self.substitute_type(&call.declared_return, &call.bound);
        self.type_table.types.insert(call_id, return_type.clone());
        return_type
    }

    /// Whether every parameter of `call` is bound to a type meeting the bound
    /// the callee declares on it; each one that is not is reported.
    fn bounds_hold(&mut self, call: &GenericCall, context: &Context) -> bool {
        let mut hold = true;
        for bound in &call.bounds {
            let Some(argument) = call.bound.get(&bound.parameter) else {
                continue;
            };
            let constraint = self.substitute_type(&bound.constraint, &call.bound);
            let subject = bounded_subject(argument, context);
            if self.satisfies_constraint(&subject, &constraint, &bound.kind, context) {
                continue;
            }
            hold = false;
            if let TypeKind::OneOf(_) = &constraint.kind {
                self.report_outside_type_set(&bound.parameter, argument, &constraint, call.span);
                continue;
            }
            self.report_error(
                DiagnosticCode::TypGenericTypeStructure,
                format!(
                    "Type {} does not satisfy constraint {} {}",
                    spelled(argument),
                    bound.kind,
                    spelled(&constraint)
                ),
                call.span,
            );
        }
        hold
    }

    /// Reports a call binding `parameter` to a type outside the set bounding
    /// it. A parameter the signature shorthand introduced is named after its
    /// set, which is how the reader wrote it.
    fn report_outside_type_set(
        &mut self,
        parameter: &str,
        argument: &Type,
        set: &Type,
        span: Span,
    ) {
        let written = parameter
            .strip_prefix(crate::type_checker::type_set_shorthand::PARAMETER_MARK)
            .unwrap_or(parameter);
        self.report_error_with_help(
            DiagnosticCode::TypGenericTypeStructure,
            format!(
                "{} is not one of the types {written} accepts: {}",
                spelled(argument),
                spelled(set)
            ),
            span,
            format!(
                "pass a value of one of {}, converting it with `as` if needed",
                spelled(set)
            ),
        );
    }

    /// Record, in the callee's declaration order, what `call_id` binds of
    /// `parameters` — the mapping MIR lowering reads the call's target from.
    fn record_call_mapping(
        &mut self,
        call_id: usize,
        parameters: &[String],
        bound: &HashMap<String, Type>,
    ) {
        let ordered: Vec<(String, Type)> = parameters
            .iter()
            .filter_map(|name| bound.get(name).map(|ty| (name.clone(), ty.clone())))
            .collect();
        if !ordered.is_empty() {
            self.call_generic_mappings.insert(call_id, ordered);
        }
    }

    /// Bind the parameters the open call `call_id` left unbound from the type
    /// of the location its result is stored at, and settle it if that binds
    /// them all; otherwise it stays parked where it was first opened.
    ///
    /// Only a type the location names in the caller's own terms binds: a slot
    /// of another open call names nothing the caller can compile a body at.
    pub(crate) fn bind_open_call_from_expected(
        &mut self,
        call_id: usize,
        expected: &Type,
        context: &Context,
    ) {
        let Some(mut open) = self.open_generic_calls.take(call_id) else {
            return;
        };
        let inferred = self.parameters_bound_by(&open.call, expected);
        let unbound: HashSet<String> = open
            .call
            .unbound_parameters()
            .into_iter()
            .map(str::to_string)
            .collect();
        // A parameter an argument bound to another open call's slot learns
        // what that slot is from the location too: `pass(make())` stored at a
        // `Box<String>` binds `make`'s parameter through `pass`'s.
        let mut slots = HashMap::new();
        for (name, ty) in inferred {
            if unbound.contains(&name) && self.names_only_types_in_scope(&ty, context) {
                if let Some(held) = open.call.bound.get(&name) {
                    self.infer_generic_types(held, &ty, &mut slots);
                }
                open.call.bound.insert(name, ty);
            }
        }
        if open.call.unbound_parameters().is_empty() {
            self.complete_call_instantiation(call_id, &open.call, context);
        } else {
            self.open_generic_calls.restore(call_id, open);
        }
        self.bind_open_call_slots(&slots, context);
    }

    /// Bind each open call's parameter whose slot `slots` names a type for,
    /// and settle each call that leaves no parameter unbound.
    fn bind_open_call_slots(&mut self, slots: &HashMap<String, Type>, context: &Context) {
        let slots: HashMap<&String, &Type> = slots
            .iter()
            .filter(|(name, ty)| is_inference_slot(name) && !holds_an_inference_slot(ty))
            .collect();
        if slots.is_empty() {
            return;
        }
        for call_id in self.open_generic_calls.ids() {
            let Some(mut open) = self.open_generic_calls.take(call_id) else {
                continue;
            };
            for name in open.call.parameters.clone() {
                let slot = inference_slot_name(&name, call_id);
                let unbound = open
                    .call
                    .bound
                    .get(&name)
                    .is_none_or(holds_an_inference_slot);
                if let (true, Some(ty)) = (unbound, slots.get(&slot)) {
                    open.call.bound.insert(name, (*ty).clone());
                }
            }
            if open.call.unbound_parameters().is_empty() {
                self.complete_call_instantiation(call_id, &open.call, context);
            } else {
                self.open_generic_calls.restore(call_id, open);
            }
        }
    }

    /// What storing the result of `call` at `expected` binds of the callee's
    /// parameters. A value stored where an optional is declared is wrapped on
    /// the way, so it is the payload type that binds.
    fn parameters_bound_by(&self, call: &GenericCall, expected: &Type) -> HashMap<String, Type> {
        let returns_an_optional = matches!(call.declared_return.kind, TypeKind::Option(_));
        let expected =
            if let (TypeKind::Option(inner), false) = (&expected.kind, returns_an_optional) {
                inner.as_ref()
            } else {
                expected
            };
        let declared = self
            .declared_return_as(&call.declared_return, expected)
            .unwrap_or_else(|| call.declared_return.clone());
        let mut inferred = HashMap::new();
        self.infer_generic_types(&declared, expected, &mut inferred);
        inferred
    }

    /// `declared`, a class or trait the callee returns, read as the type
    /// above it that `expected` names, at the arguments its `extends`,
    /// `implements` and parent-trait clauses reach that type at: a `make<T>`
    /// returning an `Impl<T>` stored where an `Op<String>` is declared binds
    /// `T` through `class Impl<T> implements Op<T>`. `None` when `expected`
    /// names the same type, or one not above it.
    fn declared_return_as(&self, declared: &Type, expected: &Type) -> Option<Type> {
        let (TypeKind::Custom(sub, sub_arguments), TypeKind::Custom(sup, _)) =
            (&declared.kind, &expected.kind)
        else {
            return None;
        };
        if sub == sup {
            return None;
        }
        let arguments = self
            .supertype_arguments(sub, sub_arguments.as_deref(), sup)?
            .into_iter()
            .collect::<Option<Vec<Type>>>()?;
        let arguments = arguments
            .into_iter()
            .map(|ty| self.create_type_expression(ty))
            .collect::<Vec<_>>();
        let arguments = (!arguments.is_empty()).then_some(arguments);
        Some(Type::new(
            TypeKind::Custom(sup.clone(), arguments),
            declared.span,
        ))
    }

    /// Record that the open call `call_id` was refused at a location of type
    /// `expected` no binding of its parameters fits — `takes(make())` with
    /// `takes(x int)` and `make` returning a `Box<T>`. That mismatch is the
    /// call's one error: it is not refused again for its unbound parameters,
    /// and the call reads back at its declared return type, so the mismatch
    /// names the type the callee declares rather than the call's slots.
    pub(crate) fn note_open_call_mismatch(&mut self, call_id: usize, expected: &Type) {
        if holds_an_inference_slot(expected) {
            return;
        }
        let Some(call) = self.open_generic_calls.mark_mismatched(call_id) else {
            return;
        };
        let (declared_return, bound) = (call.declared_return.clone(), call.bound.clone());
        let declared = self.substitute_type(&declared_return, &bound);
        self.type_table.types.insert(call_id, declared);
    }

    /// The type `expr` holds once the location it was stored at has bound any
    /// open call it is: `before` still spells that call's slots, and
    /// judging or reporting it would describe a type the value no longer has.
    pub(crate) fn settled_value_type(&self, expr: &crate::ast::Expression, before: Type) -> Type {
        if !holds_an_inference_slot(&before) {
            return before;
        }
        self.get_type(expr.id).cloned().unwrap_or(before)
    }

    /// The position the next opened call takes; see [`OpenGenericCalls::mark`].
    pub(crate) fn open_generic_call_mark(&self) -> usize {
        self.open_generic_calls.mark()
    }

    /// Refuse every generic call opened at or after `mark` that nothing bound,
    /// typing each as an error. The refusal is the one error each such call
    /// gets: what was reported against its slots since it opened is withdrawn,
    /// and a call left open only because it was handed another refused call's
    /// result is not reported again. Returns whether any call was refused.
    pub(crate) fn refuse_open_generic_calls_since(&mut self, mark: usize) -> bool {
        let refused = self.open_generic_calls.take_since(mark);
        if refused.is_empty() {
            return false;
        }
        self.withdraw_errors_against_slots(&refused);
        let refused_slots: HashSet<String> = refused
            .iter()
            .flat_map(|(call_id, open)| open.call.slot_names(*call_id))
            .collect();
        for (call_id, open) in &refused {
            if !open.mismatched && !open.call.waits_on(&refused_slots) {
                self.report_unbound_call(&open.call);
            }
            self.type_table
                .types
                .insert(*call_id, make_type(TypeKind::Error));
        }
        true
    }

    /// Withdraw every error reported since each of `refused` opened whose
    /// message names one of its slots: it describes a type the refusal says
    /// was never determined.
    fn withdraw_errors_against_slots(&mut self, refused: &[(usize, OpenGenericCall)]) {
        let withdrawn: Vec<(usize, Vec<String>)> = refused
            .iter()
            .map(|(call_id, open)| (open.errors_at_open, open.call.slot_names(*call_id)))
            .collect();
        let mut index = 0;
        self.diagnostics.errors.retain(|error| {
            let position = index;
            index += 1;
            let message = error.to_string();
            !withdrawn.iter().any(|(errors_at_open, slots)| {
                position >= *errors_at_open && slots.iter().any(|slot| message.contains(slot))
            })
        });
    }

    /// Whether every type parameter `ty` names is one the scope in `context`
    /// declares — a parameter spelled bare, or as a name no type answers to.
    /// A slot of an open call is declared by no scope, so a type holding one
    /// never passes.
    fn names_only_types_in_scope(&self, ty: &Type, context: &Context) -> bool {
        let is_parameter_in_scope = |name: &str| {
            matches!(
                context.resolve_type_definition(name),
                Some(TypeDefinition::Generic(_))
            )
        };
        let names_no_type = |name: &str| {
            context.resolve_type_definition(name).is_none()
                && !self.type_table.global_type_definitions.contains_key(name)
        };
        !spells_a_type(&ty.kind, &|kind| {
            matches!(kind, TypeKind::Generic(name, _, _) if !is_parameter_in_scope(name))
                || matches!(kind, TypeKind::Custom(name, None) if names_no_type(name))
        })
    }

    fn report_unbound_call(&mut self, call: &GenericCall) {
        let unbound = call.unbound_parameters();
        let named = format!(
            "{} `{}`",
            type_arguments_noun(unbound.len()),
            unbound.join("`, `")
        );
        let returned = unbound.iter().any(|name| call.returns_parameter(name));
        let message = match &call.callee {
            Some(callee) => {
                let written = format!("`{callee}<{}>(...)`", call.parameters.join(", "));
                if returned {
                    format!(
                        "Cannot infer {named} of `{callee}`: no argument determines it. Write \
                         the type arguments out — {written} — or declare the type of the \
                         location the result is stored in"
                    )
                } else {
                    format!(
                        "Cannot infer {named} of `{callee}`: neither an argument nor the return \
                         type mentions it. Write the type arguments out — {written}"
                    )
                }
            }
            None => format!(
                "Cannot infer {named} of the called function: no argument determines it. \
                 Declare the type of the location the result is stored in"
            ),
        };
        self.report_error(DiagnosticCode::TypTypeInference, message, call.span);
    }
}

/// The type a bound is checked against for `argument`: a parameter of the
/// calling body stands for whatever meets that parameter's own bound, so it
/// is its bound that must meet the callee's.
fn bounded_subject(argument: &Type, context: &Context) -> Type {
    let TypeKind::Generic(name, _, _) = &argument.kind else {
        return argument.clone();
    };
    match context.resolve_type_definition(name) {
        Some(TypeDefinition::Generic(definition)) => definition
            .constraint
            .clone()
            .unwrap_or_else(|| argument.clone()),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Class(_)
            | TypeDefinition::Trait(_)
            | TypeDefinition::Alias(_),
        )
        | None => argument.clone(),
    }
}
