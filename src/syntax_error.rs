// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

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
}

impl SyntaxError {
    pub fn new(kind: SyntaxErrorKind, span: Span) -> Self {
        Self { kind, span }
    }

    pub fn report(&self, source: &str) -> String {
        let start_pos = self.span.start;
        let (line_num, col_num, line_str) = find_line_info(source, start_pos);

        let (error_title, help_message): (&str, String) = match self.kind {
            SyntaxErrorKind::InvalidToken => (
                "Invalid Token",
                "The character or sequence of characters here is not a valid part of the language.".to_string(),
            ),
            SyntaxErrorKind::UnclosedMultilineComment => (
                "Unclosed Multiline Comment",
                "Multiline comments must be closed with '*/'.".to_string(),
            ),
            SyntaxErrorKind::IndentationMismatch => (
                "Indentation Mismatch",
                "This line's indentation does not match any previous level. Check your spaces or tabs.".to_string(),
            ),
            SyntaxErrorKind::UnclosedStringLiteral => (
                "Unclosed String Literal",
                "String literals must be closed with a matching quote.".to_string(),
            ),
            SyntaxErrorKind::UnexpectedToken { ref expected, ref found } => {
                if expected.is_empty() {
                    (
                        "Unexpected Token",
                        format!("The token '{found}' is not expected in this context."),
                    )
                } else {
                    (
                        "Unexpected Token",
                        format!("Expected {expected}, but found '{found}' instead."),
                    )
                }
            },
            SyntaxErrorKind::UnexpectedEOF => (
                "Unexpected End of File",
                "The file ended abruptly. An expression or statement may be incomplete.".to_string(),
            ),
            SyntaxErrorKind::InvalidAssignmentTarget => (
                "Invalid Assignment Target",
                "The left-hand side of an assignment must be a variable or a property.".to_string(),
            ),
            SyntaxErrorKind::IntegerLiteralOverflow => (
                "Integer Literal Overflow",
                "The integer literal exceeds the maximum value for its type.".to_string(),
            ),
            SyntaxErrorKind::InvalidIntegerLiteral => (
                "Invalid Integer Literal",
                "The integer literal is not valid. Ensure it is a valid number without invalid characters.".to_string(),
            ),
            SyntaxErrorKind::InvalidBinaryLiteral => (
                "Invalid Binary Literal",
                "Binary literals must start with '0b' followed by binary digits (0 or 1).".to_string(),
            ),
            SyntaxErrorKind::InvalidOctalLiteral => (
                "Invalid Octal Literal",
                "Octal literals must start with '0o' followed by octal digits (0 to 7).".to_string(),
            ),
            SyntaxErrorKind::InvalidHexLiteral => (
                "Invalid Hexadecimal Literal",
                "Hexadecimal literals must start with '0x' followed by hexadecimal digits (0-9, a-f, A-F).".to_string(),
            ),
            SyntaxErrorKind::InvalidFloatLiteral => (
                "Invalid Float Literal",
                "The float literal is not valid. Ensure it is a valid number with a decimal point and optional exponent.".to_string(),
            ),
            SyntaxErrorKind::InvalidStringLiteral => (
                "Invalid String Literal",
                "String literals must be enclosed in matching quotes (single or double).".to_string(),
            ),
            SyntaxErrorKind::InvalidBooleanLiteral => (
                "Invalid Boolean Literal",
                "Boolean literals must be either 'true' or 'false'.".to_string(),
            ),
            SyntaxErrorKind::UnexpectedOperator { ref found, ref expected } => (
                "Unexpected Operator",
                format!("The operator '{found}' is not supported. Expected one of: {expected}."),
            ),
            SyntaxErrorKind::InvalidLeftHandSideExpression => (
                "Invalid Left-Hand Side Expression",
                "The left-hand side expression is not valid. Ensure it is a valid identifier or property.".to_string(),
            ),
            SyntaxErrorKind::InvalidTypeDeclaration { ref expected } => (
                "Invalid Type Declaration",
                format!("The type expression is not correct. Expected: {expected}."),
            ),
            SyntaxErrorKind::MissingStructMemberType => (
                "Missing Struct Member Type",
                "Struct members must have a type declaration.".to_string(),
            ),
            SyntaxErrorKind::InvalidInheritanceIdentifier => (
                "Invalid Inheritance Identifier",
                "The inheritance identifier is not valid. You can only extend, implement or include a class, imported via the `use` statement. Class members can't be used as inheritance identifiers.".to_string(),
            ),
            SyntaxErrorKind::DuplicateMatchPattern => (
                "Duplicate Match Pattern",
                "This pattern is a duplicate of a previous pattern in the same match expression.".to_string(),
            ),
            SyntaxErrorKind::MissingMatchBranches => (
                "Missing Match Branches",
                "Match expressions must have at least one branch.".to_string(),
            ),
            SyntaxErrorKind::InvalidRegexLiteral => (
                "Invalid Regex Literal",
                "Regex literals must be enclosed in matching quote characters (e.g., re\".../\" or re'...').".to_string(),
            ),
            SyntaxErrorKind::InvalidFormattedString => (
                "Invalid Formatted String",
                "Formatted strings must be enclosed in matching quote characters (e.g., f\"...\" or f'...').".to_string(),
            ),
            SyntaxErrorKind::InvalidFormattedStringExpression => (
                "Invalid Formatted String Expression",
                "The formatted string is malformed, likely due to an unclosed expression brace `{`.".to_string(),
            ),
            SyntaxErrorKind::BackslashInFStringExpression => (
                "Invalid Backslash",
                "The expression part of a formatted string cannot contain backslashes.".to_string(),
            ),
            SyntaxErrorKind::InvalidNumberLiteral => (
                "Invalid Number Literal",
                "Number literals must be valid integers or floats, which cannot begin or end with underscores.".to_string(),
            ),
            SyntaxErrorKind::MissingStructMembers => (
                "Missing Struct Members",
                "Structs must have at least one member.".to_string(),
            ),
            SyntaxErrorKind::MissingEnumMembers => (
                "Missing Enum Members",
                "Enums must have at least one member.".to_string(),
            ),
            SyntaxErrorKind::MissingTypeExpression => (
                "Missing Type Expression",
                "Type expression is required but not provided.".to_string(),
            ),
        };

        let underline = "^".repeat(self.span.end - self.span.start);

        // Syntax Error: Invalid Token
        //   --> line 5:10
        // 5 | let x = 10 % 2;
        //   |           ^
        //   = help: The '%' character is not a valid operator here.
        format!(
            "Syntax Error: {error_title}\n\
              --> line {line_num}:{col_num}\n\
               |\n\
               | {line_str}\n\
               | {padding}{underline}\n\
               = help: {help_message}",
            padding = " ".repeat(col_num - 1)
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
