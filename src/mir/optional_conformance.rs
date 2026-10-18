// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every operand that meets an optional held as its bytes is spelled with
//! those bytes.
//!
//! An optional of a scalar or of a value held as its bytes is a tag and the
//! payload beside it, passed by its address. Three spellings reach such a slot
//! without the bytes:
//!
//! - the absent value written as a constant, `None`;
//! - a value of the placeholder type the absent value is spelled at before it
//!   meets a slot (`void?`, `void??`, ...), which is a word;
//! - a bare value standing for the optional holding it, as `3` stands for
//!   `Some(3)` where an `int?` is declared.
//!
//! This pass finds each one at every consumer whose slot type MIR spells — the
//! destination of an assignment, a slot of an aggregate, a side of a
//! comparison, a parameter of every kind of call and an element a collection
//! call is handed — and writes it into a local of the slot's type first: the
//! absent value and the placeholder read into the slot's bytes, a bare value
//! wrapped in `Some`. The consumer then reads that local. Lowering already
//! wraps a bare value wherever the declared type is at hand; this pass is the
//! one place that settles every seam it does not reach.
//!
//! An assignment whose whole right-hand side is the absent value or a
//! placeholder is itself such a write, and is left alone: codegen clears the
//! bytes, or reads the placeholder word into them.
//!
//! Slot types come from the one slot-type authority,
//! [`crate::mir::place_type::PlaceTypes`]. The MIR verifier checks the rule
//! on its own, after this pass and again after reference counting, and
//! refuses an absent value wherever that authority cannot read the slot.

use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind};
use crate::mir::instantiation::type_argument;
use crate::mir::option_layout::{is_absent_placeholder, OptionalLayers};
use crate::mir::place_type::{ArgumentSlot, CalleeParameters, PlaceTypes};
use crate::mir::rvalue::{AggregateKind, BinOp, Rvalue};
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::symbol::Symbol;
use crate::mir::terminator::TerminatorKind;
use crate::mir::{Body, LocalDecl, Operand, Place};
use std::collections::HashMap;

/// Where an operand meets an optional slot held as its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seam {
    /// Operand `operand` of statement `statement` of block `block`.
    Statement {
        block: usize,
        statement: usize,
        operand: usize,
    },
    /// Argument `argument` of the call ending block `block`.
    Argument { block: usize, argument: usize },
}

/// An operand spelled without the bytes of the optional slot it meets, and
/// the slot's type.
#[derive(Debug, Clone)]
pub struct Unconformed {
    pub seam: Seam,
    pub operand: Operand,
    pub slot: Type,
}

/// Every operand of `body` spelled without the bytes of the optional slot it
/// meets, held as bytes.
pub fn unconformed_operands(body: &Body, callees: &CalleeParameters) -> Vec<Unconformed> {
    if body.is_gpu() {
        return Vec::new();
    }
    let slots = PlaceTypes::of(body);
    let mut found = Vec::new();
    for (block, data) in body.basic_blocks.iter().enumerate() {
        for (statement, stmt) in data.statements.iter().enumerate() {
            found.extend(statement_seams(
                body,
                &slots,
                (block, statement),
                &stmt.kind,
            ));
        }
        if let Some(terminator) = &data.terminator {
            found.extend(argument_seams(
                body,
                &slots,
                callees,
                block,
                &terminator.kind,
            ));
        }
    }
    found
}

/// The unconformed operands one statement holds.
fn statement_seams(
    body: &Body,
    slots: &PlaceTypes<'_>,
    (block, statement): (usize, usize),
    kind: &StatementKind,
) -> Vec<Unconformed> {
    let at = |operand: usize| Seam::Statement {
        block,
        statement,
        operand,
    };
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) = kind else {
        return Vec::new();
    };
    match rvalue {
        Rvalue::Use(operand) => assignment_seam(body, slots, dest, operand, at(0)),
        Rvalue::Cast(operand, _) => assignment_seam(body, slots, dest, operand, at(0)),
        Rvalue::Aggregate(kind, operands) => {
            // Built in place, a value held as its bytes clears a member the
            // absent constant fills; a counted aggregate has no bytes for it.
            let builds_in_place = slots
                .declared(dest)
                .is_some_and(|ty| body.value_layout(&ty.kind).is_some());
            operands
                .iter()
                .enumerate()
                .filter_map(|(slot, operand)| {
                    let slot_ty = slots.aggregate_slot(kind, dest, slot)?;
                    let absent = is_absent_constant(operand);
                    // A counted holder has no bytes to clear for the absent
                    // constant in any member held as bytes, optional or the
                    // placeholder a field takes before its initializer runs.
                    let needs = if absent {
                        !builds_in_place && body.value_layout(&slot_ty.kind).is_some()
                    } else {
                        needs_bytes(body, slots, operand, &slot_ty)
                    };
                    needs.then(|| unconformed(at(slot), operand, &slot_ty))
                })
                .collect()
        }
        Rvalue::BinaryOp(BinOp::Eq | BinOp::Ne, lhs, rhs) => {
            comparison_seams(body, slots, (lhs, rhs), at)
        }
        Rvalue::BinaryOp(..)
        | Rvalue::Ref(_)
        | Rvalue::UnaryOp(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => Vec::new(),
    }
}

/// The bare value an assignment writes into an optional held as bytes.
fn assignment_seam(
    body: &Body,
    slots: &PlaceTypes<'_>,
    dest: &Place,
    operand: &Operand,
    seam: Seam,
) -> Vec<Unconformed> {
    let Some(slot) = slots.declared(dest) else {
        return Vec::new();
    };
    bare_value(body, slots, operand, &slot)
        .then(|| unconformed(seam, operand, &slot))
        .into_iter()
        .collect()
}

/// The side of a comparison spelled without the bytes of the optional held
/// as bytes the other side reads.
fn comparison_seams(
    body: &Body,
    slots: &PlaceTypes<'_>,
    (lhs, rhs): (&Operand, &Operand),
    at: impl Fn(usize) -> Seam,
) -> Vec<Unconformed> {
    let sides = [(lhs, rhs, 1), (rhs, lhs, 0)];
    sides
        .into_iter()
        .filter_map(|(held, other, position)| {
            let slot = slots.operand_type(held)?;
            // The absent constant is read as a tag test where it is compared.
            (held_optional(body, &slot)
                && !is_absent_constant(other)
                && needs_bytes(body, slots, other, &slot))
            .then(|| unconformed(at(position), other, &slot))
        })
        .collect()
}

/// The unconformed arguments of the call ending `block`.
fn argument_seams(
    body: &Body,
    slots: &PlaceTypes<'_>,
    callees: &CalleeParameters,
    block: usize,
    kind: &TerminatorKind,
) -> Vec<Unconformed> {
    let args = match kind {
        TerminatorKind::Call { args, .. } | TerminatorKind::VirtualCall { args, .. } => args,
        TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => return Vec::new(),
    };
    slots
        .call_argument_slots(kind, callees)
        .into_iter()
        .zip(args)
        .enumerate()
        .filter_map(|(argument, (slot, operand))| {
            let ArgumentSlot::Declared(slot) = slot else {
                return None;
            };
            needs_bytes(body, slots, operand, &slot).then(|| Unconformed {
                seam: Seam::Argument { block, argument },
                operand: operand.clone(),
                slot,
            })
        })
        .collect()
}

fn unconformed(seam: Seam, operand: &Operand, slot: &Type) -> Unconformed {
    Unconformed {
        seam,
        operand: operand.clone(),
        slot: slot.clone(),
    }
}

/// Whether `operand` is the absent value written as a constant.
fn is_absent_constant(operand: &Operand) -> bool {
    matches!(operand, Operand::Constant(constant) if matches!(constant.literal, Literal::None))
}

/// Whether `slot` is an optional held as its bytes in `body`.
fn held_optional(body: &Body, slot: &Type) -> bool {
    body.value_layout(&slot.kind)
        .is_some_and(|layout| layout.is_optional())
}

/// Whether `operand` meets the optional slot `slot`, held as its bytes,
/// without those bytes: the absent constant, a placeholder, or a bare value.
fn needs_bytes(body: &Body, slots: &PlaceTypes<'_>, operand: &Operand, slot: &Type) -> bool {
    if !held_optional(body, slot) {
        return false;
    }
    if is_absent_constant(operand) {
        return true;
    }
    let is_placeholder = slots
        .operand_type(operand)
        .is_some_and(|ty| is_absent_placeholder(&ty.kind));
    is_placeholder || bare_value(body, slots, operand, slot)
}

/// Whether `operand` is a bare value standing for the optional `slot`, held
/// as its bytes, holding it: a value of no optional type, or of fewer
/// optional layers than the slot.
fn bare_value(body: &Body, slots: &PlaceTypes<'_>, operand: &Operand, slot: &Type) -> bool {
    if !held_optional(body, slot) || matches!(operand, Operand::Function(_)) {
        return false;
    }
    let Some(ty) = slots.operand_type(operand) else {
        return false;
    };
    if is_absent_placeholder(&ty.kind) || matches!(ty.kind, TypeKind::Void | TypeKind::Error) {
        return false;
    }
    optional_depth(body, &ty.kind) < optional_depth(body, &slot.kind)
}

/// How many optional layers wrap `kind`.
fn optional_depth(body: &Body, kind: &TypeKind) -> usize {
    let layers = OptionalLayers::of(kind, body.layout_scope());
    layers.boxed + layers.inline + usize::from(layers.innermost_nullable_pointer)
}

/// Write every operand of every body in `bodies` that meets an optional slot
/// held as its bytes without those bytes into a local of the slot's type.
pub fn conform_optional_operands(bodies: &mut [(Symbol, Body)]) {
    let found: Vec<Vec<Unconformed>> = {
        let callees = CalleeParameters::of(bodies, Symbol::link_name);
        bodies
            .iter()
            .map(|(_, body)| unconformed_operands(body, &callees))
            .collect()
    };
    for ((_, body), found) in bodies.iter_mut().zip(found) {
        conform_body(body, found);
    }
}

/// Conform every seam `found` names in `body`.
///
/// Seams are settled last first, so the writes inserted ahead of one never
/// move a statement a later one names. Two seams in one statement both write
/// ahead of it: the consumer then sits past every write made for it so far.
fn conform_body(body: &mut Body, found: Vec<Unconformed>) {
    let mut written_ahead: HashMap<(usize, usize), usize> = HashMap::new();
    for item in found.into_iter().rev() {
        if retype_placeholder_temp(body, &item) {
            continue;
        }
        let span = body.span;
        let (writes, local) = conformed_local(body, &item.operand, &item.slot, span);
        let count = writes.len();
        let (block, index, seam) = match item.seam {
            Seam::Statement {
                block,
                statement,
                operand,
            } => {
                let ahead = written_ahead.entry((block, statement)).or_default();
                let consumer = Seam::Statement {
                    block,
                    statement: statement + *ahead + count,
                    operand,
                };
                *ahead += count;
                (block, statement, consumer)
            }
            Seam::Argument { block, argument } => {
                let end = body.basic_blocks[block].statements.len();
                (block, end, Seam::Argument { block, argument })
            }
        };
        let released = released_placeholder(body, &item);
        let statements = &mut body.basic_blocks[block].statements;
        for (offset, write) in writes.into_iter().chain(released).enumerate() {
            statements.insert(index + offset, write);
        }
        replace_operand(body, &seam, Operand::Copy(local));
    }
}

/// Build the placeholder temp `item` reads straight at its slot's type, when
/// that temp exists only to meet it: unnamed, read nowhere else, and written
/// only as the absent value or as `Some` of it. `Some(None)` handed to an
/// `Option<int?>` then builds its bytes in place rather than a box to be read
/// into them. Answers whether it did.
fn retype_placeholder_temp(body: &mut Body, item: &Unconformed) -> bool {
    let Some(local) = item.operand.bare_local() else {
        return false;
    };
    let Some(decl) = body.local_decls.get(local.0) else {
        return false;
    };
    if decl.name.is_some() || !is_absent_placeholder(&decl.ty.kind) || local.0 <= body.arg_count {
        return false;
    }
    let mut uses = PlaceUses { local, reads: 0 };
    crate::mir::visitor::Visitor::visit_body(&mut uses, body);
    if uses.reads != 1 || !absent_writes_only(body, local) {
        return false;
    }
    body.local_decls[local.0].ty = item.slot.clone();
    for stmt in body
        .basic_blocks
        .iter_mut()
        .flat_map(|block| block.statements.iter_mut())
    {
        if let StatementKind::Assign(dest, Rvalue::Aggregate(AggregateKind::Option(built), _)) =
            &mut stmt.kind
        {
            if dest.local == local && dest.projection.is_empty() {
                *built = item.slot.clone();
            }
        }
    }
    true
}

/// Whether every write of `local` in `body` is the whole local assigned the
/// absent constant, or `Some` of it.
fn absent_writes_only(body: &Body, local: crate::mir::Local) -> bool {
    let is_absent = is_absent_constant;
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .all(|stmt| match &stmt.kind {
            StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)
                if dest.local == local =>
            {
                dest.projection.is_empty()
                    && match rvalue {
                        Rvalue::Use(operand) => is_absent(operand),
                        Rvalue::Aggregate(AggregateKind::Option(_), operands) => {
                            operands.iter().all(is_absent)
                        }
                        Rvalue::Aggregate(..)
                        | Rvalue::Cast(..)
                        | Rvalue::Ref(_)
                        | Rvalue::BinaryOp(..)
                        | Rvalue::UnaryOp(..)
                        | Rvalue::Len(_)
                        | Rvalue::GpuIntrinsic(_)
                        | Rvalue::MathIntrinsic(..)
                        | Rvalue::AtomicOp { .. }
                        | Rvalue::Phi(_) => false,
                    }
            }
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => true,
        })
}

/// How many times a body reads one local.
struct PlaceUses {
    local: crate::mir::Local,
    reads: usize,
}

impl crate::mir::visitor::Visitor for PlaceUses {
    fn visit_local(
        &mut self,
        local: crate::mir::Local,
        context: crate::mir::place::PlaceContext,
        _location: crate::mir::BasicBlock,
    ) {
        if local != self.local {
            return;
        }
        match context {
            crate::mir::place::PlaceContext::NonMutatingUse => self.reads += 1,
            crate::mir::place::PlaceContext::MutatingUse
            | crate::mir::place::PlaceContext::StorageLive
            | crate::mir::place::PlaceContext::StorageDead => {}
        }
    }
}

/// The release of the placeholder local `item` reads, when the call it was
/// handed to took its reference: a runtime entry point keeping what it is
/// given. Read into an optional held as bytes instead, the call takes no
/// reference, so the placeholder's own is released where it is read.
fn released_placeholder(body: &Body, item: &Unconformed) -> Option<Statement> {
    let Seam::Argument { block, argument } = item.seam else {
        return None;
    };
    let local = item.operand.bare_local()?;
    if !is_absent_placeholder(&body.local_decls.get(local.0)?.ty.kind) {
        return None;
    }
    let terminator = body.basic_blocks.get(block)?.terminator.as_ref()?;
    let TerminatorKind::Call { func, .. } = &terminator.kind else {
        return None;
    };
    let runtime = func.called_runtime_name()?;
    crate::runtime_fns::taken_argument_positions(runtime)
        .contains(&argument)
        .then(|| Statement {
            kind: StatementKind::StorageDead(Place::new(local)),
            span: terminator.span,
        })
}

/// The statements that write `operand` into a new local of type `slot`, and
/// that local: read into its bytes when absent, wrapped in `Some` — one layer
/// at a time — when bare.
fn conformed_local(
    body: &mut Body,
    operand: &Operand,
    slot: &Type,
    span: crate::error::syntax::Span,
) -> (Vec<Statement>, Place) {
    let source = match operand {
        Operand::Move(place) => Operand::Copy(place.clone()),
        Operand::Copy(_) | Operand::Constant(_) | Operand::Function(_) => operand.clone(),
    };
    let missing_layers = {
        let slots = PlaceTypes::of(body);
        match slots.operand_type(&source) {
            Some(ty) if bare_value(body, &slots, &source, slot) => {
                optional_depth(body, &slot.kind) - optional_depth(body, &ty.kind)
            }
            Some(_) | None => 0,
        }
    };
    let mut writes = Vec::new();
    let value = match missing_layers {
        0 => Rvalue::Use(source),
        layers => wrapped(body, source, (slot, layers), span, &mut writes),
    };
    let local = Place::new(body.new_local(LocalDecl::new(slot.clone(), span)));
    writes.push(Statement {
        kind: StatementKind::Assign(local.clone(), value),
        span,
    });
    (writes, local)
}

/// The `Some` building the optional `slot` around `operand`, which is
/// `layers` optional layers short of it, the layers between them written
/// into locals of their own by `writes`.
fn wrapped(
    body: &mut Body,
    operand: Operand,
    (slot, layers): (&Type, usize),
    span: crate::error::syntax::Span,
    writes: &mut Vec<Statement>,
) -> Rvalue {
    let payload = match payload_type(slot) {
        Some(inner) if layers > 1 => {
            let value = wrapped(body, operand, (&inner, layers - 1), span, writes);
            let local = Place::new(body.new_local(LocalDecl::new(inner, span)));
            writes.push(Statement {
                kind: StatementKind::Assign(local.clone(), value),
                span,
            });
            Operand::Copy(local)
        }
        Some(_) | None => operand,
    };
    Rvalue::Aggregate(AggregateKind::Option(slot.clone()), vec![payload])
}

/// The payload type of the optional `ty`, written either way.
fn payload_type(ty: &Type) -> Option<Type> {
    if let TypeKind::Option(inner) = &ty.kind {
        return Some((**inner).clone());
    }
    let TypeKind::Custom(name, Some(args)) = &ty.kind else {
        return None;
    };
    let [payload] = args.as_slice() else {
        return None;
    };
    (name == crate::ast::types::OPTION_TYPE_NAME)
        .then(|| type_argument(payload))
        .flatten()
}

/// Hand the consumer at `seam` `replacement` in place of its operand.
fn replace_operand(body: &mut Body, seam: &Seam, replacement: Operand) {
    match *seam {
        Seam::Statement {
            block,
            statement,
            operand,
        } => {
            if let Some(stmt) = body.basic_blocks[block].statements.get_mut(statement) {
                replace_statement_operand(&mut stmt.kind, operand, replacement);
            }
        }
        Seam::Argument { block, argument } => {
            let Some(terminator) = body.basic_blocks[block].terminator.as_mut() else {
                return;
            };
            let args = match &mut terminator.kind {
                TerminatorKind::Call { args, .. } | TerminatorKind::VirtualCall { args, .. } => {
                    args
                }
                TerminatorKind::GpuLaunch { .. }
                | TerminatorKind::Goto { .. }
                | TerminatorKind::SwitchInt { .. }
                | TerminatorKind::Return
                | TerminatorKind::Unreachable => return,
            };
            if let Some(arg) = args.get_mut(argument) {
                *arg = replacement;
            }
        }
    }
}

/// Replace operand `operand` of the assignment `kind` with `replacement`.
fn replace_statement_operand(kind: &mut StatementKind, operand: usize, replacement: Operand) {
    let (StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue)) = kind else {
        return;
    };
    match rvalue {
        Rvalue::Use(op) => *op = replacement,
        Rvalue::Cast(op, _) => **op = replacement,
        Rvalue::Aggregate(_, operands) => {
            if let Some(op) = operands.get_mut(operand) {
                *op = replacement;
            }
        }
        Rvalue::BinaryOp(_, lhs, rhs) => {
            let side = if operand == 0 { lhs } else { rhs };
            **side = replacement;
        }
        Rvalue::Ref(_)
        | Rvalue::UnaryOp(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => {}
    }
}
