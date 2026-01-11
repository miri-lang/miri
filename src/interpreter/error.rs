// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Interpreter errors.

use std::fmt;

/// Errors that can occur during MIR interpretation.
#[derive(Debug, Clone)]
pub enum InterpreterError {
    /// Attempted to call an undefined function.
    UndefinedFunction(String),
    /// Type mismatch during execution.
    TypeMismatch {
        expected: String,
        got: String,
        context: String,
    },
    /// Division by zero.
    DivisionByZero,
    /// Remainder by zero.
    RemainderByZero,
    /// Integer overflow.
    Overflow,
    /// Invalid operand for operation.
    InvalidOperand { operation: String, operand: String },
    /// Undefined local variable.
    UndefinedLocal(usize),
    /// Uninitialized local variable.
    UninitializedLocal(usize),
    /// Invalid basic block reference.
    InvalidBlock(usize),
    /// Stack overflow (too many nested calls).
    StackOverflow,
    /// Feature not yet implemented.
    NotImplemented(String),
    /// Internal interpreter error.
    Internal(String),
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InterpreterError::UndefinedFunction(name) => {
                write!(f, "Undefined function: {}", name)
            }
            InterpreterError::TypeMismatch {
                expected,
                got,
                context,
            } => {
                write!(
                    f,
                    "Type mismatch in {}: expected {}, got {}",
                    context, expected, got
                )
            }
            InterpreterError::DivisionByZero => {
                write!(f, "{}", crate::error::RuntimeError::DivisionByZero)
            }
            InterpreterError::RemainderByZero => {
                write!(f, "{}", crate::error::RuntimeError::RemainderByZero)
            }
            InterpreterError::Overflow => write!(f, "Integer overflow"),
            InterpreterError::InvalidOperand { operation, operand } => {
                write!(f, "Invalid operand for {}: {}", operation, operand)
            }
            InterpreterError::UndefinedLocal(idx) => {
                write!(f, "Undefined local variable: _{}", idx)
            }
            InterpreterError::UninitializedLocal(idx) => {
                write!(f, "Uninitialized local variable: _{}", idx)
            }
            InterpreterError::InvalidBlock(idx) => {
                write!(f, "Invalid basic block: bb{}", idx)
            }
            InterpreterError::StackOverflow => write!(f, "Stack overflow"),
            InterpreterError::NotImplemented(feature) => {
                write!(f, "Not implemented: {}", feature)
            }
            InterpreterError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for InterpreterError {}
