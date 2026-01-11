// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::error::codes;
use crate::error::diagnostic::{Diagnostic, Reportable, Severity};
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
    /// Get the error code for this syntax error kind.
    pub fn code(&self) -> &'static str {
        match self {
            SyntaxErrorKind::InvalidToken => codes::syntax::INVALID_TOKEN,
            SyntaxErrorKind::UnclosedMultilineComment => codes::syntax::UNCLOSED_MULTILINE_COMMENT,
            SyntaxErrorKind::IndentationMismatch => codes::syntax::INDENTATION_MISMATCH,
            SyntaxErrorKind::UnclosedStringLiteral => codes::syntax::UNCLOSED_STRING_LITERAL,
            SyntaxErrorKind::UnexpectedToken { .. } => codes::syntax::UNEXPECTED_TOKEN,
            SyntaxErrorKind::UnexpectedOperator { .. } => codes::syntax::UNEXPECTED_OPERATOR,
            SyntaxErrorKind::UnexpectedEOF => codes::syntax::UNEXPECTED_EOF,
            SyntaxErrorKind::InvalidTypeDeclaration { .. } => {
                codes::syntax::INVALID_TYPE_DECLARATION
            }
            SyntaxErrorKind::InvalidLeftHandSideExpression => codes::syntax::INVALID_LHS_EXPRESSION,
            SyntaxErrorKind::InvalidAssignmentTarget => codes::syntax::INVALID_ASSIGNMENT_TARGET,
            SyntaxErrorKind::IntegerLiteralOverflow => codes::syntax::INTEGER_OVERFLOW,
            SyntaxErrorKind::InvalidNumberLiteral => codes::syntax::INVALID_NUMBER_LITERAL,
            SyntaxErrorKind::InvalidIntegerLiteral => codes::syntax::INVALID_INTEGER_LITERAL,
            SyntaxErrorKind::InvalidBinaryLiteral => codes::syntax::INVALID_BINARY_LITERAL,
            SyntaxErrorKind::InvalidOctalLiteral => codes::syntax::INVALID_OCTAL_LITERAL,
            SyntaxErrorKind::InvalidHexLiteral => codes::syntax::INVALID_HEX_LITERAL,
            SyntaxErrorKind::InvalidFloatLiteral => codes::syntax::INVALID_FLOAT_LITERAL,
            SyntaxErrorKind::InvalidStringLiteral => codes::syntax::INVALID_STRING_LITERAL,
            SyntaxErrorKind::InvalidBooleanLiteral => codes::syntax::INVALID_BOOLEAN_LITERAL,
            SyntaxErrorKind::InvalidInheritanceIdentifier => {
                codes::syntax::INVALID_INHERITANCE_IDENTIFIER
            }
            SyntaxErrorKind::InvalidRegexLiteral => codes::syntax::INVALID_REGEX_LITERAL,
            SyntaxErrorKind::InvalidFormattedString => codes::syntax::INVALID_FORMATTED_STRING,
            SyntaxErrorKind::InvalidFormattedStringExpression => {
                codes::syntax::INVALID_FORMATTED_STRING_EXPR
            }
            SyntaxErrorKind::BackslashInFStringExpression => codes::syntax::BACKSLASH_IN_FSTRING,
            SyntaxErrorKind::MissingStructMemberType => codes::syntax::MISSING_STRUCT_MEMBER_TYPE,
            SyntaxErrorKind::MissingStructMembers => codes::syntax::MISSING_STRUCT_MEMBERS,
            SyntaxErrorKind::MissingEnumMembers => codes::syntax::MISSING_ENUM_MEMBERS,
            SyntaxErrorKind::MissingTypeExpression => codes::syntax::MISSING_TYPE_EXPRESSION,
            SyntaxErrorKind::DuplicateMatchPattern => codes::syntax::DUPLICATE_MATCH_PATTERN,
            SyntaxErrorKind::MissingMatchBranches => codes::syntax::MISSING_MATCH_BRANCHES,
            SyntaxErrorKind::InvalidModifierCombination { .. } => {
                codes::syntax::INVALID_MODIFIER_COMBINATION
            }
        }
    }

    /// Get the human-readable title for this error.
    fn title(&self) -> &'static str {
        match self {
            SyntaxErrorKind::InvalidToken => "Invalid Token",
            SyntaxErrorKind::UnclosedMultilineComment => "Unclosed Multiline Comment",
            SyntaxErrorKind::IndentationMismatch => "Indentation Mismatch",
            SyntaxErrorKind::UnclosedStringLiteral => "Unclosed String Literal",
            SyntaxErrorKind::UnexpectedToken { .. } => "Unexpected Token",
            SyntaxErrorKind::UnexpectedOperator { .. } => "Unexpected Operator",
            SyntaxErrorKind::UnexpectedEOF => "Unexpected End of File",
            SyntaxErrorKind::InvalidTypeDeclaration { .. } => "Invalid Type Declaration",
            SyntaxErrorKind::InvalidLeftHandSideExpression => "Invalid Left-Hand Side Expression",
            SyntaxErrorKind::InvalidAssignmentTarget => "Invalid Assignment Target",
            SyntaxErrorKind::IntegerLiteralOverflow => "Integer Literal Overflow",
            SyntaxErrorKind::InvalidNumberLiteral => "Invalid Number Literal",
            SyntaxErrorKind::InvalidIntegerLiteral => "Invalid Integer Literal",
            SyntaxErrorKind::InvalidBinaryLiteral => "Invalid Binary Literal",
            SyntaxErrorKind::InvalidOctalLiteral => "Invalid Octal Literal",
            SyntaxErrorKind::InvalidHexLiteral => "Invalid Hexadecimal Literal",
            SyntaxErrorKind::InvalidFloatLiteral => "Invalid Float Literal",
            SyntaxErrorKind::InvalidStringLiteral => "Invalid String Literal",
            SyntaxErrorKind::InvalidBooleanLiteral => "Invalid Boolean Literal",
            SyntaxErrorKind::InvalidInheritanceIdentifier => "Invalid Inheritance Identifier",
            SyntaxErrorKind::InvalidRegexLiteral => "Invalid Regex Literal",
            SyntaxErrorKind::InvalidFormattedString => "Invalid Formatted String",
            SyntaxErrorKind::InvalidFormattedStringExpression => {
                "Invalid Formatted String Expression"
            }
            SyntaxErrorKind::BackslashInFStringExpression => "Invalid Backslash",
            SyntaxErrorKind::MissingStructMemberType => "Missing Struct Member Type",
            SyntaxErrorKind::MissingStructMembers => "Missing Struct Members",
            SyntaxErrorKind::MissingEnumMembers => "Missing Enum Members",
            SyntaxErrorKind::MissingTypeExpression => "Missing Type Expression",
            SyntaxErrorKind::DuplicateMatchPattern => "Duplicate Match Pattern",
            SyntaxErrorKind::MissingMatchBranches => "Missing Match Branches",
            SyntaxErrorKind::InvalidModifierCombination { .. } => "Invalid Modifier Combination",
        }
    }

    /// Get the help message for this error.
    fn help(&self) -> String {
        match self {
            SyntaxErrorKind::InvalidToken => {
                "The character or sequence of characters here is not a valid part of the language.".to_string()
            }
            SyntaxErrorKind::UnclosedMultilineComment => {
                "Multiline comments must be closed with '*/'.".to_string()
            }
            SyntaxErrorKind::IndentationMismatch => {
                "This line's indentation does not match any previous level. Check your spaces or tabs.".to_string()
            }
            SyntaxErrorKind::UnclosedStringLiteral => {
                "String literals must be closed with a matching quote.".to_string()
            }
            SyntaxErrorKind::UnexpectedToken { expected, found } => {
                if expected.is_empty() {
                    format!("The token '{found}' is not expected in this context.")
                } else {
                    format!("Expected {expected}, but found '{found}' instead.")
                }
            }
            SyntaxErrorKind::UnexpectedOperator { found, expected } => {
                format!("The operator '{found}' is not supported. Expected one of: {expected}.")
            }
            SyntaxErrorKind::UnexpectedEOF => {
                "The file ended abruptly. An expression or statement may be incomplete.".to_string()
            }
            SyntaxErrorKind::InvalidTypeDeclaration { expected } => {
                format!("The type expression is not correct. Expected: {expected}.")
            }
            SyntaxErrorKind::InvalidLeftHandSideExpression => {
                "The left-hand side expression is not valid. Ensure it is a valid identifier or property.".to_string()
            }
            SyntaxErrorKind::InvalidAssignmentTarget => {
                "The left-hand side of an assignment must be a variable or a property.".to_string()
            }
            SyntaxErrorKind::IntegerLiteralOverflow => {
                "The integer literal exceeds the maximum value for its type.".to_string()
            }
            SyntaxErrorKind::InvalidNumberLiteral => {
                "Number literals must be valid integers or floats, which cannot begin or end with underscores.".to_string()
            }
            SyntaxErrorKind::InvalidIntegerLiteral => {
                "The integer literal is not valid. Ensure it is a valid number without invalid characters.".to_string()
            }
            SyntaxErrorKind::InvalidBinaryLiteral => {
                "Binary literals must start with '0b' followed by binary digits (0 or 1).".to_string()
            }
            SyntaxErrorKind::InvalidOctalLiteral => {
                "Octal literals must start with '0o' followed by octal digits (0 to 7).".to_string()
            }
            SyntaxErrorKind::InvalidHexLiteral => {
                "Hexadecimal literals must start with '0x' followed by hexadecimal digits (0-9, a-f, A-F).".to_string()
            }
            SyntaxErrorKind::InvalidFloatLiteral => {
                "The float literal is not valid. Ensure it is a valid number with a decimal point and optional exponent.".to_string()
            }
            SyntaxErrorKind::InvalidStringLiteral => {
                "String literals must be enclosed in matching quotes (single or double).".to_string()
            }
            SyntaxErrorKind::InvalidBooleanLiteral => {
                "Boolean literals must be either 'true' or 'false'.".to_string()
            }
            SyntaxErrorKind::InvalidInheritanceIdentifier => {
                "The inheritance identifier is not valid. You can only extend, implement or include a class, imported via the `use` statement.".to_string()
            }
            SyntaxErrorKind::InvalidRegexLiteral => {
                "Regex literals must be enclosed in matching quote characters (e.g., re\".../\" or re'...').".to_string()
            }
            SyntaxErrorKind::InvalidFormattedString => {
                "Formatted strings must be enclosed in matching quote characters (e.g., f\"...\" or f'...').".to_string()
            }
            SyntaxErrorKind::InvalidFormattedStringExpression => {
                "The formatted string is malformed, likely due to an unclosed expression brace `{`.".to_string()
            }
            SyntaxErrorKind::BackslashInFStringExpression => {
                "The expression part of a formatted string cannot contain backslashes.".to_string()
            }
            SyntaxErrorKind::MissingStructMemberType => {
                "Struct members must have a type declaration.".to_string()
            }
            SyntaxErrorKind::MissingStructMembers => {
                "Structs must have at least one member.".to_string()
            }
            SyntaxErrorKind::MissingEnumMembers => {
                "Enums must have at least one member.".to_string()
            }
            SyntaxErrorKind::MissingTypeExpression => {
                "Type expression is required but not provided.".to_string()
            }
            SyntaxErrorKind::DuplicateMatchPattern => {
                "This pattern is a duplicate of a previous pattern in the same match expression.".to_string()
            }
            SyntaxErrorKind::MissingMatchBranches => {
                "Match expressions must have at least one branch.".to_string()
            }
            SyntaxErrorKind::InvalidModifierCombination { combination, reason } => {
                format!("The modifiers '{combination}' cannot be used together. {reason}")
            }
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
        Diagnostic {
            severity: Severity::Error,
            code: Some(self.kind.code()),
            title: self.kind.title().to_string(),
            message: self.kind.title().to_string(),
            span: Some(self.span.clone()),
            help: Some(self.kind.help()),
            notes: Vec::new(),
        }
    }

    fn report(&self, source: &str) -> String {
        // Use the legacy format function for backward compatibility with existing tests
        format_diagnostic(
            source,
            &self.span,
            self.kind.title(),
            "error",
            Some(&self.kind.help()),
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
