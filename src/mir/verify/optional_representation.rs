// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that two optionals meet only where they share a layout.
//!
//! [`OptionalLayers`] decides how an optional is laid out: an optional whose
//! innermost payload is a reference-counted pointer can be that pointer, null
//! when absent, and every other layer is a box. Two optionals spelled at
//! different types are only one value when that decision lays them out alike,
//! so wherever one flows into the other — a use, a cast, a slot of an
//! aggregate, an argument and its parameter, a call result and its destination
//! — the two layouts must agree. An optional of a type parameter no
//! substitution reached is a box, and so cannot stand where an optional of a
//! class is expected.
//!
//! An optional laid out as the bare pointer has no payload slot of its own:
//! its field 0 is the very word that holds the optional. Writing that field,
//! taking its address, or deallocating it therefore names a slot the layout
//! does not have, and each is reported. Reading it is the pointer itself.

use super::{local_display_name, VerificationViolation};
use crate::ast::types::{Type, TypeKind};
use crate::mir::option_layout::{is_nullable_pointer_optional, LayoutScope, OptionalLayers};
use crate::mir::place::PlaceElem;
use crate::mir::place_type::{names_open_parameter, PlaceTypes};
use crate::mir::rvalue::{AggregateKind, Rvalue};
use crate::mir::statement::StatementKind;
use crate::mir::symbol::Symbol;
use crate::mir::terminator::TerminatorKind;
use crate::mir::{Body, Operand, Place};
use crate::type_checker::context::TypeDefinition;
use std::collections::HashMap;

/// Every place in `body` where two optionals of different layouts meet, and
/// every write, reference or deallocation of the payload slot an optional laid
/// out as a bare pointer does not have.
///
/// A GPU body is skipped: its optionals are laid out by the shader backends,
/// which hold no boxes.
pub fn verify_optional_representation(
    body: &Body,
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<VerificationViolation> {
    if body.is_gpu() {
        return Vec::new();
    }
    let layouts = Layouts { body, definitions };
    let mut violations = Vec::new();
    for block in &body.basic_blocks {
        for statement in &block.statements {
            violations.extend(layouts.statement_violations(&statement.kind));
        }
        if let Some(destination) = block.terminator.as_ref().and_then(|t| written_by(&t.kind)) {
            violations.extend(layouts.payload_slot_violation(destination, "a call writing"));
        }
    }
    violations
}

/// Every static call whose argument or destination is an optional laid out
/// differently from the callee's parameter or return value, paired with the
/// function making it.
///
/// Neither body shows such a mismatch on its own, so it is checked across the
/// program. A call with no static callee, or to a function with no body here —
/// a runtime intrinsic — is skipped, as is any call a GPU body makes.
pub fn verify_optional_call_layouts(
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
            let layouts = Layouts { body, definitions };
            body.basic_blocks
                .iter()
                .filter_map(|block| block.terminator.as_ref())
                .flat_map(|terminator| layouts.call_violations(&terminator.kind, &callees))
                .map(move |violation| (symbol.clone(), violation))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The place a terminator writes its result to, for a call.
fn written_by(kind: &TerminatorKind) -> Option<&Place> {
    match kind {
        TerminatorKind::Call { destination, .. }
        | TerminatorKind::VirtualCall { destination, .. } => Some(destination),
        TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => None,
    }
}

/// Reads the types and optional layouts of one body's places.
pub(super) struct Layouts<'a> {
    pub(crate) body: &'a Body,
    pub(crate) definitions: &'a HashMap<String, TypeDefinition>,
}

impl<'a> Layouts<'a> {
    /// What this body's optionals are laid out against: the type table and the
    /// parameters the body leaves open.
    fn scope(&self) -> LayoutScope<'a> {
        LayoutScope::new(self.definitions, &self.body.type_params)
    }

    /// The violations one statement raises.
    fn statement_violations(&self, kind: &StatementKind) -> Vec<VerificationViolation> {
        match kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                let mut violations = self.rvalue_violations(place, rvalue);
                violations.extend(self.payload_slot_violation(place, "a store into"));
                violations
            }
            StatementKind::Dealloc(place) => self
                .payload_slot_violation(place, "a deallocation of")
                .into_iter()
                .collect(),
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Nop => Vec::new(),
        }
    }

    /// The violations `rvalue` raises by being written to `dest`.
    fn rvalue_violations(&self, dest: &Place, rvalue: &Rvalue) -> Vec<VerificationViolation> {
        let target = self.place_type(dest);
        match rvalue {
            Rvalue::Use(operand) => target
                .as_ref()
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
                let mut violations: Vec<VerificationViolation> = self
                    .built_type_violation(dest, kind, &target)
                    .into_iter()
                    .collect();
                violations.extend(operands.iter().enumerate().filter_map(|(slot, operand)| {
                    let slot_ty = self.aggregate_slot(kind, dest, slot)?;
                    self.meeting_violation(dest, operand, &slot_ty, "aggregate slot")
                }));
                violations
            }
            Rvalue::Ref(place) => self
                .payload_slot_violation(place, "a reference to")
                .into_iter()
                .collect(),
            Rvalue::BinaryOp(..)
            | Rvalue::UnaryOp(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. } => Vec::new(),
        }
    }

    /// The violation `operand` raises by flowing, as a `what`, into a slot of
    /// `target` type in `dest`.
    fn meeting_violation(
        &self,
        dest: &Place,
        operand: &Operand,
        target: &Type,
        what: &str,
    ) -> Option<VerificationViolation> {
        let source = self.operand_type(operand)?;
        let (source_layout, target_layout) = self.disagreement(&source, target)?;
        Some(VerificationViolation {
            local: dest.local,
            local_name: local_display_name(self.body, dest.local),
            message: format!(
                "a {what} writes `{operand}`, a {} laid out as {source_layout}, into `{dest}` at \
                 {}, laid out as {target_layout}, so the two optionals are not one value",
                source.kind, target.kind
            ),
        })
    }

    /// The violation a `Some(..)` aggregate raises when the optional it says it
    /// builds is laid out apart from the `target` type of `dest`, the place it
    /// is written to.
    fn built_type_violation(
        &self,
        dest: &Place,
        kind: &AggregateKind,
        target: &Type,
    ) -> Option<VerificationViolation> {
        let AggregateKind::Option(built) = kind else {
            return None;
        };
        let (built_layout, target_layout) = self.disagreement(built, target)?;
        Some(VerificationViolation {
            local: dest.local,
            local_name: local_display_name(self.body, dest.local),
            message: format!(
                "a `Some` built as a {}, laid out as {built_layout}, is written into `{dest}` at \
                 {}, laid out as {target_layout}, so the two optionals are not one value",
                built.kind, target.kind
            ),
        })
    }

    /// The violation of writing, referencing or deallocating `place` when it is
    /// field 0 of an optional laid out as the bare payload pointer.
    fn payload_slot_violation(&self, place: &Place, what: &str) -> Option<VerificationViolation> {
        let (PlaceElem::Field(0), prefix) = place.projection.split_last()? else {
            return None;
        };
        let holder = Place {
            local: place.local,
            projection: prefix.to_vec(),
        };
        let holder_ty = self.place_type(&holder)?;
        if !is_nullable_pointer_optional(&holder_ty.kind, self.scope()) {
            return None;
        }
        Some(VerificationViolation {
            local: place.local,
            local_name: local_display_name(self.body, place.local),
            message: format!(
                "{what} `{place}` names field 0 of a {}, which is laid out as the nullable \
                 payload pointer itself and has no payload slot apart from the optional",
                holder_ty.kind
            ),
        })
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
        let name = symbol.link_name();
        let mut violations: Vec<VerificationViolation> = args
            .iter()
            .enumerate()
            .take(callee.arg_count)
            .filter_map(|(position, arg)| {
                let parameter = &callee.local_decls.get(position + 1)?.ty;
                let parameter = (parameter, self.scope_of(callee));
                self.argument_violation(destination, &name, position, arg, parameter)
            })
            .collect();
        violations.extend(self.return_violation(destination, &name, callee));
        violations
    }

    /// The violation an argument raises against the parameter it binds.
    fn argument_violation(
        &self,
        destination: &Place,
        callee: &str,
        position: usize,
        arg: &Operand,
        (parameter, callee_scope): (&Type, LayoutScope<'_>),
    ) -> Option<VerificationViolation> {
        let source = self.operand_type(arg)?;
        if names_open_parameter(&source.kind, &self.body.type_params)
            || names_open_parameter(&parameter.kind, callee_scope.type_params)
        {
            return None;
        }
        let (source_layout, target_layout) = layouts_disagree(
            OptionalLayers::of(&source.kind, self.scope()),
            OptionalLayers::of(&parameter.kind, callee_scope),
        )?;
        Some(VerificationViolation {
            local: destination.local,
            local_name: local_display_name(self.body, destination.local),
            message: format!(
                "the call to {callee} passes argument {position}, `{arg}`, a {} laid out as \
                 {source_layout}, to a parameter declared {}, laid out as {target_layout}",
                source.kind, parameter.kind
            ),
        })
    }

    /// The violation a call's destination raises against the callee's return.
    fn return_violation(
        &self,
        destination: &Place,
        callee: &str,
        callee_body: &Body,
    ) -> Option<VerificationViolation> {
        let returned = &callee_body.local_decls.first()?.ty;
        let target = self.place_type(destination)?;
        if names_open_parameter(&returned.kind, &callee_body.type_params)
            || names_open_parameter(&target.kind, &self.body.type_params)
        {
            return None;
        }
        let (source_layout, target_layout) = layouts_disagree(
            OptionalLayers::of(&returned.kind, self.scope_of(callee_body)),
            OptionalLayers::of(&target.kind, self.scope()),
        )?;
        Some(VerificationViolation {
            local: destination.local,
            local_name: local_display_name(self.body, destination.local),
            message: format!(
                "{callee} returns a {} laid out as {source_layout}, into `{destination}` at {}, \
                 laid out as {target_layout}",
                returned.kind, target.kind
            ),
        })
    }

    /// [`layouts_disagree`] for two types of this body.
    fn disagreement(&self, source: &Type, target: &Type) -> Option<(String, String)> {
        let open = PlaceTypes::of(self.body);
        if open.names_open_parameter(&source.kind) || open.names_open_parameter(&target.kind) {
            return None;
        }
        layouts_disagree(
            OptionalLayers::of(&source.kind, self.scope()),
            OptionalLayers::of(&target.kind, self.scope()),
        )
    }

    /// What `callee`'s own types are laid out against.
    fn scope_of<'b>(&self, callee: &'b Body) -> LayoutScope<'b>
    where
        'a: 'b,
    {
        LayoutScope::new(self.definitions, &callee.type_params)
    }

    /// The type `operand` reads, unless it is the absent value, whose type
    /// is the slot it meets; see [`PlaceTypes::operand_type`].
    pub(crate) fn operand_type(&self, operand: &Operand) -> Option<Type> {
        PlaceTypes::of(self.body).operand_type(operand)
    }

    /// The type of the value at `place`; see [`PlaceTypes::declared`].
    pub(crate) fn place_type(&self, place: &Place) -> Option<Type> {
        PlaceTypes::of(self.body).declared(place)
    }

    /// The type member `slot` of an aggregate of `kind` built into `dest`
    /// declares; see [`PlaceTypes::aggregate_slot`].
    pub(crate) fn aggregate_slot(
        &self,
        kind: &AggregateKind,
        dest: &Place,
        slot: usize,
    ) -> Option<Type> {
        PlaceTypes::of(self.body).aggregate_slot(kind, dest, slot)
    }
}

/// Whether `layers` describes an optional at all.
fn is_optional(layers: &OptionalLayers<'_>) -> bool {
    layers.is_optional()
}

/// A layout, in words, outermost layer first.
fn describe(layers: &OptionalLayers<'_>) -> String {
    let parts: Vec<String> = [
        count_of(layers.boxed, "one box", "boxes"),
        count_of(layers.inline, "one inline layer", "inline layers"),
        layers
            .innermost_nullable_pointer
            .then(|| "the nullable payload pointer".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();
    parts.join(" around ")
}

/// `count` of a kind of layer in words, or `None` when there are none.
fn count_of(count: usize, one: &str, many: &str) -> Option<String> {
    match count {
        0 => None,
        1 => Some(one.to_string()),
        count => Some(format!("{count} {many}")),
    }
}

/// The layouts of `source` and `target`, described, when both are optionals
/// laid out differently.
///
/// An optional whose innermost value is `void` is the type an absent value is
/// spelled at before it meets a slot; it holds nothing, so it agrees with every
/// layout.
fn layouts_disagree(
    source: OptionalLayers<'_>,
    target: OptionalLayers<'_>,
) -> Option<(String, String)> {
    let is_absent_placeholder =
        |layers: &OptionalLayers<'_>| matches!(*layers.value, TypeKind::Void | TypeKind::Error);
    if !is_optional(&source)
        || !is_optional(&target)
        || is_absent_placeholder(&source)
        || is_absent_placeholder(&target)
        || source.reprs() == target.reprs()
    {
        return None;
    }
    Some((describe(&source), describe(&target)))
}
