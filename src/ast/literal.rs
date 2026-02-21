// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::lexer::RegexToken;

/// Represents a literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    String(String),
    Boolean(bool),
    Symbol(String),
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
