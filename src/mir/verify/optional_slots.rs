// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that every value meeting an optional held as its bytes brings those
//! bytes.
//!
//! An optional of a scalar or of a value held as its bytes is a presence tag
//! and the payload beside it. Three spellings carry no such bytes: the absent
//! constant `None`, a value of the placeholder type the absent value is
//! spelled at before it meets a slot (`void?`, `void??`, ...), and a bare value
//! standing for the optional holding it. Each must be written into a local of
//! the slot's type before it reaches the slot, and this rule reports one that
//! is not:
//!
//! - an assignment of a bare value into such an optional, or of the absent
//!   value into a place whose type cannot be read;
//! - a member of an aggregate: the absent constant into any member held as
//!   its bytes of a holder that is not built in place — which has no bytes
//!   of its own to clear for it — a placeholder or a bare value; or the
//!   absent value into a member whose type cannot be read;
//! - a side of `==` or `!=` meeting such an optional as a placeholder or bare;
//! - an argument of any call: the absent value, a placeholder or a bare value
//!   at such a parameter, element or key, or the absent value at a parameter
//!   whose type cannot be read.
//!
//! Where the slot's type cannot be read the absent value is refused: a check
//! that could not see the slot cannot say the bytes are there. Slot types are
//! read from [`PlaceTypes`], the one slot-type authority; which spellings
//! need the bytes is decided here, apart from the pass that writes them.

use super::{local_display_name, VerificationViolation};
use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::mir::option_layout::{is_absent_placeholder, OptionalLayers};
use crate::mir::place_type::{ArgumentSlot, CalleeParameters, PlaceTypes};
use crate::mir::rvalue::{BinOp, Rvalue};
use crate::mir::statement::StatementKind;
use crate::mir::terminator::TerminatorKind;
use crate::mir::{Body, Local, Operand, Place};

/// Every value in `program` meeting an optional held as its bytes without
/// those bytes, paired with the body it is found in. `link_name` names a
/// body the way a call to it names its callee.
pub fn verify_optional_slots<N: Clone>(
    program: &[(N, Body)],
    link_name: impl Fn(&N) -> String,
) -> Vec<(N, VerificationViolation)> {
    let callees = CalleeParameters::of(program, link_name);
    program
        .iter()
        .filter(|(_, body)| !body.is_gpu())
        .flat_map(|(name, body)| {
            body_violations(body, &callees)
                .into_iter()
                .map(move |violation| (name.clone(), violation))
        })
        .collect()
}

/// The violations one body raises.
fn body_violations(body: &Body, callees: &CalleeParameters) -> Vec<VerificationViolation> {
    let check = Check {
        body,
        slots: PlaceTypes::of(body),
    };
    let mut found = Vec::new();
    for data in &body.basic_blocks {
        for statement in &data.statements {
            found.extend(check.statement(&statement.kind));
        }
        if let Some(terminator) = &data.terminator {
            found.extend(check.call(&terminator.kind, callees));
        }
    }
    found
}

/// How one spelling meets a slot.
enum Meeting {
    /// The slot holds an optional held as its bytes, which the spelling
    /// does not bring.
    Missing(&'static str),
    /// The slot's type cannot be read, and the spelling is the absent value.
    Unread,
}

struct Check<'b> {
    body: &'b Body,
    slots: PlaceTypes<'b>,
}

impl Check<'_> {
    /// The violations one statement raises.
    fn statement(&self, kind: &StatementKind) -> Vec<VerificationViolation> {
        let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) = kind
        else {
            return Vec::new();
        };
        match rvalue {
            Rvalue::Use(operand) => self.assignment(dest, operand),
            Rvalue::Cast(operand, _) => self.assignment(dest, operand),
            Rvalue::Aggregate(kind, operands) => {
                let in_place = self
                    .slots
                    .declared(dest)
                    .is_some_and(|ty| self.is_held(&ty));
                operands
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, operand)| {
                        let slot_ty = self.slots.aggregate_slot(kind, dest, slot);
                        let meeting = self.member(slot_ty.as_ref(), operand, in_place)?;
                        Some(self.violation(dest.local, operand, &meeting, "a member"))
                    })
                    .collect()
            }
            Rvalue::BinaryOp(BinOp::Eq | BinOp::Ne, lhs, rhs) => [(lhs, rhs), (rhs, lhs)]
                .into_iter()
                .filter_map(|(held, other)| {
                    let meeting = self.compared(held, other)?;
                    Some(self.violation(dest.local, other, &meeting, "a comparison"))
                })
                .collect(),
            Rvalue::BinaryOp(..)
            | Rvalue::Ref(_)
            | Rvalue::UnaryOp(..)
            | Rvalue::Len(_)
            | Rvalue::GpuIntrinsic(_)
            | Rvalue::MathIntrinsic(..)
            | Rvalue::AtomicOp { .. } => Vec::new(),
        }
    }

    /// An assignment: a bare value into an optional held as its bytes, or
    /// the absent value into a place whose type cannot be read.
    fn assignment(&self, dest: &Place, operand: &Operand) -> Vec<VerificationViolation> {
        if is_absent_constant(operand) {
            return self.unread_assignment(dest, operand);
        }
        let Some(slot) = self.slots.declared(dest) else {
            return self.unread_assignment(dest, operand);
        };
        if !self.is_held_optional(&slot) || !self.is_bare(&slot, operand) {
            return Vec::new();
        }
        vec![self.violation(
            dest.local,
            operand,
            &Meeting::Missing("a bare value"),
            "an assignment",
        )]
    }

    /// An assignment of the absent value, which brings its bytes wherever the
    /// destination's type can be read.
    fn unread_assignment(&self, dest: &Place, operand: &Operand) -> Vec<VerificationViolation> {
        if self.slots.declared(dest).is_some() || !self.spells_absence(operand) {
            return Vec::new();
        }
        vec![self.violation(dest.local, operand, &Meeting::Unread, "an assignment")]
    }

    /// How `other` meets the optional `held` reads in a comparison; `None`
    /// when it brings what the comparison needs. The absent constant is
    /// tested against a tag; a placeholder compared with a place whose type
    /// cannot be read has no layout to be read into.
    fn compared(&self, held: &Operand, other: &Operand) -> Option<Meeting> {
        let Some(slot) = self.slots.operand_type(held) else {
            let unread = matches!(held, Operand::Copy(_) | Operand::Move(_));
            return (unread && self.is_placeholder(other)).then_some(Meeting::Unread);
        };
        if !self.is_held_optional(&slot) || is_absent_constant(other) {
            return None;
        }
        self.declared(&slot, other)
    }

    /// How `operand` meets the aggregate member `slot`, built in place when
    /// `in_place`; `None` when it brings what the member needs.
    fn member(&self, slot: Option<&Type>, operand: &Operand, in_place: bool) -> Option<Meeting> {
        let Some(slot) = slot else {
            return self.spells_absence(operand).then_some(Meeting::Unread);
        };
        if is_absent_constant(operand) {
            if in_place || !self.is_held(slot) {
                return None;
            }
            return Some(Meeting::Missing("the absent constant"));
        }
        self.declared(slot, operand)
    }

    /// How `operand` meets a slot declared `slot`; `None` when it brings
    /// what the slot needs.
    fn declared(&self, slot: &Type, operand: &Operand) -> Option<Meeting> {
        if !self.is_held_optional(slot) {
            return None;
        }
        if is_absent_constant(operand) {
            return Some(Meeting::Missing("the absent constant"));
        }
        if self.is_placeholder(operand) {
            return Some(Meeting::Missing("the absent value's placeholder"));
        }
        self.is_bare(slot, operand)
            .then_some(Meeting::Missing("a bare value"))
    }

    /// The violations the call `kind` raises.
    fn call(
        &self,
        kind: &TerminatorKind,
        callees: &CalleeParameters,
    ) -> Vec<VerificationViolation> {
        let (args, destination) = match kind {
            TerminatorKind::Call {
                args, destination, ..
            }
            | TerminatorKind::VirtualCall {
                args, destination, ..
            } => (args, destination),
            TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => return Vec::new(),
        };
        self.slots
            .call_argument_slots(kind, callees)
            .iter()
            .zip(args)
            .filter_map(|(slot, operand)| {
                let meeting = match slot {
                    ArgumentSlot::Declared(slot) => self.declared(slot, operand)?,
                    ArgumentSlot::Unknown => {
                        self.spells_absence(operand).then_some(Meeting::Unread)?
                    }
                    ArgumentSlot::Word => return None,
                };
                Some(self.violation(destination.local, operand, &meeting, "an argument"))
            })
            .collect()
    }

    fn violation(
        &self,
        local: Local,
        operand: &Operand,
        meeting: &Meeting,
        consumer: &str,
    ) -> VerificationViolation {
        let message = match meeting {
            Meeting::Missing(spelling) => format!(
                "{spelling}, `{operand}`, reaches {consumer} declared an optional held as its \
                 bytes, without those bytes"
            ),
            Meeting::Unread => format!(
                "the absent value, `{operand}`, reaches {consumer} whose type cannot be read, so \
                 nothing says the bytes it needs are there"
            ),
        };
        VerificationViolation {
            local,
            local_name: local_display_name(self.body, local),
            message,
        }
    }

    /// Whether `ty` is held as its bytes in this body.
    fn is_held(&self, ty: &Type) -> bool {
        self.body.value_layout(&ty.kind).is_some()
    }

    /// Whether `ty` is an optional held as its bytes in this body. One that
    /// names a parameter the body leaves open has no settled layout, and is
    /// left to the instantiation that binds it.
    fn is_held_optional(&self, ty: &Type) -> bool {
        !self.slots.names_open_parameter(&ty.kind)
            && self
                .body
                .value_layout(&ty.kind)
                .is_some_and(|layout| layout.is_optional())
    }

    /// Whether `operand` spells the absent value: the constant, or a value of
    /// the placeholder type.
    fn spells_absence(&self, operand: &Operand) -> bool {
        is_absent_constant(operand) || self.is_placeholder(operand)
    }

    /// Whether `operand` reads a value of the absent value's placeholder type.
    fn is_placeholder(&self, operand: &Operand) -> bool {
        matches!(operand, Operand::Copy(_) | Operand::Move(_))
            && self
                .slots
                .operand_type(operand)
                .is_some_and(|ty| is_absent_placeholder(&ty.kind))
    }

    /// Whether `operand` is a bare value standing for the optional `slot`:
    /// it reads fewer optional layers than the slot holds.
    fn is_bare(&self, slot: &Type, operand: &Operand) -> bool {
        if matches!(operand, Operand::Function(_)) {
            return false;
        }
        let Some(ty) = self.slots.operand_type(operand) else {
            return false;
        };
        if is_absent_placeholder(&ty.kind) || matches!(ty.kind, TypeKind::Void | TypeKind::Error) {
            return false;
        }
        self.layers(&ty) < self.layers(slot)
    }

    /// How many optional layers wrap `ty`.
    fn layers(&self, ty: &Type) -> usize {
        let layers = OptionalLayers::of(&ty.kind, self.body.layout_scope());
        layers.boxed + layers.inline + usize::from(layers.innermost_nullable_pointer)
    }
}

/// Whether `operand` is the absent value written as a constant.
fn is_absent_constant(operand: &Operand) -> bool {
    matches!(operand, Operand::Constant(constant) if matches!(constant.literal, Literal::None))
}
