// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
    Custom {
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
    /// with its original error code and title rather than being downgraded to
    /// a generic "Type Error".
    ParseError {
        code: &'static str,
        title: &'static str,
        message: String,
    },
}

impl TypeErrorKind {
    /// Returns the error code, title, message, and help text for this error kind.
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::UndefinedVariable { name } => {
                ErrorProperties::simple("E0100", "Undefined Variable")
                    .with_message(format!("Undefined variable: {}", name))
                    .with_help("Ensure the variable is defined and in scope.")
            }
            Self::TypeMismatch { expected, found } => {
                ErrorProperties::simple("E0101", "Type Mismatch")
                    .with_message(format!("Expected type {}, but got {}", expected, found))
                    .with_help("Ensure the types match the expected values.")
            }
            Self::UnknownType { name } => ErrorProperties::simple("E0102", "Unknown Type")
                .with_message(format!("Unknown type: {}", name))
                .with_help("Ensure the type is defined and imported correctly."),
            Self::MissingField { field, type_name } => {
                ErrorProperties::simple("E0103", "Missing Field")
                    .with_message(format!("Missing field '{}' in type {}", field, type_name))
                    .with_help("Ensure all required fields are initialized.")
            }
            Self::MissingVariant { variant, enum_name } => {
                ErrorProperties::simple("E0104", "Missing Variant")
                    .with_message(format!(
                        "Missing variant '{}' in type {}",
                        variant, enum_name
                    ))
                    .with_help("Ensure the variant is defined in the enum.")
            }
            Self::IncompatibleTypes { lhs, rhs, .. } => {
                ErrorProperties::simple("E0105", "Incompatible Types")
                    .with_message(format!("Types {} and {} are incompatible", lhs, rhs))
                    .with_help("These types cannot be used together in this operation.")
            }
            Self::ImmutableAssignment { name } => {
                ErrorProperties::simple("E0106", "Immutable Assignment")
                    .with_message(format!("Cannot assign to immutable variable: {}", name))
                    .with_help("Declare the variable as mutable using 'mut'.")
            }
            Self::MissingReturn { expected } => ErrorProperties::simple("E0107", "Missing Return")
                .with_message(format!("Missing return statement of type {}", expected))
                .with_help("Ensure the function returns a value on all paths."),
            Self::InvalidCall { reason } => ErrorProperties::simple("E0108", "Invalid Call")
                .with_message(format!("Invalid call: {}", reason))
                .with_help("Ensure you are calling a function or closure."),
            Self::ArityMismatch { expected, found } => {
                ErrorProperties::simple("E0109", "Arity Mismatch")
                    .with_message(format!(
                        "Function expects {} arguments, but got {}",
                        expected, found
                    ))
                    .with_help(
                        "Check the function signature and provide the correct number of arguments.",
                    )
            }
            Self::Custom { message, .. } => {
                ErrorProperties::simple("E0110", "Type Error").with_message(message.clone())
            }
            Self::NonExhaustiveEnumNeedsCatchAll { enum_name, module } => {
                ErrorProperties::simple("E0111", "Non-Exhaustive Enum Match")
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
                ErrorProperties::simple("E0112", "Unknown Attribute")
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
                ErrorProperties::simple("E0113", "Attribute Not Valid")
                    .with_message(format!("Attribute @{} is not valid on {}", name, target))
                    .with_help(help)
            }
            Self::AttributeArgumentMissing { name } => {
                ErrorProperties::simple("E0114", "Attribute Argument Missing")
                    .with_message(format!(
                        "Attribute @{} requires a string literal argument",
                        name
                    ))
                    .with_help(format!("Provide an argument: @{}(\"value\")", name))
            }
            Self::AttributeArgumentExtra { name } => {
                ErrorProperties::simple("E0114", "Attribute Argument Extra")
                    .with_message(format!("Attribute @{} does not take an argument", name))
                    .with_help(format!("Remove the argument: @{}", name))
            }
            Self::InvalidRegexLiteral { reason } => {
                ErrorProperties::simple("E0115", "Invalid Regex Literal")
                    .with_message(format!("Invalid regex literal: {}", reason))
            }
            Self::InvalidTestFunctionSignature {
                function_name,
                reason,
            } => {
                ErrorProperties::simple("E0116", "Invalid Test Function Signature")
                    .with_message(format!(
                        "Invalid test function signature for '{}': {}",
                        function_name, reason
                    ))
            }
            Self::MissingRequiredAttribute {
                attribute_name,
                required_attribute,
            } => {
                ErrorProperties::simple("E0117", "Missing Required Attribute")
                    .with_message(format!(
                        "Attribute @{} requires @{} to be present on the same declaration",
                        attribute_name, required_attribute
                    ))
            }
            Self::ParseError {
                code,
                title,
                message,
            } => ErrorProperties::simple(code, title).with_message(message.clone()),
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
                title: props.title,
                message: props.message.unwrap_or_else(|| props.title.to_string()),
            },
            span: syntax_err.span,
            source_override: None,
        }
    }

    /// Creates a custom type error with a freeform message.
    pub fn custom(message: String, span: Span, help: Option<String>) -> Self {
        Self {
            kind: TypeErrorKind::Custom { message, help },
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
        let mut props = self.kind.properties();
        if let TypeErrorKind::Custom { help, .. } = &self.kind {
            props.help = help.clone();
        }
        Diagnostic::from_props(props, Some(self.span), self.source_override.clone())
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}
