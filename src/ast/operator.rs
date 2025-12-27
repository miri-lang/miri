// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

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
    Range, // Represents a range operator (e.g., `1..10`)
    In,    // Represents the `in` operator for membership tests
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
    Negate, // - operator
    Not,
    Plus,       // + operator (unary plus)
    BitwiseNot, // ~ operator
    Decrement,  // -- operator
    Increment,  // ++ operator
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
