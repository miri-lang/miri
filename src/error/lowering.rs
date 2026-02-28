// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Error types for MIR lowering.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity, BUG_REPORT_URL};
use crate::error::syntax::Span;

/// An error produced during MIR lowering, with its source location.
#[derive(Debug, Clone, PartialEq)]
pub struct LoweringError {
    pub kind: LoweringErrorKind,
    pub span: Span,
}

/// All possible error variants produced during MIR lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum LoweringErrorKind {
    UnsupportedExpression {
        desc: String,
    },
    UnsupportedStatement {
        desc: String,
    },
    UndefinedVariable {
        name: String,
    },
    TypeNotFound {
        expr_id: usize,
    },
    BreakOutsideLoop,
    ContinueOutsideLoop,
    UnsupportedLhs {
        desc: String,
    },
    UnsupportedOperator {
        op: String,
    },
    UnsupportedRangeType,
    InvalidGpuLaunchArgs {
        expected: usize,
        got: usize,
    },
    UnsupportedType {
        desc: String,
    },
    MissingStructField {
        field: String,
        struct_name: String,
    },
    Custom {
        message: String,
        help: Option<String>,
    },
}

impl LoweringErrorKind {
    /// Returns the error code, title, message, and help text for this error kind.
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::UnsupportedExpression { desc } => ErrorProperties {
                code: "E0200",
                title: "Unsupported Expression",
                message: Some(format!("Unsupported expression: {}", desc)),
                help: Some(
                    "This expression is not yet supported by the compiler. Try rewriting it using simpler constructs."
                        .to_string(),
                ),
            },
            Self::UnsupportedStatement { desc } => ErrorProperties {
                code: "E0201",
                title: "Unsupported Statement",
                message: Some(format!("Unsupported statement: {}", desc)),
                help: Some(
                    "This statement is not yet supported by the compiler. Try rewriting it using simpler constructs."
                        .to_string(),
                ),
            },
            Self::UndefinedVariable { name } => ErrorProperties {
                code: "E0202",
                title: "Undefined Variable",
                message: Some(format!("Undefined variable: {}", name)),
                help: Some("Ensure the variable is defined before use.".to_string()),
            },
            Self::TypeNotFound { .. } => ErrorProperties {
                code: "E0203",
                title: "Type Not Found",
                message: Some(
                    "Could not determine the type of this expression. This is an internal compiler error — please report it."
                        .to_string(),
                ),
                help: Some(format!("Please report this at {}", BUG_REPORT_URL)),
            },
            Self::BreakOutsideLoop => ErrorProperties {
                code: "E0204",
                title: "Break Outside Loop",
                message: Some("break statement outside of loop".to_string()),
                help: Some("Move the break statement inside a loop.".to_string()),
            },
            Self::ContinueOutsideLoop => ErrorProperties {
                code: "E0205",
                title: "Continue Outside Loop",
                message: Some("continue statement outside of loop".to_string()),
                help: Some("Move the continue statement inside a loop.".to_string()),
            },
            Self::UnsupportedLhs { desc } => ErrorProperties {
                code: "E0206",
                title: "Unsupported Left-Hand Side",
                message: Some(format!("Unsupported left-hand side: {}", desc)),
                help: Some("This expression cannot be assigned to.".to_string()),
            },
            Self::UnsupportedOperator { op } => ErrorProperties {
                code: "E0207",
                title: "Unsupported Operator",
                message: Some(format!("Unsupported operator: {}", op)),
                help: Some(
                    "Supported operators: +, -, *, /, %, ==, !=, <, >, <=, >=, &&, ||.".to_string(),
                ),
            },
            Self::UnsupportedRangeType => ErrorProperties {
                code: "E0208",
                title: "Unsupported Range Type",
                message: Some("Unsupported range type for loop".to_string()),
                help: Some("Use exclusive (..) or inclusive (..=) ranges.".to_string()),
            },
            Self::InvalidGpuLaunchArgs { expected, got } => ErrorProperties {
                code: "E0209",
                title: "Invalid GPU Launch Arguments",
                message: Some(format!(
                    "GPU launch expects {} arguments, got {}",
                    expected, got
                )),
                help: Some(
                    "GPU launch requires exactly 2 arguments: grid and block dimensions."
                        .to_string(),
                ),
            },
            Self::UnsupportedType { desc } => ErrorProperties {
                code: "E0210",
                title: "Unsupported Type",
                message: Some(format!("Unsupported type: {}", desc)),
                help: Some(
                    "This type is not yet supported by the compiler. Use a supported type instead."
                        .to_string(),
                ),
            },
            Self::MissingStructField { field, struct_name } => ErrorProperties {
                code: "E0211",
                title: "Missing Struct Field",
                message: Some(format!(
                    "Missing field '{}' in struct '{}' constructor",
                    field, struct_name
                )),
                help: Some("Provide a value for all required struct fields.".to_string()),
            },
            Self::Custom { message, .. } => ErrorProperties {
                code: "E0299",
                title: "Lowering Error",
                message: Some(message.clone()),
                help: None,
            },
        }
    }
}

impl LoweringError {
    /// Creates a new lowering error of the given kind at the given span.
    pub fn new(kind: LoweringErrorKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Creates a custom lowering error with a freeform message.
    pub fn custom(message: String, span: Span, help: Option<String>) -> Self {
        Self {
            kind: LoweringErrorKind::Custom { message, help },
            span,
        }
    }

    /// Creates an unsupported expression error.
    pub fn unsupported_expression(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedExpression { desc: desc.into() },
            span,
        )
    }

    /// Creates an unsupported statement error.
    pub fn unsupported_statement(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedStatement { desc: desc.into() },
            span,
        )
    }

    /// Creates an undefined variable error.
    pub fn undefined_variable(name: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UndefinedVariable { name: name.into() },
            span,
        )
    }

    /// Creates a type-not-found error for the given expression ID.
    pub fn type_not_found(expr_id: usize, span: Span) -> Self {
        Self::new(LoweringErrorKind::TypeNotFound { expr_id }, span)
    }

    /// Creates a break-outside-loop error.
    pub fn break_outside_loop(span: Span) -> Self {
        Self::new(LoweringErrorKind::BreakOutsideLoop, span)
    }

    /// Creates a continue-outside-loop error.
    pub fn continue_outside_loop(span: Span) -> Self {
        Self::new(LoweringErrorKind::ContinueOutsideLoop, span)
    }

    /// Creates an unsupported left-hand side error.
    pub fn unsupported_lhs(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedLhs { desc: desc.into() },
            span,
        )
    }

    /// Creates an unsupported operator error.
    pub fn unsupported_operator(op: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedOperator { op: op.into() },
            span,
        )
    }

    /// Creates an unsupported range type error.
    pub fn unsupported_range_type(span: Span) -> Self {
        Self::new(LoweringErrorKind::UnsupportedRangeType, span)
    }

    /// Creates an invalid GPU launch arguments error.
    pub fn invalid_gpu_launch_args(expected: usize, got: usize, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::InvalidGpuLaunchArgs { expected, got },
            span,
        )
    }

    /// Creates an unsupported type error.
    pub fn unsupported_type(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedType { desc: desc.into() },
            span,
        )
    }

    /// Creates a missing struct field error.
    pub fn missing_struct_field(
        field: impl Into<String>,
        struct_name: impl Into<String>,
        span: Span,
    ) -> Self {
        Self::new(
            LoweringErrorKind::MissingStructField {
                field: field.into(),
                struct_name: struct_name.into(),
            },
            span,
        )
    }

    /// Formats this error for terminal display using the given source code.
    pub fn report(&self, source: &str) -> String {
        Reportable::report(self, source)
    }
}

impl Reportable for LoweringError {
    fn to_diagnostic(&self) -> Diagnostic {
        let props = self.kind.properties();
        let help = if let LoweringErrorKind::Custom { help, .. } = &self.kind {
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
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for LoweringError {}
