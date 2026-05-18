// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Error types for MIR lowering.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, BUG_REPORT_URL};
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
            Self::UnsupportedExpression { desc } => {
                ErrorProperties::simple("E0200", "Unsupported Expression")
                    .with_message(format!("Unsupported expression: {}", desc))
                    .with_help(
                        "This expression is not yet supported by the compiler. \
                         Try rewriting it using simpler constructs.",
                    )
            }
            Self::UnsupportedStatement { desc } => {
                ErrorProperties::simple("E0201", "Unsupported Statement")
                    .with_message(format!("Unsupported statement: {}", desc))
                    .with_help(
                        "This statement is not yet supported by the compiler. \
                         Try rewriting it using simpler constructs.",
                    )
            }
            Self::UndefinedVariable { name } => {
                ErrorProperties::simple("E0202", "Undefined Variable")
                    .with_message(format!("Undefined variable: {}", name))
                    .with_help("Ensure the variable is defined before use.")
            }
            Self::TypeNotFound { .. } => ErrorProperties::simple("E0203", "Type Not Found")
                .with_message(
                    "Could not determine the type of this expression. \
                     This is an internal compiler error — please report it.",
                )
                .with_help(format!("Please report this at {}", BUG_REPORT_URL)),
            Self::BreakOutsideLoop => ErrorProperties::simple("E0204", "Break Outside Loop")
                .with_message("break statement outside of loop")
                .with_help("Move the break statement inside a loop."),
            Self::ContinueOutsideLoop => ErrorProperties::simple("E0205", "Continue Outside Loop")
                .with_message("continue statement outside of loop")
                .with_help("Move the continue statement inside a loop."),
            Self::UnsupportedLhs { desc } => {
                ErrorProperties::simple("E0206", "Unsupported Left-Hand Side")
                    .with_message(format!("Unsupported left-hand side: {}", desc))
                    .with_help("This expression cannot be assigned to.")
            }
            Self::UnsupportedOperator { op } => {
                ErrorProperties::simple("E0207", "Unsupported Operator")
                    .with_message(format!("Unsupported operator: {}", op))
                    .with_help("Supported operators: +, -, *, /, %, ==, !=, <, >, <=, >=, &&, ||.")
            }
            Self::UnsupportedRangeType => {
                ErrorProperties::simple("E0208", "Unsupported Range Type")
                    .with_message("Unsupported range type for loop")
                    .with_help("Use exclusive (..) or inclusive (..=) ranges.")
            }
            Self::InvalidGpuLaunchArgs { expected, got } => {
                ErrorProperties::simple("E0209", "Invalid GPU Launch Arguments")
                    .with_message(format!(
                        "GPU launch expects {} arguments, got {}",
                        expected, got
                    ))
                    .with_help(
                        "GPU launch requires exactly 2 arguments: grid and block dimensions.",
                    )
            }
            Self::UnsupportedType { desc } => ErrorProperties::simple("E0210", "Unsupported Type")
                .with_message(format!("Unsupported type: {}", desc))
                .with_help(
                    "This type is not yet supported by the compiler. \
                     Use a supported type instead.",
                ),
            Self::MissingStructField { field, struct_name } => {
                ErrorProperties::simple("E0211", "Missing Struct Field")
                    .with_message(format!(
                        "Missing field '{}' in struct '{}' constructor",
                        field, struct_name
                    ))
                    .with_help("Provide a value for all required struct fields.")
            }
            Self::Custom { message, .. } => {
                ErrorProperties::simple("E0299", "Lowering Error").with_message(message.clone())
            }
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
        let mut props = self.kind.properties();
        if let LoweringErrorKind::Custom { help, .. } = &self.kind {
            props.help = help.clone();
        }
        Diagnostic::from_props(props, Some(self.span), None)
    }
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for LoweringError {}
