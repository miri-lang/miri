// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Interpreter errors.
//!
//! Error types for the MIR interpreter, consolidated in the error module
//! for consistent formatting and reporting.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpreterError {
    pub kind: InterpreterErrorKind,
}

/// Errors that can occur during MIR interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpreterErrorKind {
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

impl InterpreterErrorKind {
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::UndefinedFunction(name) => ErrorProperties {
                code: "E0403",
                title: "Undefined Function",
                message: Some(format!("Undefined function: {}", name)),
                help: Some("Ensure the function is defined and imported correctly.".to_string()),
            },
            Self::TypeMismatch {
                expected,
                got,
                context,
            } => ErrorProperties {
                code: "E0404",
                title: "Type Mismatch",
                message: Some(format!(
                    "Type mismatch in {}: expected {}, got {}",
                    context, expected, got
                )),
                help: Some("Ensure types match the expected values.".to_string()),
            },
            Self::DivisionByZero => ErrorProperties {
                code: "E0400",
                title: "Division by Zero",
                message: Some("attempt to divide by zero".to_string()),
                help: Some("Check the divisor to ensure it is not zero.".to_string()),
            },
            Self::RemainderByZero => ErrorProperties {
                code: "E0401",
                title: "Remainder by Zero",
                message: Some(
                    "attempt to calculate the remainder with a divisor of zero".to_string(),
                ),
                help: Some("Check the divisor to ensure it is not zero.".to_string()),
            },
            Self::Overflow => ErrorProperties {
                code: "E0402",
                title: "Integer Overflow",
                message: Some("Integer overflow".to_string()),
                help: Some(
                    "The result of the operation exceeds the integer type limits.".to_string(),
                ),
            },
            Self::InvalidOperand { operation, operand } => ErrorProperties {
                code: "E0405",
                title: "Invalid Operand",
                message: Some(format!("Invalid operand for {}: {}", operation, operand)),
                help: Some("Ensure the operand is valid for the operation.".to_string()),
            },
            Self::UndefinedLocal(idx) => ErrorProperties {
                code: "E0406",
                title: "Undefined Local Variable",
                message: Some(format!("Undefined local variable: _{}", idx)),
                help: Some("Internal error: Local variable accessed but not defined.".to_string()),
            },
            Self::UninitializedLocal(idx) => ErrorProperties {
                code: "E0407",
                title: "Uninitialized Local Variable",
                message: Some(format!("Uninitialized local variable: _{}", idx)),
                help: Some(
                    "Internal error: Local variable accessed before initialization.".to_string(),
                ),
            },
            Self::InvalidBlock(idx) => ErrorProperties {
                code: "E0408",
                title: "Invalid Block",
                message: Some(format!("Invalid basic block: bb{}", idx)),
                help: Some("Internal error: Jump to a non-existent basic block.".to_string()),
            },
            Self::StackOverflow => ErrorProperties {
                code: "E0409",
                title: "Stack Overflow",
                message: Some("Stack overflow".to_string()),
                help: Some("Recursion depth exceeded the limit.".to_string()),
            },
            Self::NotImplemented(feature) => ErrorProperties {
                code: "E0410",
                title: "Not Implemented",
                message: Some(format!("Not implemented: {}", feature)),
                help: Some("This feature is not yet supported.".to_string()),
            },
            Self::Internal(msg) => ErrorProperties {
                code: "E0411",
                title: "Internal Error",
                message: Some(format!("Internal error: {}", msg)),
                help: Some("Please report this issue to the Miri developers.".to_string()),
            },
        }
    }
}

impl InterpreterError {
    pub fn new(kind: InterpreterErrorKind) -> Self {
        Self { kind }
    }

    pub fn undefined_function(name: impl Into<String>) -> Self {
        Self::new(InterpreterErrorKind::UndefinedFunction(name.into()))
    }

    pub fn type_mismatch(
        expected: impl Into<String>,
        got: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self::new(InterpreterErrorKind::TypeMismatch {
            expected: expected.into(),
            got: got.into(),
            context: context.into(),
        })
    }

    pub fn division_by_zero() -> Self {
        Self::new(InterpreterErrorKind::DivisionByZero)
    }

    pub fn remainder_by_zero() -> Self {
        Self::new(InterpreterErrorKind::RemainderByZero)
    }

    pub fn overflow() -> Self {
        Self::new(InterpreterErrorKind::Overflow)
    }

    pub fn invalid_operand(operation: impl Into<String>, operand: impl Into<String>) -> Self {
        Self::new(InterpreterErrorKind::InvalidOperand {
            operation: operation.into(),
            operand: operand.into(),
        })
    }

    pub fn undefined_local(idx: usize) -> Self {
        Self::new(InterpreterErrorKind::UndefinedLocal(idx))
    }

    pub fn uninitialized_local(idx: usize) -> Self {
        Self::new(InterpreterErrorKind::UninitializedLocal(idx))
    }

    pub fn invalid_block(idx: usize) -> Self {
        Self::new(InterpreterErrorKind::InvalidBlock(idx))
    }

    pub fn stack_overflow() -> Self {
        Self::new(InterpreterErrorKind::StackOverflow)
    }

    pub fn not_implemented(feature: impl Into<String>) -> Self {
        Self::new(InterpreterErrorKind::NotImplemented(feature.into()))
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(InterpreterErrorKind::Internal(msg.into()))
    }
}

impl Reportable for InterpreterError {
    fn to_diagnostic(&self) -> Diagnostic {
        let props = self.kind.properties();
        Diagnostic {
            severity: Severity::Error,
            code: Some(props.code),
            title: props.title.to_string(),
            message: props.message.unwrap_or_else(|| props.title.to_string()),
            span: None, // Interpreter errors don't have source spans
            help: props.help,
            notes: Vec::new(),
        }
    }
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for InterpreterError {}
