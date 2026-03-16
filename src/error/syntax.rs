// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::diagnostic::{Diagnostic, ErrorProperties, Reportable, Severity};

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

impl SyntaxErrorKind {
    /// Returns the error code, title, message, and help text for this error kind.
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
            Self::RecursionLimitExceeded => ErrorProperties {
                code: "E0035",
                title: "Recursion Limit Exceeded",
                message: None,
                help: Some("The expression or statement is nested too deeply. Simplify your code.".to_string()),
            },
            Self::UnknownRuntime { name } => ErrorProperties {
                code: "E0032",
                title: "Unknown Runtime",
                message: Some(format!("Unknown runtime '{}'", name)),
                help: Some("Known runtimes: \"core\".".to_string()),
            },
            Self::MissingConstantInitializer { name } => ErrorProperties {
                code: "E0033",
                title: "Missing Constant Initializer",
                message: Some(format!("Constant '{}' must be initialized with a value", name)),
                help: Some("Add '= <value>' after the constant name, e.g. 'const X = 1'.".to_string()),
            },
            Self::UnsupportedCStyleOperator { found, suggestion } => ErrorProperties {
                code: "E0034",
                title: "Unsupported C-Style Operator",
                message: Some(format!("'{}' is not a valid operator in Miri", found)),
                help: Some(format!("Use '{}' instead of '{}'.", suggestion, found)),
            },
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
        let props = self.kind.properties();

        // If the span points outside the user's source (e.g. from a stdlib module),
        // omit it to avoid panicking in format_diagnostic_full.
        let span = if self.span.start < usize::MAX {
            Some(self.span)
        } else {
            None
        };

        Diagnostic {
            severity: Severity::Error,
            code: Some(props.code),
            title: props.title.to_string(),
            message: props.message.unwrap_or_else(|| props.title.to_string()),
            span,
            help: props.help,
            notes: Vec::new(),
        }
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
