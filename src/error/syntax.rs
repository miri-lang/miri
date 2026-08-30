// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable};
use crate::error::foreign_syntax::ForeignForm;

/// Byte offset range in source code, used for error reporting and AST spans.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A syntax error from the lexer or parser, with its source location.
#[derive(Debug, PartialEq, Clone)]
pub struct SyntaxError {
    pub kind: SyntaxErrorKind,
    pub span: Span,
    /// The construct from another language this error recognised, when the
    /// failing token names one.
    pub foreign_form: Option<ForeignForm>,
}

/// All possible syntax error variants produced by the lexer and parser.
#[derive(Debug, PartialEq, Clone)]
pub enum SyntaxErrorKind {
    // Lexer errors
    InvalidToken,
    UnclosedMultilineComment,
    IndentationMismatch,

    // Parser errors
    UnexpectedToken {
        expected: String,
        found: String,
    },
    UnexpectedEOF,

    InvalidTypeDeclaration {
        expected: String,
    },
    InvalidLeftHandSideExpression,
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
    UnsupportedAttributeTarget,
    MissingTypeExpression,

    DuplicateMatchPattern,
    MissingMatchBranches,

    InvalidModifierCombination {
        combination: String,
        reason: String,
    },

    RecursionLimitExceeded,

    /// An unknown runtime name was specified in a runtime function declaration.
    UnknownRuntime {
        name: String,
    },

    /// A constant declaration is missing its required initializer.
    MissingConstantInitializer {
        name: String,
    },

    /// A C-style operator was used instead of the Miri keyword equivalent.
    UnsupportedCStyleOperator {
        found: String,
        suggestion: String,
    },
}

const HELP_INVALID_TOKEN: &str =
    "The character or sequence of characters here is not a valid part of the language.";
const HELP_UNEXPECTED_EOF: &str =
    "The file ended unexpectedly. Check for unclosed blocks or expressions.";
const HELP_RECURSION_LIMIT: &str =
    "The expression or statement is nested too deeply. Simplify your code.";

impl SyntaxErrorKind {
    /// Returns the error code, title, message, and help text for this error kind.
    ///
    /// The match is intentionally a single exhaustive table over all variants.
    /// Splitting it would either require non-exhaustive helper matches or
    /// duplicate variant lists across helpers — both strictly worse than one
    /// flat lookup. Adding a variant fails to compile here, which is the
    /// safety property we want.
    #[allow(clippy::too_many_lines)]
    pub fn properties(&self) -> ErrorProperties {
        use SyntaxErrorKind as K;
        let p = ErrorProperties::simple;
        match self {
            K::InvalidToken => p(DiagnosticCode::LexInvalidToken).with_help(HELP_INVALID_TOKEN),
            K::UnclosedMultilineComment => p(DiagnosticCode::LexUnclosedMultilineComment)
                .with_help("Add '*/' to close the comment."),
            K::IndentationMismatch => p(DiagnosticCode::LexIndentationMismatch)
                .with_help("Ensure the indentation level matches the surrounding code block."),
            K::UnexpectedToken { expected, found } => p(DiagnosticCode::ParUnexpectedToken)
                .with_message(format!("Expected {}, but found {}", expected, found)),
            K::UnexpectedEOF => p(DiagnosticCode::ParUnexpectedEndOfFile)
                .with_message("Unexpected end of file")
                .with_help(HELP_UNEXPECTED_EOF),
            K::InvalidTypeDeclaration { .. } => p(DiagnosticCode::ParInvalidTypeDeclaration)
                .with_help("Types must be declared with a valid identifier."),
            K::InvalidIntegerLiteral => p(DiagnosticCode::ParInvalidIntegerLiteral)
                .with_help("Ensure the integer literal format is correct."),
            K::InvalidBinaryLiteral => p(DiagnosticCode::LexInvalidBinaryLiteral)
                .with_help("Binary literals must start with '0b' followed by 0s and 1s."),
            K::InvalidOctalLiteral => p(DiagnosticCode::LexInvalidOctalLiteral)
                .with_help("Octal literals must start with '0o' followed by digits 0-7."),
            K::InvalidHexLiteral => p(DiagnosticCode::LexInvalidHexLiteral)
                .with_help("Hexadecimal literals must start with '0x' followed by hex digits."),
            K::InvalidFloatLiteral => p(DiagnosticCode::ParInvalidFloatLiteral)
                .with_help("Ensure the float literal format is correct."),
            K::InvalidStringLiteral => p(DiagnosticCode::ParInvalidStringLiteral)
                .with_help("Ensure the string literal is properly quoted and escaped."),
            K::InvalidBooleanLiteral => p(DiagnosticCode::ParInvalidBooleanLiteral)
                .with_help("Boolean literals must be 'true' or 'false'."),
            K::InvalidLeftHandSideExpression => p(DiagnosticCode::ParInvalidLeftHandSide)
                .with_help("The expression on the left side of the assignment is not valid."),
            K::MissingStructMemberType => p(DiagnosticCode::ParMissingStructMemberType)
                .with_help("Struct members must have a type annotation."),
            K::InvalidInheritanceIdentifier => p(DiagnosticCode::ParInvalidInheritanceIdentifier)
                .with_help("Parent type in inheritance must be a valid identifier."),
            K::DuplicateMatchPattern => p(DiagnosticCode::ParDuplicateMatchPattern)
                .with_help("This pattern is already covered in a previous branch."),
            K::MissingMatchBranches => p(DiagnosticCode::ParMissingMatchBranches)
                .with_help("The match expression must cover all possible cases."),
            K::InvalidRegexLiteral => p(DiagnosticCode::LexInvalidRegexLiteral)
                .with_help("Ensure the regex pattern is valid."),
            K::InvalidFormattedString => p(DiagnosticCode::LexInvalidFormattedString)
                .with_help("The format string syntax is incorrect."),
            K::InvalidFormattedStringExpression => {
                p(DiagnosticCode::LexInvalidFormattedStringExpression)
                    .with_help("The expression inside the format string is invalid.")
            }
            K::BackslashInFStringExpression => p(DiagnosticCode::LexBackslashInFormatString)
                .with_help("Backslashes are not allowed in format string expressions."),
            K::InvalidNumberLiteral => p(DiagnosticCode::LexInvalidNumberLiteral)
                .with_help("Ensure the number literal format is correct."),
            K::MissingStructMembers => p(DiagnosticCode::ParMissingStructMembers)
                .with_help("All struct fields must be initialized."),
            K::MissingEnumMembers => p(DiagnosticCode::ParMissingEnumMembers)
                .with_help("All enum variants must be handled."),
            K::UnsupportedAttributeTarget => p(DiagnosticCode::ParUnsupportedAttributeTarget)
                .with_help("Attributes may only precede an enum, function, or class declaration."),
            K::MissingTypeExpression => p(DiagnosticCode::ParMissingTypeExpression)
                .with_help("A type expression is expected here."),
            K::InvalidModifierCombination { .. } => {
                p(DiagnosticCode::ParInvalidModifierCombination)
                    .with_help("These modifiers cannot be used together.")
            }
            K::UnknownRuntime { name } => p(DiagnosticCode::ParUnknownRuntime)
                .with_message(format!("Unknown runtime '{}'", name))
                .with_help("Known runtimes: \"core\"."),
            K::MissingConstantInitializer { name } => {
                p(DiagnosticCode::ParMissingConstantInitializer)
                    .with_message(format!(
                        "Constant '{}' must be initialized with a value",
                        name
                    ))
                    .with_help("Add '= <value>' after the constant name, e.g. 'const X = 1'.")
            }
            K::UnsupportedCStyleOperator { found, suggestion } => {
                p(DiagnosticCode::ParUnsupportedCStyleOperator)
                    .with_message(format!("'{}' is not a valid operator in Miri", found))
                    .with_help(format!("Use '{}' instead of '{}'.", suggestion, found))
            }
            K::RecursionLimitExceeded => {
                p(DiagnosticCode::ParRecursionLimitExceeded).with_help(HELP_RECURSION_LIMIT)
            }
        }
    }
}

impl SyntaxError {
    /// Creates a new syntax error of the given kind at the given span.
    pub fn new(kind: SyntaxErrorKind, span: Span) -> Self {
        Self {
            kind,
            span,
            foreign_form: None,
        }
    }

    /// Records the construct from another language this error recognised.
    ///
    /// The form supplies the help text and, where the rewrite is textual, the
    /// repair. Both are read when the error becomes a diagnostic.
    pub fn with_foreign_form(mut self, form: ForeignForm) -> Self {
        self.foreign_form = Some(form);
        self
    }

    /// Formats this error for terminal display using the given source code.
    pub fn report(&self, source: &str) -> String {
        Reportable::report(self, source)
    }
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let props = self.kind.properties();
        write!(f, "{}", props.message.as_deref().unwrap_or(props.title))
    }
}

impl Reportable for SyntaxError {
    fn to_diagnostic(&self) -> Diagnostic {
        let mut diag = Diagnostic::from_props(self.kind.properties(), Some(self.span), None);
        // A recognised foreign construct names the Miri form that replaces it,
        // which is more use than the token the parser expected. Its help
        // supersedes whatever the error kind carries.
        if let Some(foreign_form) = &self.foreign_form {
            diag.help = Some(foreign_form.help().to_string());
            diag.repair = foreign_form.repair();
        }
        diag
    }
}

/// Finds the line number, column number, and line content for a byte position in source.
pub fn find_line_info(source: &str, pos: usize) -> (usize, usize, &str) {
    let mut line_start = 0;
    let mut line_num = 1;
    let pos = pos.min(source.len()); // Clamp pos to avoid out-of-bounds slicing

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
