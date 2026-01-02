// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::types::Type;
use crate::mir::operand::Operand;
use crate::mir::place::Place;
use std::fmt;

/// Right-hand value: the result of a computation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Rvalue {
    /// Use the operand as is (copy or move).
    Use(Operand),
    /// Create a reference to a place.
    Ref(Place),
    /// Binary operation.
    BinaryOp(BinOp, Box<Operand>, Box<Operand>),
    /// Unary operation.
    UnaryOp(UnOp, Box<Operand>),
    /// Cast operand to a type.
    Cast(Box<Operand>, Type),
    /// Get the length of an array/slice.
    Len(Place),
    // Aggregate constructions (tuples, arrays, structs) could go here.
}

impl fmt::Display for Rvalue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rvalue::Use(op) => write!(f, "{}", op),
            Rvalue::Ref(place) => write!(f, "&{}", place),
            Rvalue::BinaryOp(op, lhs, rhs) => write!(f, "{:?}({}, {})", op, lhs, rhs),
            Rvalue::UnaryOp(op, val) => write!(f, "{:?}({})", op, val),
            Rvalue::Cast(op, ty) => write!(f, "{} as {}", op, ty),
            Rvalue::Len(place) => write!(f, "Len({})", place),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    Offset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Not,
    Neg,
}
