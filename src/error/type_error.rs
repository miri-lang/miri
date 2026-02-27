// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};
use crate::error::format::format_diagnostic;
use crate::error::syntax::Span;

/// A type error detected during type checking, with its source location.
#[derive(Debug, PartialEq, Clone)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Span,
}

/// All possible type error variants produced by the type checker.
#[derive(Debug, PartialEq, Clone)]
pub enum TypeErrorKind {
    UndefinedVariable {
        name: String,
    },
    TypeMismatch {
        expected: String,
        found: String,
    },
    UnknownType {
        name: String,
    },
    MissingField {
        field: String,
        type_name: String,
    },
    MissingVariant {
        variant: String,
        enum_name: String,
    },
    IncompatibleTypes {
        op: String,
        lhs: String,
        rhs: String,
    },
    ImmutableAssignment {
        name: String,
    },
    MissingReturn {
        expected: String,
    },
    InvalidCall {
        reason: String,
    },
    ArityMismatch {
        expected: usize,
        found: usize,
    },
    Custom {
        message: String,
        help: Option<String>,
    },
}

impl TypeErrorKind {
    /// Returns the error code, title, message, and help text for this error kind.
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::UndefinedVariable { name } => ErrorProperties {
                code: "E0100",
                title: "Undefined Variable",
                message: Some(format!("Undefined variable: {}", name)),
                help: Some("Ensure the variable is defined and in scope.".to_string()),
            },
            Self::TypeMismatch { expected, found } => ErrorProperties {
                code: "E0101",
                title: "Type Mismatch",
                message: Some(format!("Expected type {}, but got {}", expected, found)),
                help: Some("Ensure the types match the expected values.".to_string()),
            },
            Self::UnknownType { name } => ErrorProperties {
                code: "E0102",
                title: "Unknown Type",
                message: Some(format!("Unknown type: {}", name)),
                help: Some("Ensure the type is defined and imported correctly.".to_string()),
            },
            Self::MissingField { field, type_name } => ErrorProperties {
                code: "E0103",
                title: "Missing Field",
                message: Some(format!("Missing field '{}' in type {}", field, type_name)),
                help: Some("Ensure all required fields are initialized.".to_string()),
            },
            Self::MissingVariant { variant, enum_name } => ErrorProperties {
                code: "E0104",
                title: "Missing Variant",
                message: Some(format!(
                    "Missing variant '{}' in type {}",
                    variant, enum_name
                )),
                help: Some("Ensure the variant is defined in the enum.".to_string()),
            },
            Self::IncompatibleTypes { lhs, rhs, .. } => ErrorProperties {
                code: "E0105",
                title: "Incompatible Types",
                message: Some(format!("Types {} and {} are incompatible", lhs, rhs)),
                help: Some("These types cannot be used together in this operation.".to_string()),
            },
            Self::ImmutableAssignment { name } => ErrorProperties {
                code: "E0106",
                title: "Immutable Assignment",
                message: Some(format!("Cannot assign to immutable variable: {}", name)),
                help: Some("Declare the variable as mutable using 'mut'.".to_string()),
            },
            Self::MissingReturn { expected } => ErrorProperties {
                code: "E0107",
                title: "Missing Return",
                message: Some(format!("Missing return statement of type {}", expected)),
                help: Some("Ensure the function returns a value on all paths.".to_string()),
            },
            Self::InvalidCall { reason } => ErrorProperties {
                code: "E0108",
                title: "Invalid Call",
                message: Some(format!("Invalid call: {}", reason)),
                help: Some("Ensure you are calling a function or closure.".to_string()),
            },
            Self::ArityMismatch { expected, found } => ErrorProperties {
                code: "E0109",
                title: "Arity Mismatch",
                message: Some(format!(
                    "Function expects {} arguments, but got {}",
                    expected, found
                )),
                help: Some(
                    "Check the function signature and provide the correct number of arguments."
                        .to_string(),
                ),
            },
            Self::Custom { message, .. } => ErrorProperties {
                code: "E0110",
                title: "Type Error",
                message: Some(message.clone()),
                help: None,
            },
        }
    }
}

impl TypeError {
    /// Creates a new type error of the given kind at the given span.
    pub fn new(kind: TypeErrorKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Creates a custom type error with a freeform message.
    pub fn custom(message: String, span: Span, help: Option<String>) -> Self {
        Self {
            kind: TypeErrorKind::Custom { message, help },
            span,
        }
    }

    /// Formats this error for terminal display using the given source code.
    pub fn report(&self, source: &str) -> String {
        Reportable::report(self, source)
    }
}

impl Reportable for TypeError {
    fn to_diagnostic(&self) -> Diagnostic {
        let props = self.kind.properties();
        let help = if let TypeErrorKind::Custom { help, .. } = &self.kind {
            help.clone()
        } else {
            props.help
        };

        Diagnostic {
            severity: Severity::Error,
            code: Some(props.code),
            title: props.title.to_string(),
            message: props.message.unwrap_or_else(|| props.title.to_string()),
            span: Some(self.span),
            help,
            notes: Vec::new(),
        }
    }

    fn report(&self, source: &str) -> String {
        let props = self.kind.properties();
        let help = if let TypeErrorKind::Custom { help, .. } = &self.kind {
            help.as_deref()
        } else {
            props.help.as_deref()
        };

        format_diagnostic(
            source,
            &self.span,
            props.message.as_deref().unwrap_or(props.title),
            "error",
            help,
        )
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}
