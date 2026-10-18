// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that a bitwise value held as its bytes is treated as bytes.
//!
//! A *value place* is one whose declared type the value-class authority calls
//! bitwise and whose body's classifier does not count. Such a value lives in
//! the frame that holds it, so:
//!
//! - no statement adjusts its count: an increment, a decrement or a
//!   deallocation of it names a count it does not have;
//! - its address is a stack address, which may be handed to a call but never
//!   stored into a word slot — a field, an element, a payload, a capture — or
//!   returned, where it would outlive the frame;
//! - it never meets a counted word: wherever one value flows into another — a
//!   use, a cast, a slot of an aggregate, an argument and its parameter, a
//!   call result and its destination — both sides are bytes or neither is.
//!
//! Which values are held as their bytes is the body's to say
//! ([`Body::value_layout`]); an optional is not one, and is checked by the
//! optional-representation rule instead.

use super::optional_representation::Layouts;
use super::{local_display_name, VerificationViolation};
use crate::ast::types::{Type, TypeKind};
use crate::mir::rvalue::{AggregateKind, Rvalue};
use crate::mir::statement::StatementKind;
use crate::mir::symbol::Symbol;
use crate::mir::terminator::TerminatorKind;
use crate::mir::value_arguments::projected_value_arguments;
use crate::mir::{Body, Local, Operand, Place};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::value_class::{value_class, LayoutScope, ValueClass, WordRole};
use std::collections::HashMap;

/// Every count adjustment of a value place in `body`, every escape of a value
/// place's address into a slot that outlives the frame, and every place a
/// value place meets a counted word.
///
/// A GPU body is skipped: it keeps no counts and its values are laid out by
/// the shader backends.
pub fn verify_value_representation(
    body: &Body,
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let rule = Representations::new(body, definitions);
    let mut violations: Vec<VerificationViolation> = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .flat_map(|statement| rule.statement_violations(&statement.kind))
        .collect();
    violations.extend(projected_argument_violations(body));
    violations
}

/// Every value argument a call takes through a projection rather than from
/// a whole local: its address would point inside its holder, which the
/// callee may write or free while it reads.
fn projected_argument_violations(body: &Body) -> Vec<VerificationViolation> {
    (0..body.basic_blocks.len())
        .flat_map(|block| {
            let Some(TerminatorKind::Call {
                args, destination, ..
            }) = body.basic_blocks[block]
                .terminator
                .as_ref()
                .map(|terminator| &terminator.kind)
            else {
                return Vec::new();
            };
            projected_value_arguments(body, block)
                .into_iter()
                .filter_map(|position| {
                    Some(VerificationViolation {
                        local: destination.local,
                        local_name: local_display_name(body, destination.local),
                        message: format!(
                            "argument {position}, `{}`, a value held as its bytes, is passed \
                             by address from inside its holder rather than from a whole local",
                            args.get(position)?
                        ),
                    })
                })
                .collect()
        })
        .collect()
}

/// Every static call whose argument or destination holds a value differently
/// from the callee's parameter or return value — one as bytes, the other as a
/// word — paired with the function making it.
///
/// A call with no static callee, or to a function with no body here, is
/// skipped, as is any call a GPU body makes.
pub fn verify_value_call_classes(
    program: &[(Symbol, Body)],
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<(Symbol, VerificationViolation)> {
    let mut callees: HashMap<&Symbol, &Body> = HashMap::new();
    for (symbol, body) in program {
        callees.entry(symbol).or_insert(body);
    }
    program
        .iter()
        .filter(|(_, body)| !body.is_gpu())
        .flat_map(|(symbol, body)| {
            let rule = Representations::new(body, definitions);
            body.basic_blocks
                .iter()
                .filter_map(|block| block.terminator.as_ref())
                .flat_map(|terminator| rule.call_violations(&terminator.kind, &callees))
                .map(move |violation| (symbol.clone(), violation))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether a value of `kind`, written in `body`, is held as its bytes.
fn holds_value(body: &Body, kind: &TypeKind) -> bool {
    body.value_layout(kind).is_some()
}

/// How a value of `kind`, written in `body`, is held, in words.
fn describe(
    body: &Body,
    definitions: &HashMap<String, TypeDefinition>,
    kind: &TypeKind,
) -> &'static str {
    if matches!(kind, TypeKind::Void) {
        return "no value";
    }
    match value_class(kind, LayoutScope::new(definitions, &body.type_params)) {
        ValueClass::Scalar(_) => "a scalar",
        ValueClass::Bitwise(_) if body.is_counted(kind) => "a counted bitwise value",
        ValueClass::Bitwise(_) => "a bitwise value held as its bytes",
        ValueClass::Word(WordRole::NullablePointer) | ValueClass::Managed => "a counted word",
        ValueClass::Word(WordRole::Parameter | WordRole::Untracked) => "an uncounted word",
    }
}

/// Reads one body's places as values or words, knowing which of its locals
/// hold the address of a value place.
struct Representations<'a> {
    layouts: Layouts<'a>,
    /// Each local holding a value place's address, to the value local whose
    /// address it holds.
    addresses: HashMap<Local, Local>,
}

impl<'a> Representations<'a> {
    fn new(body: &'a Body, definitions: &'a HashMap<String, TypeDefinition>) -> Self {
        let mut rule = Self {
            layouts: Layouts { body, definitions },
            addresses: HashMap::new(),
        };
        rule.addresses = rule.address_locals();
        rule
    }

    /// Every local a value place's address reaches, through a reference and
    /// then any chain of whole-local copies and casts, found by iterating to a
    /// fixpoint so the order of blocks does not matter.
    fn address_locals(&self) -> HashMap<Local, Local> {
        let mut addresses = HashMap::new();
        loop {
            let mut is_changed = false;
            for (dest, rvalue) in self.assignments() {
                if !dest.projection.is_empty() || addresses.contains_key(&dest.local) {
                    continue;
                }
                if let Some(value) = self.address_carried(rvalue, &addresses) {
                    addresses.insert(dest.local, value);
                    is_changed = true;
                }
            }
            if !is_changed {
                return addresses;
            }
        }
    }

    /// Every assignment in the body, as its destination and rvalue.
    fn assignments(&self) -> impl Iterator<Item = (&'a Place, &'a Rvalue)> + 'a {
        let body: &'a Body = self.layouts.body;
        body.basic_blocks
            .iter()
            .flat_map(|block| &block.statements)
            .filter_map(|statement| match &statement.kind {
                StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                    Some((dest, rvalue))
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::Nop => None,
            })
    }

    /// The value local whose address `rvalue` produces: a reference into a
    /// value local, or a copy or cast of a local already holding one.
    fn address_carried(&self, rvalue: &Rvalue, addresses: &HashMap<Local, Local>) -> Option<Local> {
        match rvalue {
            Rvalue::Ref(place) => self.is_value_local(place.local).then_some(place.local),
            Rvalue::Use(operand) => addresses.get(&operand.bare_local()?).copied(),
            Rvalue::Cast(operand, _) => addresses.get(&operand.bare_local()?).copied(),
            Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Aggregate(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => None,
        }
    }

    fn is_value_local(&self, local: Local) -> bool {
        self.layouts
            .body
            .local_decls
            .get(local.0)
            .is_some_and(|decl| self.holds_value(&decl.ty.kind))
    }

    fn holds_value(&self, kind: &TypeKind) -> bool {
        holds_value(self.layouts.body, kind)
    }

    fn describe(&self, kind: &TypeKind) -> &'static str {
        describe(self.layouts.body, self.layouts.definitions, kind)
    }

    fn violation(&self, place: &Place, message: String) -> VerificationViolation {
        VerificationViolation {
            local: place.local,
            local_name: local_display_name(self.layouts.body, place.local),
            message,
        }
    }

    /// The violations one statement raises.
    fn statement_violations(&self, kind: &StatementKind) -> Vec<VerificationViolation> {
        match kind {
            StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                let mut violations = self.escape_violations(dest, rvalue);
                violations.extend(self.meeting_violations(dest, rvalue));
                violations
            }
            StatementKind::IncRef(place) => self.count_violation(place, "IncRef"),
            StatementKind::DecRef(place) => self.count_violation(place, "DecRef"),
            StatementKind::Dealloc(place) => self.count_violation(place, "Dealloc"),
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
                Vec::new()
            }
        }
    }

    /// The violation of the count adjustment `operation` on `place`, when it
    /// is a value place.
    fn count_violation(&self, place: &Place, operation: &str) -> Vec<VerificationViolation> {
        let Some(ty) = self.layouts.place_type(place) else {
            return Vec::new();
        };
        if !self.holds_value(&ty.kind) {
            return Vec::new();
        }
        vec![self.violation(
            place,
            format!(
                "{operation}(`{place}`) adjusts the count of a {}, {}, which has no count",
                ty.kind,
                self.describe(&ty.kind)
            ),
        )]
    }

    /// The violations of writing `rvalue` into `dest` when that carries a
    /// value place's address somewhere that outlives the frame.
    fn escape_violations(&self, dest: &Place, rvalue: &Rvalue) -> Vec<VerificationViolation> {
        if let Rvalue::Aggregate(kind, operands) = rvalue {
            return operands
                .iter()
                .filter_map(|operand| self.addresses.get(&operand.bare_local()?))
                .map(|value| self.escape(dest, *value, &held_in(kind, dest)))
                .collect();
        }
        let Some(value) = self.address_carried(rvalue, &self.addresses) else {
            return Vec::new();
        };
        if !dest.projection.is_empty() {
            vec![self.escape(dest, value, &format!("stored into `{dest}`"))]
        } else if dest.local == Local(0) {
            vec![self.escape(dest, value, "returned through `_0`")]
        } else {
            Vec::new()
        }
    }

    /// The violation of the address of value local `value` being `fate`.
    fn escape(&self, dest: &Place, value: Local, fate: &str) -> VerificationViolation {
        let kind = self
            .layouts
            .body
            .local_decls
            .get(value.0)
            .map_or(String::new(), |decl| decl.ty.kind.to_string());
        self.violation(
            dest,
            format!(
                "the address of `{value}`, a {kind} held as its bytes in this frame, is {fate}, \
                 a word slot that outlives the frame"
            ),
        )
    }

    /// The violations `rvalue` raises by meeting `dest` at a different value
    /// class.
    fn meeting_violations(&self, dest: &Place, rvalue: &Rvalue) -> Vec<VerificationViolation> {
        let target = self.layouts.place_type(dest);
        match rvalue {
            Rvalue::Use(operand) => target
                .and_then(|target| self.meeting_violation(dest, operand, target, "use"))
                .into_iter()
                .collect(),
            Rvalue::Cast(operand, cast_ty) => self
                .meeting_violation(dest, operand, cast_ty, "cast")
                .into_iter()
                .collect(),
            Rvalue::Aggregate(kind, operands) => {
                let Some(target) = target else {
                    return Vec::new();
                };
                operands
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, operand)| {
                        let slot_ty = self.layouts.aggregate_slot(kind, target, slot)?;
                        self.meeting_violation(dest, operand, slot_ty, "aggregate slot")
                    })
                    .collect()
            }
            Rvalue::Ref(_)
            | Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => Vec::new(),
        }
    }

    /// The violation `operand` raises by flowing, as a `what`, into a slot of
    /// `target` type in `dest` that holds its value the other way.
    fn meeting_violation(
        &self,
        dest: &Place,
        operand: &Operand,
        target: &Type,
        what: &str,
    ) -> Option<VerificationViolation> {
        let source = self.layouts.operand_type(operand)?;
        if self.holds_value(&source.kind) == self.holds_value(&target.kind) {
            return None;
        }
        Some(self.violation(
            dest,
            format!(
                "a {what} writes `{operand}`, a {} held as {}, into `{dest}` at {}, held as {}, \
                 so bytes and a word meet as one value",
                source.kind,
                self.describe(&source.kind),
                target.kind,
                self.describe(&target.kind)
            ),
        ))
    }

    /// The violations a call raises against its callee's declared parameters
    /// and return value.
    fn call_violations(
        &self,
        kind: &TerminatorKind,
        callees: &HashMap<&Symbol, &Body>,
    ) -> Vec<VerificationViolation> {
        let TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } = kind
        else {
            return Vec::new();
        };
        let Some((symbol, callee)) = func
            .called_symbol()
            .and_then(|symbol| Some((symbol, *callees.get(symbol)?)))
        else {
            return Vec::new();
        };
        let call = Call {
            name: symbol.link_name(),
            callee,
            destination,
        };
        let mut violations: Vec<VerificationViolation> = args
            .iter()
            .enumerate()
            .take(callee.arg_count)
            .filter_map(|(position, arg)| self.argument_violation(&call, position, arg))
            .collect();
        violations.extend(self.return_violation(&call));
        violations
    }

    /// The violation an argument raises against the parameter it binds.
    fn argument_violation(
        &self,
        call: &Call<'_>,
        position: usize,
        arg: &Operand,
    ) -> Option<VerificationViolation> {
        let source = self.layouts.operand_type(arg)?;
        let parameter = &call.callee.local_decls.get(position + 1)?.ty;
        let (source_how, parameter_how) = self.disagreement(&source.kind, call, &parameter.kind)?;
        Some(self.violation(
            call.destination,
            format!(
                "the call to {} passes argument {position}, `{arg}`, a {} held as {source_how}, \
                 to a parameter declared {}, held as {parameter_how}",
                call.name, source.kind, parameter.kind
            ),
        ))
    }

    /// The violation a call's destination raises against the callee's return.
    fn return_violation(&self, call: &Call<'_>) -> Option<VerificationViolation> {
        let returned = &call.callee.local_decls.first()?.ty;
        let target = self.layouts.place_type(call.destination)?;
        let (target_how, returned_how) = self.disagreement(&target.kind, call, &returned.kind)?;
        Some(self.violation(
            call.destination,
            format!(
                "{} returns a {} held as {returned_how}, into `{}` at {}, held as {target_how}",
                call.name, returned.kind, call.destination, target.kind
            ),
        ))
    }

    /// How `own` (a type of this body) and `callee_kind` (a type of the
    /// callee) are held, described, when one is bytes and the other is not.
    fn disagreement(
        &self,
        own: &TypeKind,
        call: &Call<'_>,
        callee_kind: &TypeKind,
    ) -> Option<(&'static str, &'static str)> {
        let definitions = self.layouts.definitions;
        if self.holds_value(own) == holds_value(call.callee, callee_kind) {
            return None;
        }
        Some((
            self.describe(own),
            describe(call.callee, definitions, callee_kind),
        ))
    }
}

/// One static call being checked against its callee.
struct Call<'c> {
    name: String,
    callee: &'c Body,
    destination: &'c Place,
}

/// Where an aggregate of `kind` built into `dest` holds an operand, in words.
fn held_in(kind: &AggregateKind, dest: &Place) -> String {
    match kind {
        AggregateKind::Closure(_, _) => format!("captured by the closure built into `{dest}`"),
        AggregateKind::Enum(_, _) => format!("held in the enum payload built into `{dest}`"),
        AggregateKind::Option(_) => format!("held in the optional payload built into `{dest}`"),
        AggregateKind::Tuple(_)
        | AggregateKind::Struct(_)
        | AggregateKind::Class(_)
        | AggregateKind::Array
        | AggregateKind::List
        | AggregateKind::Set
        | AggregateKind::Map
        | AggregateKind::FormattedString => {
            format!("held in a slot of the aggregate built into `{dest}`")
        }
    }
}
