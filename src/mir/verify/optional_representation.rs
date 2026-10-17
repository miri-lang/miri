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
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::mir::option_layout::OptionalLayers;
use crate::mir::place::PlaceElem;
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
struct Layouts<'a> {
    body: &'a Body,
    definitions: &'a HashMap<String, TypeDefinition>,
}

impl<'a> Layouts<'a> {
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
                        let slot_ty = self.aggregate_slot(kind, target, slot)?;
                        self.meeting_violation(dest, operand, slot_ty, "aggregate slot")
                    })
                    .collect()
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
            | Rvalue::AtomicOp { .. }
            | Rvalue::Phi(_) => Vec::new(),
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
        let (source_layout, target_layout) = self.disagreement(source, target)?;
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
        let layers = OptionalLayers::of(&holder_ty.kind, self.definitions);
        if layers.boxed > 0 || !layers.innermost_nullable_pointer {
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
        parameter: &Type,
    ) -> Option<VerificationViolation> {
        let source = self.operand_type(arg)?;
        let (source_layout, target_layout) = self.disagreement(source, parameter)?;
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
        let (source_layout, target_layout) = self.disagreement(returned, target)?;
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

    /// The layouts of `source` and `target`, described, when both are
    /// optionals laid out differently.
    ///
    /// An optional whose innermost value is `void` is the type an absent value
    /// is spelled at before it meets a slot; it holds nothing, so it agrees
    /// with every layout.
    fn disagreement(&self, source: &Type, target: &Type) -> Option<(String, String)> {
        let source = OptionalLayers::of(&source.kind, self.definitions);
        let target = OptionalLayers::of(&target.kind, self.definitions);
        let is_absent_placeholder =
            |layers: &OptionalLayers<'_>| matches!(layers.value, TypeKind::Void | TypeKind::Error);
        if !is_optional(&source)
            || !is_optional(&target)
            || is_absent_placeholder(&source)
            || is_absent_placeholder(&target)
            || (source.boxed, source.innermost_nullable_pointer)
                == (target.boxed, target.innermost_nullable_pointer)
        {
            return None;
        }
        Some((describe(&source), describe(&target)))
    }

    /// The type `operand` reads, unless it is the absent value, which is null
    /// under every layout.
    fn operand_type(&self, operand: &'a Operand) -> Option<&'a Type> {
        match operand {
            Operand::Constant(constant) => {
                (!matches!(constant.literal, Literal::None)).then_some(&constant.ty)
            }
            Operand::Function(function) => Some(&function.ty),
            Operand::Copy(place) | Operand::Move(place) => self.place_type(place),
        }
    }

    /// The type of the value at `place`, when its projections can be read from
    /// the body alone: a dereference, an enum payload, a closure capture and a
    /// field of a generic class are not.
    fn place_type(&self, place: &Place) -> Option<&'a Type> {
        let mut current = &self.body.local_decls.get(place.local.0)?.ty;
        for elem in &place.projection {
            current = match elem {
                PlaceElem::Deref => return None,
                PlaceElem::Field(index) => self.field_type(current, *index)?,
                PlaceElem::Index(_) => element_type(&current.kind)?,
            };
        }
        Some(current)
    }

    /// The type of field `index` of a `holder` value: an optional's payload,
    /// a tuple's element, or a field of a struct or class this body records
    /// at no type parameter.
    fn field_type(&self, holder: &'a Type, index: usize) -> Option<&'a Type> {
        if let TypeKind::Option(inner) = &holder.kind {
            return (index == 0).then_some(inner);
        }
        if let TypeKind::Tuple(elements) = &holder.kind {
            return written_type(elements.get(index)?);
        }
        let TypeKind::Custom(name, _) = &holder.kind else {
            return None;
        };
        let is_generic = self
            .body
            .class_type_params
            .get(name)
            .is_some_and(|params| !params.is_empty());
        if is_generic {
            return None;
        }
        self.body.field_types.get(name)?.get(index)
    }

    /// The type slot `slot` of an aggregate of `kind` holds, built into a
    /// value of `target` type.
    fn aggregate_slot(
        &self,
        kind: &AggregateKind,
        target: &'a Type,
        slot: usize,
    ) -> Option<&'a Type> {
        match kind {
            AggregateKind::Option
            | AggregateKind::Tuple
            | AggregateKind::Struct(_)
            | AggregateKind::Class(_) => self.field_type(target, slot),
            AggregateKind::Array | AggregateKind::List | AggregateKind::Set => {
                element_type(&target.kind)
            }
            AggregateKind::Map => map_slot_type(&target.kind, slot),
            AggregateKind::FormattedString
            | AggregateKind::Enum(_, _)
            | AggregateKind::Closure(_, _) => None,
        }
    }
}

/// Whether `layers` describes an optional at all.
fn is_optional(layers: &OptionalLayers<'_>) -> bool {
    layers.boxed > 0 || layers.innermost_nullable_pointer
}

/// A layout, in words.
fn describe(layers: &OptionalLayers<'_>) -> String {
    let boxes = match layers.boxed {
        1 => "one box".to_string(),
        count => format!("{count} boxes"),
    };
    match (layers.boxed, layers.innermost_nullable_pointer) {
        (0, _) => "the nullable payload pointer".to_string(),
        (_, true) => format!("{boxes} around a nullable payload pointer"),
        (_, false) => boxes,
    }
}

/// The type an element of a collection of `kind` has; for a map, its value.
fn element_type(kind: &TypeKind) -> Option<&Type> {
    let position = match collection_kind(kind)? {
        BuiltinCollectionKind::List | BuiltinCollectionKind::Array | BuiltinCollectionKind::Set => {
            0
        }
        BuiltinCollectionKind::Map => 1,
    };
    written_type(type_argument(kind, position)?)
}

/// The type slot `slot` of a map literal holds: keys at even slots, values at
/// odd ones.
fn map_slot_type(kind: &TypeKind, slot: usize) -> Option<&Type> {
    let BuiltinCollectionKind::Map = collection_kind(kind)? else {
        return None;
    };
    written_type(type_argument(kind, slot % 2)?)
}

/// Which built-in collection `kind` is, in either of its spellings.
fn collection_kind(kind: &TypeKind) -> Option<BuiltinCollectionKind> {
    if let TypeKind::Custom(name, Some(_)) = kind {
        return BuiltinCollectionKind::from_name(name);
    }
    if let TypeKind::List(_) = kind {
        return Some(BuiltinCollectionKind::List);
    }
    if let TypeKind::Array(_, _) = kind {
        return Some(BuiltinCollectionKind::Array);
    }
    if let TypeKind::Set(_) = kind {
        return Some(BuiltinCollectionKind::Set);
    }
    if let TypeKind::Map(_, _) = kind {
        return Some(BuiltinCollectionKind::Map);
    }
    None
}

/// The collection type argument at `position`, in declaration order.
fn type_argument(kind: &TypeKind, position: usize) -> Option<&Expression> {
    if let TypeKind::Custom(_, Some(args)) = kind {
        return args.get(position);
    }
    if let TypeKind::List(element) | TypeKind::Array(element, _) | TypeKind::Set(element) = kind {
        return (position == 0).then_some(element.as_ref());
    }
    if let TypeKind::Map(key, value) = kind {
        return [key.as_ref(), value.as_ref()].get(position).copied();
    }
    None
}

/// The type a type argument expression spells.
fn written_type(expression: &Expression) -> Option<&Type> {
    let ExpressionKind::Type(ty, _) = &expression.node else {
        return None;
    };
    Some(ty)
}
