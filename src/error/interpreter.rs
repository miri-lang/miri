// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Interpreter errors.
//!
//! Error types for the MIR interpreter, consolidated in the error module
//! for consistent formatting and reporting.

use crate::error::codes;
use crate::error::diagnostic::{Diagnostic, Reportable, Severity};
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

impl InterpreterError {
    /// Get the error code for this interpreter error.
    pub fn code(&self) -> &'static str {
        match self {
            InterpreterError::UndefinedFunction(_) => codes::runtime::UNDEFINED_FUNCTION,
            InterpreterError::TypeMismatch { .. } => codes::runtime::TYPE_MISMATCH,
            InterpreterError::DivisionByZero => codes::runtime::DIVISION_BY_ZERO,
            InterpreterError::RemainderByZero => codes::runtime::REMAINDER_BY_ZERO,
            InterpreterError::Overflow => codes::runtime::OVERFLOW,
            InterpreterError::InvalidOperand { .. } => codes::runtime::INVALID_OPERAND,
            InterpreterError::UndefinedLocal(_) => codes::runtime::UNDEFINED_LOCAL,
            InterpreterError::UninitializedLocal(_) => codes::runtime::UNINITIALIZED_LOCAL,
            InterpreterError::InvalidBlock(_) => codes::runtime::INVALID_BLOCK,
            InterpreterError::StackOverflow => codes::runtime::STACK_OVERFLOW,
            InterpreterError::NotImplemented(_) => codes::runtime::NOT_IMPLEMENTED,
            InterpreterError::Internal(_) => codes::runtime::INTERNAL,
        }
    }

    /// Get the human-readable title for this error.
    pub fn title(&self) -> &'static str {
        match self {
            InterpreterError::UndefinedFunction(_) => "Undefined Function",
            InterpreterError::TypeMismatch { .. } => "Type Mismatch",
            InterpreterError::DivisionByZero => "Division by Zero",
            InterpreterError::RemainderByZero => "Remainder by Zero",
            InterpreterError::Overflow => "Integer Overflow",
            InterpreterError::InvalidOperand { .. } => "Invalid Operand",
            InterpreterError::UndefinedLocal(_) => "Undefined Local Variable",
            InterpreterError::UninitializedLocal(_) => "Uninitialized Local Variable",
            InterpreterError::InvalidBlock(_) => "Invalid Block",
            InterpreterError::StackOverflow => "Stack Overflow",
            InterpreterError::NotImplemented(_) => "Not Implemented",
            InterpreterError::Internal(_) => "Internal Error",
        }
    }
}

impl Reportable for InterpreterError {
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: Some(self.code()),
            title: self.title().to_string(),
            message: self.to_string(),
            span: None, // Interpreter errors don't have source spans
            help: None,
            notes: Vec::new(),
        }
    }
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
