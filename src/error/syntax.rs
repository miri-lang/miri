// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable};

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
}

/// All possible syntax error variants produced by the lexer and parser.
#[derive(Debug, PartialEq, Clone)]
pub enum SyntaxErrorKind {
    // Lexer errors
    InvalidToken,
    UnclosedMultilineComment,
    IndentationMismatch,
    UnclosedStringLiteral,

    // Parser errors
    UnexpectedToken {
        expected: String,
        found: String,
    },
    UnexpectedOperator {
        expected: String,
        found: String,
    },
    UnexpectedEOF,

    InvalidTypeDeclaration {
        expected: String,
    },
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
    /// Splitting it would either require non-exhaustive helper matches (banned
    /// by PRINCIPLES §3.7) or duplicate variant lists across helpers — both
    /// strictly worse than one flat lookup. Adding a variant fails to compile
    /// here, which is the safety property we want.
    #[allow(clippy::too_many_lines)]
    pub fn properties(&self) -> ErrorProperties {
        use SyntaxErrorKind as K;
        let p = ErrorProperties::simple;
        match self {
            K::InvalidToken => p("E0001", "Invalid Token").with_help(HELP_INVALID_TOKEN),
            K::UnclosedMultilineComment => {
                p("E0002", "Unclosed Multiline Comment").with_help("Add '*/' to close the comment.")
            }
            K::IndentationMismatch => p("E0003", "Indentation Mismatch")
                .with_help("Ensure the indentation level matches the surrounding code block."),
            K::UnclosedStringLiteral => p("E0004", "Unclosed String Literal")
                .with_help("Add a closing quote to the string literal."),
            K::UnexpectedToken { expected, found } => p("E0005", "Unexpected Token")
                .with_message(format!("Expected {}, but found {}", expected, found)),
            K::UnexpectedEOF => p("E0006", "Unexpected End of File")
                .with_message("Unexpected end of file")
                .with_help(HELP_UNEXPECTED_EOF),
            K::InvalidTypeDeclaration { .. } => p("E0007", "Invalid Type Declaration")
                .with_help("Types must be declared with a valid identifier."),
            K::InvalidAssignmentTarget => p("E0008", "Invalid Assignment Target")
                .with_help("You can only assign values to variables or mutable properties."),
            K::IntegerLiteralOverflow => p("E0009", "Integer Overflow")
                .with_help("The integer literal is too large for the target type."),
            K::InvalidIntegerLiteral => p("E0010", "Invalid Integer Literal")
                .with_help("Ensure the integer literal format is correct."),
            K::InvalidBinaryLiteral => p("E0011", "Invalid Binary Literal")
                .with_help("Binary literals must start with '0b' followed by 0s and 1s."),
            K::InvalidOctalLiteral => p("E0012", "Invalid Octal Literal")
                .with_help("Octal literals must start with '0o' followed by digits 0-7."),
            K::InvalidHexLiteral => p("E0013", "Invalid Hex Literal")
                .with_help("Hexadecimal literals must start with '0x' followed by hex digits."),
            K::InvalidFloatLiteral => p("E0014", "Invalid Float Literal")
                .with_help("Ensure the float literal format is correct."),
            K::InvalidStringLiteral => p("E0015", "Invalid String Literal")
                .with_help("Ensure the string literal is properly quoted and escaped."),
            K::InvalidBooleanLiteral => p("E0016", "Invalid Boolean Literal")
                .with_help("Boolean literals must be 'true' or 'false'."),
            K::UnexpectedOperator { .. } => p("E0017", "Unexpected Operator")
                .with_help("This operator cannot be used in this context."),
            K::InvalidLeftHandSideExpression => p("E0018", "Invalid Left-Hand Side Expression")
                .with_help("The expression on the left side of the assignment is not valid."),
            K::MissingStructMemberType => p("E0019", "Missing Struct Member Type")
                .with_help("Struct members must have a type annotation."),
            K::InvalidInheritanceIdentifier => p("E0020", "Invalid Inheritance Identifier")
                .with_help("Parent type in inheritance must be a valid identifier."),
            K::DuplicateMatchPattern => p("E0021", "Duplicate Match Pattern")
                .with_help("This pattern is already covered in a previous branch."),
            K::MissingMatchBranches => p("E0022", "Missing Match Branches")
                .with_help("The match expression must cover all possible cases."),
            K::InvalidRegexLiteral => {
                p("E0023", "Invalid Regex Literal").with_help("Ensure the regex pattern is valid.")
            }
            K::InvalidFormattedString => p("E0024", "Invalid Formatted String")
                .with_help("The format string syntax is incorrect."),
            K::InvalidFormattedStringExpression => {
                p("E0025", "Invalid Formatted String Expression")
                    .with_help("The expression inside the format string is invalid.")
            }
            K::BackslashInFStringExpression => p("E0026", "Backslash in Format String")
                .with_help("Backslashes are not allowed in format string expressions."),
            K::InvalidNumberLiteral => p("E0027", "Invalid Number Literal")
                .with_help("Ensure the number literal format is correct."),
            K::MissingStructMembers => p("E0028", "Missing Struct Members")
                .with_help("All struct fields must be initialized."),
            K::MissingEnumMembers => {
                p("E0029", "Missing Enum Members").with_help("All enum variants must be handled.")
            }
            K::MissingTypeExpression => p("E0030", "Missing Type Expression")
                .with_help("A type expression is expected here."),
            K::InvalidModifierCombination { .. } => p("E0031", "Invalid Modifier Combination")
                .with_help("These modifiers cannot be used together."),
            K::UnknownRuntime { name } => p("E0032", "Unknown Runtime")
                .with_message(format!("Unknown runtime '{}'", name))
                .with_help("Known runtimes: \"core\"."),
            K::MissingConstantInitializer { name } => p("E0033", "Missing Constant Initializer")
                .with_message(format!(
                    "Constant '{}' must be initialized with a value",
                    name
                ))
                .with_help("Add '= <value>' after the constant name, e.g. 'const X = 1'."),
            K::UnsupportedCStyleOperator { found, suggestion } => {
                p("E0034", "Unsupported C-Style Operator")
                    .with_message(format!("'{}' is not a valid operator in Miri", found))
                    .with_help(format!("Use '{}' instead of '{}'.", suggestion, found))
            }
            K::RecursionLimitExceeded => {
                p("E0035", "Recursion Limit Exceeded").with_help(HELP_RECURSION_LIMIT)
            }
        }
    }
}

impl SyntaxError {
    /// Creates a new syntax error of the given kind at the given span.
    pub fn new(kind: SyntaxErrorKind, span: Span) -> Self {
        Self { kind, span }
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
        Diagnostic::from_props(self.kind.properties(), Some(self.span), None)
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
