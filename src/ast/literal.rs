// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
            _ => false,
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
