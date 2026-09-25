// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::types::TypeKind;
use crate::lexer::RegexToken;

use std::fmt;

/// Represents a literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    String(String),
    Boolean(bool),
    Identifier(String),
    Regex(RegexToken),
    None,
}

impl Literal {
    pub fn is_zero(&self) -> bool {
        match self {
            Literal::Integer(i) => i.is_zero(),
            Literal::Float(f) => f.is_zero(),
            Literal::String(_)
            | Literal::Boolean(_)
            | Literal::Identifier(_)
            | Literal::Regex(_)
            | Literal::None => false,
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Integer(i) => write!(f, "{}", i),
            Literal::Float(fl) => write!(f, "{}", fl),
            Literal::String(s) => write!(f, "\"{}\"", s),
            Literal::Boolean(b) => write!(f, "{}", b),
            Literal::Identifier(id) => write!(f, "{}", id),
            Literal::Regex(_) => write!(f, "<regex>"),
            Literal::None => write!(f, "none"),
        }
    }
}

/// Represents an integer literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntegerLiteral {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
}

impl IntegerLiteral {
    /// The literal variant matching `kind`, carrying `value`; `None` for a
    /// non-integer kind.
    pub fn from_type_kind(kind: &TypeKind, value: i128) -> Option<Self> {
        match kind {
            TypeKind::I8 => Some(IntegerLiteral::I8(value as i8)),
            TypeKind::I16 => Some(IntegerLiteral::I16(value as i16)),
            TypeKind::I32 => Some(IntegerLiteral::I32(value as i32)),
            TypeKind::Int | TypeKind::I64 => Some(IntegerLiteral::I64(value as i64)),
            TypeKind::I128 => Some(IntegerLiteral::I128(value)),
            TypeKind::U8 => Some(IntegerLiteral::U8(value as u8)),
            TypeKind::U16 => Some(IntegerLiteral::U16(value as u16)),
            TypeKind::U32 => Some(IntegerLiteral::U32(value as u32)),
            TypeKind::U64 => Some(IntegerLiteral::U64(value as u64)),
            TypeKind::U128 => Some(IntegerLiteral::U128(value as u128)),
            _ => None,
        }
    }

    /// Returns `true` if the literal value is zero.
    pub fn is_zero(&self) -> bool {
        match self {
            IntegerLiteral::I8(v) => *v == 0,
            IntegerLiteral::I16(v) => *v == 0,
            IntegerLiteral::I32(v) => *v == 0,
            IntegerLiteral::I64(v) => *v == 0,
            IntegerLiteral::I128(v) => *v == 0,
            IntegerLiteral::U8(v) => *v == 0,
            IntegerLiteral::U16(v) => *v == 0,
            IntegerLiteral::U32(v) => *v == 0,
            IntegerLiteral::U64(v) => *v == 0,
            IntegerLiteral::U128(v) => *v == 0,
        }
    }

    /// The value of a literal written above `i128::MAX`, which only a `u128`
    /// holds; `None` for every literal an `i128` holds.
    ///
    /// [`Self::to_i128`] gives such a literal's bit pattern, which reads as a
    /// negative number — right for storing it, wrong for judging its range.
    pub fn above_i128(&self) -> Option<u128> {
        match self {
            IntegerLiteral::U128(v) if *v > i128::MAX as u128 => Some(*v),
            IntegerLiteral::I8(_)
            | IntegerLiteral::I16(_)
            | IntegerLiteral::I32(_)
            | IntegerLiteral::I64(_)
            | IntegerLiteral::I128(_)
            | IntegerLiteral::U8(_)
            | IntegerLiteral::U16(_)
            | IntegerLiteral::U32(_)
            | IntegerLiteral::U64(_)
            | IntegerLiteral::U128(_) => None,
        }
    }

    /// The literal's value when an `i128` holds it, `None` for one written above
    /// `i128::MAX`.
    ///
    /// Arithmetic on a literal's value uses this: [`Self::to_i128`] is a bit
    /// pattern for the widest literals, and computing with it is computing with
    /// a different number.
    pub fn as_i128(&self) -> Option<i128> {
        match self.above_i128() {
            Some(_) => None,
            None => Some(self.to_i128()),
        }
    }

    /// The value of `-literal`, or `None` when no `i128` holds it.
    ///
    /// The magnitude of `i128::MIN` is one past `i128::MAX`, so it is written
    /// as a literal only a `u128` carries; its negation is the one value above
    /// `i128::MAX` that still lands in range.
    pub fn negated(&self) -> Option<i128> {
        match self.above_i128() {
            Some(magnitude) if magnitude == i128::MIN.unsigned_abs() => Some(i128::MIN),
            Some(_) => None,
            None => self.to_i128().checked_neg(),
        }
    }

    /// Converts the integer literal to an `i128` value.
    ///
    /// Used by the type checker for compile-time constant evaluation.
    pub fn to_i128(&self) -> i128 {
        match self {
            IntegerLiteral::I8(v) => *v as i128,
            IntegerLiteral::I16(v) => *v as i128,
            IntegerLiteral::I32(v) => *v as i128,
            IntegerLiteral::I64(v) => *v as i128,
            IntegerLiteral::I128(v) => *v,
            IntegerLiteral::U8(v) => *v as i128,
            IntegerLiteral::U16(v) => *v as i128,
            IntegerLiteral::U32(v) => *v as i128,
            IntegerLiteral::U64(v) => *v as i128,
            IntegerLiteral::U128(v) => *v as i128,
        }
    }

    /// Converts the integer literal to a `usize` index.
    ///
    /// Used by the type checker for compile-time tuple indexing and bounds checking.
    /// Signed values are cast directly; callers should validate non-negativity
    /// if required by context.
    pub fn to_usize(&self) -> usize {
        match self {
            IntegerLiteral::I8(v) => *v as usize,
            IntegerLiteral::I16(v) => *v as usize,
            IntegerLiteral::I32(v) => *v as usize,
            IntegerLiteral::I64(v) => *v as usize,
            IntegerLiteral::I128(v) => *v as usize,
            IntegerLiteral::U8(v) => *v as usize,
            IntegerLiteral::U16(v) => *v as usize,
            IntegerLiteral::U32(v) => *v as usize,
            IntegerLiteral::U64(v) => *v as usize,
            IntegerLiteral::U128(v) => *v as usize,
        }
    }
}

impl fmt::Display for IntegerLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntegerLiteral::I8(v) => write!(f, "{}", v),
            IntegerLiteral::I16(v) => write!(f, "{}", v),
            IntegerLiteral::I32(v) => write!(f, "{}", v),
            IntegerLiteral::I64(v) => write!(f, "{}", v),
            IntegerLiteral::I128(v) => write!(f, "{}", v),
            IntegerLiteral::U8(v) => write!(f, "{}", v),
            IntegerLiteral::U16(v) => write!(f, "{}", v),
            IntegerLiteral::U32(v) => write!(f, "{}", v),
            IntegerLiteral::U64(v) => write!(f, "{}", v),
            IntegerLiteral::U128(v) => write!(f, "{}", v),
        }
    }
}

/// Represents a floating-point literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FloatLiteral {
    F32(u32), // Store as u32 to be hashable
    F64(u64),
}

impl FloatLiteral {
    pub fn is_zero(&self) -> bool {
        match self {
            FloatLiteral::F32(v) => f32::from_bits(*v) == 0.0,
            FloatLiteral::F64(v) => f64::from_bits(*v) == 0.0,
        }
    }
}

impl fmt::Display for FloatLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FloatLiteral::F32(v) => write!(f, "{}", f32::from_bits(*v)),
            FloatLiteral::F64(v) => write!(f, "{}", f64::from_bits(*v)),
        }
    }
}
