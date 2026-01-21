// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Error types for MIR lowering.

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};
use crate::error::format::format_diagnostic;
use crate::error::syntax::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct LoweringError {
    pub kind: LoweringErrorKind,
    pub span: Span,
}

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
    // Fallback
    Custom {
        message: String,
        help: Option<String>,
    },
}

impl LoweringErrorKind {
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::UnsupportedExpression { desc } => ErrorProperties {
                code: "E0200",
                title: "Unsupported Expression",
                message: Some(format!("Unsupported expression: {}", desc)),
                help: Some(
                    "This expression type is not yet supported in MIR lowering.".to_string(),
                ),
            },
            Self::UnsupportedStatement { desc } => ErrorProperties {
                code: "E0201",
                title: "Unsupported Statement",
                message: Some(format!("Unsupported statement: {}", desc)),
                help: Some("This statement type is not yet supported in MIR lowering.".to_string()),
            },
            Self::UndefinedVariable { name } => ErrorProperties {
                code: "E0202",
                title: "Undefined Variable",
                message: Some(format!("Undefined variable: {}", name)),
                help: Some("Ensure the variable is defined before use.".to_string()),
            },
            Self::TypeNotFound { expr_id } => ErrorProperties {
                code: "E0203",
                title: "Type Not Found",
                message: Some(format!("Type not found for expression ID {}", expr_id)),
                help: Some("This indicates an internal type checking failure.".to_string()),
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
                help: Some("This operator is not yet supported in MIR lowering.".to_string()),
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
                help: Some("This type is not yet supported in MIR lowering.".to_string()),
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
    pub fn new(kind: LoweringErrorKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Helper for backward compatibility or custom errors
    pub fn custom(message: String, span: Span, help: Option<String>) -> Self {
        Self {
            kind: LoweringErrorKind::Custom { message, help },
            span,
        }
    }

    pub fn unsupported_expression(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedExpression { desc: desc.into() },
            span,
        )
    }

    pub fn unsupported_statement(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedStatement { desc: desc.into() },
            span,
        )
    }

    pub fn undefined_variable(name: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UndefinedVariable { name: name.into() },
            span,
        )
    }

    pub fn type_not_found(expr_id: usize, span: Span) -> Self {
        Self::new(LoweringErrorKind::TypeNotFound { expr_id }, span)
    }

    pub fn break_outside_loop(span: Span) -> Self {
        Self::new(LoweringErrorKind::BreakOutsideLoop, span)
    }

    pub fn continue_outside_loop(span: Span) -> Self {
        Self::new(LoweringErrorKind::ContinueOutsideLoop, span)
    }

    pub fn unsupported_lhs(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedLhs { desc: desc.into() },
            span,
        )
    }

    pub fn unsupported_operator(op: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedOperator { op: op.into() },
            span,
        )
    }

    pub fn unsupported_range_type(span: Span) -> Self {
        Self::new(LoweringErrorKind::UnsupportedRangeType, span)
    }

    pub fn invalid_gpu_launch_args(expected: usize, got: usize, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::InvalidGpuLaunchArgs { expected, got },
            span,
        )
    }

    pub fn unsupported_type(desc: impl Into<String>, span: Span) -> Self {
        Self::new(
            LoweringErrorKind::UnsupportedType { desc: desc.into() },
            span,
        )
    }

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
            span: Some(self.span.clone()),
            help,
            notes: Vec::new(),
        }
    }

    fn report(&self, source: &str) -> String {
        let props = self.kind.properties();
        let help = if let LoweringErrorKind::Custom { help, .. } = &self.kind {
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

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl std::error::Error for LoweringError {}
