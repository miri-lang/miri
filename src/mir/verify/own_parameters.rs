// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic runtime entry point is one symbol at every instantiation of its
//! own type parameters, so every value a call hands it at one of them, and the
//! value it hands back there, must be one type. Lowering records the call's
//! instantiation on the callee's function operand; this rule does not trust
//! that record. From the declaration's effect alone (which positions are
//! declared as which parameter) it checks that the operands typed by one
//! parameter, and the destination, agree in width; that the recorded
//! signature spells one type for each parameter; and, in a body compiled for
//! one instantiation, that the call records one at all and leaves none of the
//! entry point's own parameters open.

use super::{concrete_operand_type, local_display_name, pinned_type, VerificationViolation};
use crate::ast::types::{Type, TypeKind};
use crate::mir::lowering::constructors::compute_elem_size_from_type;
use crate::mir::operand::Operand;
use crate::mir::place::Place;
use crate::mir::terminator::TerminatorKind;
use crate::mir::{Body, Local};
use crate::runtime_effects::RuntimeEffect;

/// Report every call to a generic runtime entry point whose values at one of
/// its own type parameters disagree, whose recorded instantiation spells two
/// types for one parameter, or, in a body with no open parameters, records
/// no instantiation or leaves a parameter open.
pub fn verify_own_parameter_instances(body: &Body) -> Vec<VerificationViolation> {
    let mut violations = Vec::new();
    for block in &body.basic_blocks {
        let Some(terminator) = &block.terminator else {
            continue;
        };
        let TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } = &terminator.kind
        else {
            continue;
        };
        let Some(effect) = func.called_runtime_effect() else {
            continue;
        };
        if effect.type_parameters.is_empty() {
            continue;
        }
        let call = OwnParameterCall {
            symbol: func.called_runtime_name().unwrap_or_default(),
            effect,
            args,
            destination,
            recorded: recorded_signature(func),
        };
        violations.extend(call.violations(body));
    }
    violations
}

/// One call to a generic runtime entry point, as this rule reads it.
struct OwnParameterCall<'a> {
    symbol: &'a str,
    effect: &'a RuntimeEffect,
    args: &'a [Operand],
    destination: &'a Place,
    /// The parameter types and the result the callee operand records.
    recorded: Option<(Vec<Option<Type>>, Option<Type>)>,
}

impl OwnParameterCall<'_> {
    fn violations(&self, body: &Body) -> Vec<VerificationViolation> {
        let mut violations = Vec::new();
        for (parameter, name) in self.effect.type_parameters.iter().enumerate() {
            violations.extend(self.width_disagreement(body, parameter, name));
            violations.extend(self.recorded_disagreement(body, parameter, name));
        }
        if body.generics.open_params.is_empty() {
            violations.extend(self.open_instantiation(body));
        }
        violations
    }

    /// The positions declared as the type parameter `parameter`, bare.
    fn positions(&self, parameter: usize) -> impl Iterator<Item = usize> + '_ {
        (self.effect.bare_parameter_positions())
            .filter(move |&(_, typed_by)| typed_by == parameter)
            .map(|(position, _)| position)
    }

    /// Whether the result is declared as the type parameter `parameter`.
    fn result_is(&self, parameter: usize) -> bool {
        self.effect.result_typed_by == Some(parameter)
    }

    /// A finding when the operands and the destination typed by `parameter`
    /// are not all one width.
    fn width_disagreement(
        &self,
        body: &Body,
        parameter: usize,
        name: &str,
    ) -> Option<VerificationViolation> {
        let operands = (self.positions(parameter))
            .filter_map(|position| self.args.get(position))
            .filter_map(|operand| concrete_operand_type(operand, body));
        let destination = (self.result_is(parameter) && self.destination.projection.is_empty())
            .then(|| pinned_type(&body.local_decls[self.destination.local.0].ty, body))
            .flatten();
        let widths: Vec<(Type, i64)> = (operands.chain(destination))
            .map(|ty| {
                let bytes = compute_elem_size_from_type(&ty.kind, body.layout_scope());
                (ty, bytes)
            })
            .collect();
        let (first, rest) = widths.split_first()?;
        let other = rest.iter().find(|(_, bytes)| *bytes != first.1)?;
        Some(self.violation(
            body,
            format!(
                "`{}` is handed a {} of {} bytes and a {} of {} bytes at its one type parameter \
                 `{name}`, which the entry point reads as one width",
                self.symbol, first.0.kind, first.1, other.0.kind, other.1
            ),
        ))
    }

    /// A finding when the recorded instantiation spells two types for
    /// `parameter` across the positions and the result declared as it.
    fn recorded_disagreement(
        &self,
        body: &Body,
        parameter: usize,
        name: &str,
    ) -> Option<VerificationViolation> {
        let (params, result) = self.recorded.as_ref()?;
        let mut spelled = (self.positions(parameter))
            .filter_map(|position| params.get(position).cloned().flatten())
            .chain(result.clone().filter(|_| self.result_is(parameter)))
            .map(|ty| ty.kind.to_string());
        let first = spelled.next()?;
        let second = spelled.find(|other| *other != first)?;
        Some(self.violation(
            body,
            format!(
                "`{}` is recorded at `{name}` = {first} and at `{name}` = {second}, so its \
                 recorded instantiation is no substitution of its declaration",
                self.symbol
            ),
        ))
    }

    /// A finding when this call, in a body compiled for one instantiation,
    /// records no instantiation or one still naming an own parameter.
    fn open_instantiation(&self, body: &Body) -> Option<VerificationViolation> {
        let Some((params, _)) = &self.recorded else {
            return Some(self.violation(
                body,
                format!("`{}` is called with no instantiation recorded", self.symbol),
            ));
        };
        let open = (self.effect.bare_parameter_positions())
            .filter_map(|(position, _)| params.get(position).cloned().flatten())
            .find(|ty| self.names_own_parameter(ty))?;
        Some(self.violation(
            body,
            format!(
                "`{}` is called at its own type parameter {}, left open in a body compiled \
                 for one instantiation",
                self.symbol, open.kind
            ),
        ))
    }

    fn names_own_parameter(&self, ty: &Type) -> bool {
        matches!(&ty.kind, TypeKind::Custom(name, None) | TypeKind::Generic(name, _)
            if self.effect.type_parameters.contains(name))
    }

    fn violation(&self, body: &Body, message: String) -> VerificationViolation {
        let local = (self.args.first())
            .and_then(Operand::bare_local)
            .unwrap_or(Local(0));
        VerificationViolation {
            local,
            local_name: local_display_name(body, local),
            message,
        }
    }
}

/// The parameter types and the result the function operand `func` records
/// for its call; `None` when it records no signature.
fn recorded_signature(func: &Operand) -> Option<(Vec<Option<Type>>, Option<Type>)> {
    let Operand::Function(function) = func else {
        return None;
    };
    let TypeKind::Function(signature) = &function.ty.kind else {
        return None;
    };
    let params = (signature.params.iter())
        .map(|param| crate::type_checker::resolve_type_name(&param.typ))
        .collect();
    let result =
        (signature.return_type.as_deref()).and_then(crate::type_checker::resolve_type_name);
    Some((params, result))
}
