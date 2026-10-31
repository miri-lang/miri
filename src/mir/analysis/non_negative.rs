// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which integer locals of a body no definition can make negative.
//!
//! Decided once for the whole body: every definition of such a local is a
//! non-negative constant, a length, a copy of such a local, or a sum or
//! product of such operands. A sum or product of non-negative operands stays
//! non-negative only because integer `+` and `*` stop the program on
//! overflow ([`INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW`]); were they to wrap, a
//! sum could turn negative, and this rule would admit only constants,
//! lengths and copies. A parameter, a call's result, a capture, a local
//! written through a projection and one whose address is taken are never
//! known non-negative; an unsigned local always is.
//!
//! [`constant_step`] reads the steps by a constant at least 0 — `x + k`,
//! `k + x`, `x - k` — that the in-bounds and loop-versioning passes follow.

use std::collections::{HashMap, HashSet};

use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::mir::{BinOp, Body, Local, Operand, Rvalue, StatementKind, Terminator};

/// Whether integer `+`, `-` and `*` stop the program on overflow rather than
/// wrap. Code generation emits them overflow-checked at every optimization
/// level while this holds; the non-negativity rule above relies on it.
pub const INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW: bool = true;

/// The integer locals of `body` no definition can make negative, decided
/// optimistically: every candidate starts known, and one with a definition
/// that could be negative given the others is dropped, until none is.
pub fn non_negative_locals(body: &Body) -> HashSet<usize> {
    let definitions = whole_definitions(body);
    let excluded = excluded_locals(body);
    let mut known: HashSet<usize> = (body.local_decls.iter().enumerate())
        .filter(|(index, decl)| {
            is_unsigned(&decl.ty.kind)
                || (is_signed_integer(&decl.ty.kind)
                    && !excluded.contains(index)
                    && definitions.contains_key(index))
        })
        .map(|(index, _)| index)
        .collect();
    loop {
        let dropped: Vec<usize> = (known.iter().copied())
            .filter(|local| !is_unsigned_local(body, *local))
            .filter(|local| {
                (definitions.get(local).into_iter().flatten())
                    .any(|rvalue| !is_non_negative_rvalue(rvalue, body, &known))
            })
            .collect();
        if dropped.is_empty() {
            return known;
        }
        for local in dropped {
            known.remove(&local);
        }
    }
}

/// Every rvalue each whole local is assigned, by local index.
fn whole_definitions(body: &Body) -> HashMap<usize, Vec<&Rvalue>> {
    let mut definitions: HashMap<usize, Vec<&Rvalue>> = HashMap::new();
    for statement in body.basic_blocks.iter().flat_map(|data| &data.statements) {
        if let StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) =
            &statement.kind
        {
            if place.projection.is_empty() {
                definitions.entry(place.local.0).or_default().push(rvalue);
            }
        }
    }
    definitions
}

/// The locals whose value code this body does not show may set: parameters,
/// captures, call results, locals written through a projection and locals
/// whose address is taken.
fn excluded_locals(body: &Body) -> HashSet<usize> {
    let mut excluded: HashSet<usize> = (1..=body.arg_count).collect();
    excluded.extend(body.env_capture_locals.iter().map(|local| local.0));
    for data in &body.basic_blocks {
        for statement in &data.statements {
            match &statement.kind {
                StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                    if !place.projection.is_empty() {
                        excluded.insert(place.local.0);
                    }
                    if let Rvalue::Ref(taken) = rvalue {
                        excluded.insert(taken.local.0);
                    }
                }
                StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::IncRef(_)
                | StatementKind::DecRef(_)
                | StatementKind::Dealloc(_)
                | StatementKind::Nop => {}
            }
        }
        let written = data.terminator.iter().flat_map(Terminator::written_places);
        excluded.extend(written.map(|place| place.local.0));
    }
    excluded
}

/// Whether `rvalue` is non-negative when every local of `known` is.
fn is_non_negative_rvalue(rvalue: &Rvalue, body: &Body, known: &HashSet<usize>) -> bool {
    let operand_is = |operand: &Operand| is_non_negative_operand(operand, body, known);
    match rvalue {
        Rvalue::Use(operand) => operand_is(operand),
        Rvalue::Len(_) => true,
        Rvalue::BinaryOp(BinOp::Add | BinOp::Mul, lhs, rhs) => {
            INTEGER_ARITHMETIC_TRAPS_ON_OVERFLOW && operand_is(lhs) && operand_is(rhs)
        }
        Rvalue::BinaryOp(..)
        | Rvalue::Ref(_)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::Aggregate(..)
        | Rvalue::AtomicOp { .. } => false,
    }
}

fn is_non_negative_operand(operand: &Operand, body: &Body, known: &HashSet<usize>) -> bool {
    match operand {
        Operand::Constant(constant) => matches!(
            &constant.literal,
            Literal::Integer(value) if is_non_negative_literal(value)
        ),
        Operand::Copy(place) | Operand::Move(place) => {
            place.projection.is_empty()
                && (known.contains(&place.local.0) || is_unsigned_local(body, place.local.0))
        }
        Operand::Function(_) => false,
    }
}

fn is_non_negative_literal(value: &IntegerLiteral) -> bool {
    match value {
        IntegerLiteral::I8(v) => *v >= 0,
        IntegerLiteral::I16(v) => *v >= 0,
        IntegerLiteral::I32(v) => *v >= 0,
        IntegerLiteral::I64(v) => *v >= 0,
        IntegerLiteral::I128(v) => *v >= 0,
        IntegerLiteral::U8(_)
        | IntegerLiteral::U16(_)
        | IntegerLiteral::U32(_)
        | IntegerLiteral::U64(_)
        | IntegerLiteral::U128(_) => true,
    }
}

fn is_unsigned_local(body: &Body, local: usize) -> bool {
    body.local_decls
        .get(local)
        .is_some_and(|decl| is_unsigned(&decl.ty.kind))
}

fn is_unsigned(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
    )
}

/// Whether `kind` is an integer type, signed or unsigned.
pub fn is_integer(kind: &TypeKind) -> bool {
    is_unsigned(kind) || is_signed_integer(kind)
}

fn is_signed_integer(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
    )
}

/// A step of a local by a constant that is never negative.
#[derive(Debug, Clone, Copy)]
pub enum Step {
    /// Up by the constant: `source + value` or `value + source`.
    Up(u64),
    /// Down by the constant: `source - value`.
    Down(u64),
}

/// `(source, step)` when `op` over `lhs` and `rhs` is `source + value`,
/// `value + source` or `source - value` for a whole local `source` and a
/// constant `value >= 0`.
pub fn constant_step(op: BinOp, lhs: &Operand, rhs: &Operand) -> Option<(Local, Step)> {
    match op {
        BinOp::Add => match (lhs.bare_local(), rhs.bare_local()) {
            (Some(source), None) => Some((source, Step::Up(non_negative_constant(rhs)?))),
            (None, Some(source)) => Some((source, Step::Up(non_negative_constant(lhs)?))),
            (Some(_), Some(_)) | (None, None) => None,
        },
        BinOp::Sub => Some((lhs.bare_local()?, Step::Down(non_negative_constant(rhs)?))),
        BinOp::Mul
        | BinOp::Div
        | BinOp::Rem
        | BinOp::BitXor
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::Eq
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Ne
        | BinOp::Ge
        | BinOp::Gt
        | BinOp::Offset => None,
    }
}

/// The value of an integer constant `operand`, when it is not negative.
pub fn non_negative_constant(operand: &Operand) -> Option<u64> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    let Literal::Integer(value) = &constant.literal else {
        return None;
    };
    u64::try_from(value.as_i128()?).ok()
}
