// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};
use crate::error::format::format_diagnostic;

// Span type for tracking positions in source code
pub type Span = std::ops::Range<usize>;

#[derive(Debug, PartialEq, Clone)]
pub struct SyntaxError {
    pub kind: SyntaxErrorKind,
    pub span: Span,
}

#[derive(Debug, PartialEq, Clone)]
pub enum SyntaxErrorKind {
    // Lexer errors
    InvalidToken,
    UnclosedMultilineComment,
    IndentationMismatch,
    UnclosedStringLiteral,

    // Parser errors
    UnexpectedToken { expected: String, found: String },
    UnexpectedOperator { expected: String, found: String },
    UnexpectedEOF,

    InvalidTypeDeclaration { expected: String },
    InvalidLeftHandSideExpression,
    InvalidAssignmentTarget,
    IntegerLiteralOverflow,
    InvalidNumberLiteral,
    InvalidIntegerLiteral,
    InvalidBinaryLiteral,
    InvalidOctalLiteral,
    InvalidHexLiteral,
    InvalidFloatLiteral,
    InvalidStringLiteral,
    InvalidBooleanLiteral,
    InvalidInheritanceIdentifier,
    InvalidRegexLiteral,
    InvalidFormattedString,
    InvalidFormattedStringExpression,
    BackslashInFStringExpression,

    MissingStructMemberType,
    MissingStructMembers,
    MissingEnumMembers,
    MissingTypeExpression,

    DuplicateMatchPattern,
    MissingMatchBranches,

    InvalidModifierCombination { combination: String, reason: String },
}

impl SyntaxErrorKind {
    pub fn properties(&self) -> ErrorProperties {
        match self {
            Self::InvalidToken => ErrorProperties {
                code: "E0001",
                title: "Invalid Token",
                message: None,
                help: Some("The character or sequence of characters here is not a valid part of the language.".to_string()),
            },
            Self::UnclosedMultilineComment => ErrorProperties {
                code: "E0002",
                title: "Unclosed Multiline Comment",
                message: None,
                help: Some("Add '*/' to close the comment.".to_string()),
            },
            Self::IndentationMismatch => ErrorProperties {
                code: "E0003",
                title: "Indentation Mismatch",
                message: None,
                help: Some("Ensure the indentation level matches the surrounding code block.".to_string()),
            },
            Self::UnclosedStringLiteral => ErrorProperties {
                code: "E0004",
                title: "Unclosed String Literal",
                message: None,
                help: Some("Add a closing quote to the string literal.".to_string()),
            },
            Self::UnexpectedToken { expected, found } => ErrorProperties {
                code: "E0005",
                title: "Unexpected Token",
                message: Some(format!("Expected {}, but found {}", expected, found)),
                help: None,
            },
            Self::UnexpectedEOF => ErrorProperties {
                code: "E0006",
                title: "Unexpected End of File",
                message: Some("Unexpected end of file".to_string()),
                help: Some("The file ended unexpectedly. Check for unclosed blocks or expressions.".to_string()),
            },
            Self::InvalidTypeDeclaration { expected: _ } => ErrorProperties {
                code: "E0007",
                title: "Invalid Type Declaration",
                message: None,
                help: Some("Types must be declared with a valid identifier.".to_string()),
            },
            Self::InvalidAssignmentTarget => ErrorProperties {
                code: "E0008",
                title: "Invalid Assignment Target",
                message: None,
                help: Some("You can only assign values to variables or mutable properties.".to_string()),
            },
            Self::IntegerLiteralOverflow => ErrorProperties {
                code: "E0009",
                title: "Integer Overflow",
                message: None,
                help: Some("The integer literal is too large for the target type.".to_string()),
            },
            Self::InvalidIntegerLiteral => ErrorProperties {
                code: "E0010",
                title: "Invalid Integer Literal",
                message: None,
                help: Some("Ensure the integer literal format is correct.".to_string()),
            },
            Self::InvalidBinaryLiteral => ErrorProperties {
                code: "E0011",
                title: "Invalid Binary Literal",
                message: None,
                help: Some("Binary literals must start with '0b' followed by 0s and 1s.".to_string()),
            },
            Self::InvalidOctalLiteral => ErrorProperties {
                code: "E0012",
                title: "Invalid Octal Literal",
                message: None,
                help: Some("Octal literals must start with '0o' followed by digits 0-7.".to_string()),
            },
            Self::InvalidHexLiteral => ErrorProperties {
                code: "E0013",
                title: "Invalid Hex Literal",
                message: None,
                help: Some("Hexadecimal literals must start with '0x' followed by hex digits.".to_string()),
            },
            Self::InvalidFloatLiteral => ErrorProperties {
                code: "E0014",
                title: "Invalid Float Literal",
                message: None,
                help: Some("Ensure the float literal format is correct.".to_string()),
            },
            Self::InvalidStringLiteral => ErrorProperties {
                code: "E0015",
                title: "Invalid String Literal",
                message: None,
                help: Some("Ensure the string literal is properly quoted and escaped.".to_string()),
            },
            Self::InvalidBooleanLiteral => ErrorProperties {
                code: "E0016",
                title: "Invalid Boolean Literal",
                message: None,
                help: Some("Boolean literals must be 'true' or 'false'.".to_string()),
            },
            Self::UnexpectedOperator { expected: _, found: _ } => ErrorProperties {
                code: "E0017",
                title: "Unexpected Operator",
                message: None,
                help: Some("This operator cannot be used in this context.".to_string()),
            },
            Self::InvalidLeftHandSideExpression => ErrorProperties {
                code: "E0018",
                title: "Invalid Left-Hand Side Expression",
                message: None,
                help: Some("The expression on the left side of the assignment is not valid.".to_string()),
            },
            Self::MissingStructMemberType => ErrorProperties {
                code: "E0019",
                title: "Missing Struct Member Type",
                message: None,
                help: Some("Struct members must have a type annotation.".to_string()),
            },
            Self::InvalidInheritanceIdentifier => ErrorProperties {
                code: "E0020",
                title: "Invalid Inheritance Identifier",
                message: None,
                help: Some("Parent type in inheritance must be a valid identifier.".to_string()),
            },
            Self::DuplicateMatchPattern => ErrorProperties {
                code: "E0021",
                title: "Duplicate Match Pattern",
                message: None,
                help: Some("This pattern is already covered in a previous branch.".to_string()),
            },
            Self::MissingMatchBranches => ErrorProperties {
                code: "E0022",
                title: "Missing Match Branches",
                message: None,
                help: Some("The match expression must cover all possible cases.".to_string()),
            },
            Self::InvalidRegexLiteral => ErrorProperties {
                code: "E0023",
                title: "Invalid Regex Literal",
                message: None,
                help: Some("Ensure the regex pattern is valid.".to_string()),
            },
            Self::InvalidFormattedString => ErrorProperties {
                code: "E0024",
                title: "Invalid Formatted String",
                message: None,
                help: Some("The format string syntax is incorrect.".to_string()),
            },
            Self::InvalidFormattedStringExpression => ErrorProperties {
                code: "E0025",
                title: "Invalid Formatted String Expression",
                message: None,
                help: Some("The expression inside the format string is invalid.".to_string()),
            },
            Self::BackslashInFStringExpression => ErrorProperties {
                code: "E0026",
                title: "Backslash in Format String",
                message: None,
                help: Some("Backslashes are not allowed in format string expressions.".to_string()),
            },
            Self::InvalidNumberLiteral => ErrorProperties {
                code: "E0027",
                title: "Invalid Number Literal",
                message: None,
                help: Some("Ensure the number literal format is correct.".to_string()),
            },
            Self::MissingStructMembers => ErrorProperties {
                code: "E0028",
                title: "Missing Struct Members",
                message: None,
                help: Some("All struct fields must be initialized.".to_string()),
            },
            Self::MissingEnumMembers => ErrorProperties {
                code: "E0029",
                title: "Missing Enum Members",
                message: None,
                help: Some("All enum variants must be handled.".to_string()),
            },
            Self::MissingTypeExpression => ErrorProperties {
                code: "E0030",
                title: "Missing Type Expression",
                message: None,
                help: Some("A type expression is expected here.".to_string()),
            },
            Self::InvalidModifierCombination { combination: _, reason: _ } => ErrorProperties {
                code: "E0031",
                title: "Invalid Modifier Combination",
                message: None,
                help: Some("These modifiers cannot be used together.".to_string()),
            },
        }
    }
}

impl SyntaxError {
    pub fn new(kind: SyntaxErrorKind, span: Span) -> Self {
        Self { kind, span }
    }

    /// Report the error using the legacy format function.
    /// For new code, prefer using `Reportable::report()` or `to_diagnostic()`.
    pub fn report(&self, source: &str) -> String {
        // Delegate to the new Reportable trait implementation
        Reportable::report(self, source)
    }
}

impl Reportable for SyntaxError {
    fn to_diagnostic(&self) -> Diagnostic {
        let props = self.kind.properties();
        Diagnostic {
            severity: Severity::Error,
            code: Some(props.code),
            title: props.title.to_string(),
            message: props.message.unwrap_or_else(|| props.title.to_string()),
            span: Some(self.span.clone()),
            help: props.help,
            notes: Vec::new(),
        }
    }

    fn report(&self, source: &str) -> String {
        let props = self.kind.properties();
        // Use the legacy format function for backward compatibility with existing tests
        format_diagnostic(
            source,
            &self.span,
            props.title,
            "error",
            props.help.as_deref(),
        )
    }
}

// Helper function to find line number, column, and the line content from a source string and a byte position.
pub fn find_line_info(source: &str, pos: usize) -> (usize, usize, &str) {
    let mut line_start = 0;
    let mut line_num = 1;
    for (i, c) in source.char_indices() {
        if i >= pos {
            break;
        }
        if c == '\n' {
            line_start = i + 1;
            line_num += 1;
        }
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |i| line_start + i);
    let line_str = &source[line_start..line_end];
    let col_num = source[line_start..pos].chars().count() + 1;
    (line_num, col_num, line_str)
}
