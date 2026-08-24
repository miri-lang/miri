// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable};
use crate::error::syntax::Span;

/// A type error detected during type checking, with its source location.
#[derive(Debug, PartialEq, Clone)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Span,
    /// When set, this error originates from an imported file (file_path, source_text).
    pub source_override: Option<(String, String)>,
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
    /// A type error identified by a registry code, carrying the specific
    /// message for this occurrence. The code names the family of failure; the
    /// message describes the individual case within it.
    Coded {
        code: DiagnosticCode,
        message: String,
        help: Option<String>,
    },
    /// E0112: Unknown attribute name
    /// E0111: Match on an open enum outside its defining module lacks a catch-all
    NonExhaustiveEnumNeedsCatchAll {
        enum_name: String,
        module: String,
    },
    UnknownAttribute {
        name: String,
        known: Vec<String>,
    },
    /// E0113: Attribute used on the wrong declaration kind
    AttributeNotValid {
        name: String,
        target: String,
        accepted: Vec<String>,
    },
    /// E0114: Attribute argument mismatch
    AttributeArgumentMissing {
        name: String,
    },
    /// E0114: Attribute argument extra
    AttributeArgumentExtra {
        name: String,
    },
    /// E0115: Invalid regex literal (malformed pattern or invalid flags)
    InvalidRegexLiteral {
        reason: String,
    },
    /// E0116: Invalid @test function signature (takes parameters or declares return type)
    InvalidTestFunctionSignature {
        function_name: String,
        reason: String,
    },
    /// E0117: An attribute requiring a companion was used without it
    MissingRequiredAttribute {
        attribute_name: String,
        required_attribute: String,
    },
    /// A syntax/parse error that originated in an imported module, preserved
    /// with its original error code rather than being downgraded to a generic
    /// type error. The title comes from the registry entry for `code`.
    ParseError {
        code: DiagnosticCode,
        message: String,
    },
}

impl TypeErrorKind {
    /// Returns the error code, title, message, and help text for this error kind.
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::UndefinedVariable { name } => {
                ErrorProperties::simple(DiagnosticCode::TypUndefinedVariable)
                    .with_message(format!("Undefined variable: {}", name))
                    .with_help("Ensure the variable is defined and in scope.")
            }
            Self::TypeMismatch { expected, found } => {
                ErrorProperties::simple(DiagnosticCode::TypTypeMismatch)
                    .with_message(format!("Expected type {}, but got {}", expected, found))
                    .with_help("Ensure the types match the expected values.")
            }
            Self::UnknownType { name } => ErrorProperties::simple(DiagnosticCode::TypUnknownType)
                .with_message(format!("Unknown type: {}", name))
                .with_help("Ensure the type is defined and imported correctly."),
            Self::MissingField { field, type_name } => {
                ErrorProperties::simple(DiagnosticCode::TypMissingField)
                    .with_message(format!("Missing field '{}' in type {}", field, type_name))
                    .with_help("Ensure all required fields are initialized.")
            }
            Self::MissingVariant { variant, enum_name } => {
                ErrorProperties::simple(DiagnosticCode::TypMissingVariant)
                    .with_message(format!(
                        "Missing variant '{}' in type {}",
                        variant, enum_name
                    ))
                    .with_help("Ensure the variant is defined in the enum.")
            }
            Self::IncompatibleTypes { lhs, rhs, .. } => {
                ErrorProperties::simple(DiagnosticCode::TypIncompatibleTypesInOperation)
                    .with_message(format!("Types {} and {} are incompatible", lhs, rhs))
                    .with_help("These types cannot be used together in this operation.")
            }
            Self::ImmutableAssignment { name } => {
                ErrorProperties::simple(DiagnosticCode::TypImmutableAssignment)
                    .with_message(format!("Cannot assign to immutable variable: {}", name))
                    .with_help("Declare the variable as mutable using 'mut'.")
            }
            Self::MissingReturn { expected } => ErrorProperties::simple(DiagnosticCode::TypMissingReturnStatement)
                .with_message(format!("Missing return statement of type {}", expected))
                .with_help("Ensure the function returns a value on all paths."),
            Self::InvalidCall { reason } => ErrorProperties::simple(DiagnosticCode::TypInvalidCall)
                .with_message(format!("Invalid call: {}", reason))
                .with_help("Ensure you are calling a function or closure."),
            Self::ArityMismatch { expected, found } => {
                ErrorProperties::simple(DiagnosticCode::TypArityMismatch)
                    .with_message(format!(
                        "Function expects {} arguments, but got {}",
                        expected, found
                    ))
                    .with_help(
                        "Check the function signature and provide the correct number of arguments.",
                    )
            }
            Self::Coded {
                code,
                message,
                help,
            } => crate::error::diagnostic::coded_properties(*code, message, help),
            Self::NonExhaustiveEnumNeedsCatchAll { enum_name, module } => {
                ErrorProperties::simple(DiagnosticCode::TypNonExhaustiveMatchNeedsDefault)
                    .with_message(format!(
                        "Match on `@non_exhaustive` enum '{}' requires a `default` arm outside its defining module '{}'",
                        enum_name, module
                    ))
                    .with_help(format!(
                        "Add a `default:` arm. '{}' may gain variants later, and listing only today's variants would stop compiling when it does.",
                        enum_name
                    ))
            }
            Self::UnknownAttribute { name, known } => {
                ErrorProperties::simple(DiagnosticCode::TypUnknownAttribute)
                    .with_message(format!("Unknown attribute: @{}", name))
                    .with_help(format!(
                        "Attributes are a closed set. Known attributes: {}.",
                        known.join(", ")
                    ))
            }
            Self::AttributeNotValid {
                name,
                target,
                accepted,
            } => {
                let help = if accepted.is_empty() {
                    format!("No attribute is valid on {}.", target)
                } else {
                    format!("Attributes valid on {}: {}.", target, accepted.join(", "))
                };
                ErrorProperties::simple(DiagnosticCode::TypAttributeNotValidOnTarget)
                    .with_message(format!("Attribute @{} is not valid on {}", name, target))
                    .with_help(help)
            }
            Self::AttributeArgumentMissing { name } => {
                ErrorProperties::simple(DiagnosticCode::TypAttributeArgumentMissing)
                    .with_message(format!(
                        "Attribute @{} requires a string literal argument",
                        name
                    ))
                    .with_help(format!("Provide an argument: @{}(\"value\")", name))
            }
            Self::AttributeArgumentExtra { name } => {
                ErrorProperties::simple(DiagnosticCode::TypAttributeArgumentExtra)
                    .with_message(format!("Attribute @{} does not take an argument", name))
                    .with_help(format!("Remove the argument: @{}", name))
            }
            Self::InvalidRegexLiteral { reason } => {
                ErrorProperties::simple(DiagnosticCode::TypInvalidRegexLiteral)
                    .with_message(format!("Invalid regex literal: {}", reason))
            }
            Self::InvalidTestFunctionSignature {
                function_name,
                reason,
            } => {
                ErrorProperties::simple(DiagnosticCode::TypInvalidTestFunctionSignature)
                    .with_message(format!(
                        "Invalid test function signature for '{}': {}",
                        function_name, reason
                    ))
            }
            Self::MissingRequiredAttribute {
                attribute_name,
                required_attribute,
            } => {
                ErrorProperties::simple(DiagnosticCode::TypMissingRequiredAttribute)
                    .with_message(format!(
                        "Attribute @{} requires @{} to be present on the same declaration",
                        attribute_name, required_attribute
                    ))
            }
            Self::ParseError { code, message } => {
                ErrorProperties::simple(*code).with_message(message.clone())
            }
        }
    }
}

impl TypeError {
    /// Creates a new type error of the given kind at the given span.
    pub fn new(kind: TypeErrorKind, span: Span) -> Self {
        Self {
            kind,
            span,
            source_override: None,
        }
    }

    /// Creates a type error that preserves the code and title of a syntax error.
    pub fn from_syntax_error(syntax_err: &crate::error::syntax::SyntaxError) -> Self {
        let props = syntax_err.kind.properties();
        Self {
            kind: TypeErrorKind::ParseError {
                code: props.code,
                message: props.message.unwrap_or_else(|| props.title.to_string()),
            },
            span: syntax_err.span,
            source_override: None,
        }
    }

    /// Creates a type error under the given registry code.
    pub fn coded(code: DiagnosticCode, message: String, span: Span, help: Option<String>) -> Self {
        Self {
            kind: TypeErrorKind::Coded {
                code,
                message,
                help,
            },
            span,
            source_override: None,
        }
    }

    /// Formats this error for terminal display using the given source code.
    pub fn report(&self, source: &str) -> String {
        Reportable::report(self, source)
    }
}

impl Reportable for TypeError {
    fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::from_props(
            self.kind.properties(),
            Some(self.span),
            self.source_override.clone(),
        )
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}
