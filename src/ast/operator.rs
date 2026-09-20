// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
    NullCoalesce,
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

impl AssignmentOp {
    /// The binary operator this assignment combines the target with, or `None`
    /// for a plain assignment, which combines with nothing.
    ///
    /// `x op= y` means `x op y` stored back into `x`, so every layer that has
    /// to decide what the compound form yields — what type it produces, which
    /// method it calls — asks the operator it names. Reading the two spellings
    /// differently is what let `s += t` add two addresses while `s + t`
    /// concatenated.
    pub fn binary_op(self) -> Option<BinaryOp> {
        match self {
            AssignmentOp::AssignAdd => Some(BinaryOp::Add),
            AssignmentOp::AssignSub => Some(BinaryOp::Sub),
            AssignmentOp::AssignMul => Some(BinaryOp::Mul),
            AssignmentOp::AssignDiv => Some(BinaryOp::Div),
            AssignmentOp::AssignMod => Some(BinaryOp::Mod),
            AssignmentOp::Assign => None,
        }
    }
}
