// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

/// Represents a binary operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitwiseOr,
    BitwiseAnd,
    BitwiseXor,
    Equal,
    NotEqual,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Not,
    And,
    Or,
    Range,
    In,
}

/// Represents a guard operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum GuardOp {
    NotEqual,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Not,
    NotIn,
    In,
}

/// Represents a unary operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum UnaryOp {
    Negate,
    Not,
    Plus,
    BitwiseNot,
    Decrement,
    Increment,
    Await,
}

/// Represents an assignment operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum AssignmentOp {
    Assign,
    AssignAdd,
    AssignSub,
    AssignMul,
    AssignDiv,
    AssignMod,
}
