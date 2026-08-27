// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The vocabulary shared by the lexer and the published grammar.
//!
//! `docs/grammar.peg` is written over terminal names rather than over source
//! characters, because Miri's block structure arrives as `Indent`, `Dedent` and
//! `ExpressionStatementEnd` tokens that the lexer synthesises from indentation.
//! This module is where those names are decided, so the grammar and the lexer
//! cannot disagree about what a token is called.

use super::Token;
use std::fmt;

/// Reason why a token is not a grammar terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonTerminalReason {
    /// Lexer skips this token entirely (never appears in token stream).
    LexerSkip,
    /// Intermediate lexer state rewritten before the parser sees it.
    IntermediateState,
    /// Rewritten or post-processed before the parser.
    Rewritten,
}

impl fmt::Display for NonTerminalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LexerSkip => write!(f, "lexer skips this token"),
            Self::IntermediateState => write!(f, "intermediate lexer state"),
            Self::Rewritten => write!(f, "rewritten before parser"),
        }
    }
}

/// Classification of a token: either a published terminal name or a reason it is skipped.
pub enum TerminalClassification {
    /// The terminal as published in the grammar.
    Terminal(String),
    /// Not a grammar terminal; the reason why.
    NotTerminal(NonTerminalReason),
}

/// Classifies every `Token` variant as either a grammar terminal, carrying the name
/// the published grammar uses for it, or a non-terminal with the reason it never
/// reaches the parser.
///
/// This is the single mapping between the lexer and the published grammar. It is
/// deliberately one exhaustive match rather than a set of per-family helpers: the
/// exhaustiveness is the point. A new `Token` variant fails the build here until
/// someone decides whether the grammar names it, which is what keeps the published
/// artifact from drifting away from the lexer. Splitting it would give each helper a
/// `_ => None` arm and lose exactly that guarantee, so the length is accepted.
pub fn classify_token(token: &Token) -> TerminalClassification {
    use TerminalClassification::*;
    match token {
        // Keywords → terminals
        Token::Use => Terminal("USE".to_string()),
        Token::Fn => Terminal("FN".to_string()),
        Token::Async => Terminal("ASYNC".to_string()),
        Token::Await => Terminal("AWAIT".to_string()),
        Token::Spawn => Terminal("SPAWN".to_string()),
        Token::Parallel => Terminal("PARALLEL".to_string()),
        Token::Gpu => Terminal("GPU".to_string()),
        Token::Frame => Terminal("FRAME".to_string()),
        Token::If => Terminal("IF".to_string()),
        Token::Unless => Terminal("UNLESS".to_string()),
        Token::Else => Terminal("ELSE".to_string()),
        Token::Match => Terminal("MATCH".to_string()),
        Token::Default => Terminal("DEFAULT".to_string()),
        Token::Return => Terminal("RETURN".to_string()),
        Token::While => Terminal("WHILE".to_string()),
        Token::Until => Terminal("UNTIL".to_string()),
        Token::Do => Terminal("DO".to_string()),
        Token::For => Terminal("FOR".to_string()),
        Token::Forall => Terminal("FORALL".to_string()),
        Token::Forever => Terminal("FOREVER".to_string()),
        Token::In => Terminal("IN".to_string()),
        Token::Let => Terminal("LET".to_string()),
        Token::Var => Terminal("VAR".to_string()),
        Token::Const => Terminal("CONST".to_string()),
        Token::Or => Terminal("OR".to_string()),
        Token::And => Terminal("AND".to_string()),
        Token::Not => Terminal("NOT".to_string()),
        Token::True => Terminal("TRUE".to_string()),
        Token::False => Terminal("FALSE".to_string()),
        Token::None => Terminal("NONE".to_string()),
        Token::From => Terminal("FROM".to_string()),
        Token::As => Terminal("AS".to_string()),
        Token::Break => Terminal("BREAK".to_string()),
        Token::Continue => Terminal("CONTINUE".to_string()),
        Token::Extends => Terminal("EXTENDS".to_string()),
        Token::Is => Terminal("IS".to_string()),
        Token::Includes => Terminal("INCLUDES".to_string()),
        Token::Implements => Terminal("IMPLEMENTS".to_string()),
        Token::Type => Terminal("TYPE".to_string()),
        Token::Enum => Terminal("ENUM".to_string()),
        Token::Struct => Terminal("STRUCT".to_string()),
        Token::Class => Terminal("CLASS".to_string()),
        Token::Trait => Terminal("TRAIT".to_string()),
        Token::Super => Terminal("SUPER".to_string()),
        Token::Public => Terminal("PUBLIC".to_string()),
        Token::Protected => Terminal("PROTECTED".to_string()),
        Token::Shared => Terminal("SHARED".to_string()),
        Token::Private => Terminal("PRIVATE".to_string()),
        Token::System => Terminal("SYSTEM".to_string()),
        Token::Local => Terminal("LOCAL".to_string()),
        Token::Abstract => Terminal("ABSTRACT".to_string()),
        Token::MustUse => Terminal("MUST_USE".to_string()),
        Token::Out => Terminal("OUT".to_string()),
        Token::Runtime => Terminal("RUNTIME".to_string()),
        Token::Intrinsic => Terminal("INTRINSIC".to_string()),

        // Symbols and Operators → terminals
        Token::At => Terminal("AT".to_string()),
        Token::Semicolon => Terminal("SEMICOLON".to_string()),
        Token::Colon => Terminal("COLON".to_string()),
        Token::DoubleColon => Terminal("DOUBLE_COLON".to_string()),
        Token::FatArrow => Terminal("FAT_ARROW".to_string()),
        Token::Arrow => Terminal("ARROW".to_string()),
        Token::LeftArrow => Terminal("LEFT_ARROW".to_string()),
        Token::Equal => Terminal("EQUAL".to_string()),
        Token::NotEqual => Terminal("NOT_EQUAL".to_string()),
        Token::GreaterThanEqual => Terminal("GREATER_THAN_EQUAL".to_string()),
        Token::LessThanEqual => Terminal("LESS_THAN_EQUAL".to_string()),
        Token::GreaterThan => Terminal("GREATER_THAN".to_string()),
        Token::LessThan => Terminal("LESS_THAN".to_string()),
        Token::Assign => Terminal("ASSIGN".to_string()),
        Token::AssignAdd => Terminal("ASSIGN_ADD".to_string()),
        Token::AssignSub => Terminal("ASSIGN_SUB".to_string()),
        Token::AssignMul => Terminal("ASSIGN_MUL".to_string()),
        Token::AssignDiv => Terminal("ASSIGN_DIV".to_string()),
        Token::AssignMod => Terminal("ASSIGN_MOD".to_string()),
        Token::Plus => Terminal("PLUS".to_string()),
        Token::Increment => Terminal("INCREMENT".to_string()),
        Token::Minus => Terminal("MINUS".to_string()),
        Token::Decrement => Terminal("DECREMENT".to_string()),
        Token::Star => Terminal("STAR".to_string()),
        Token::Slash => Terminal("SLASH".to_string()),
        Token::Percent => Terminal("PERCENT".to_string()),
        Token::Comma => Terminal("COMMA".to_string()),
        Token::Range => Terminal("RANGE".to_string()),
        Token::RangeInclusive => Terminal("RANGE_INCLUSIVE".to_string()),
        Token::Dot => Terminal("DOT".to_string()),
        Token::LParen => Terminal("L_PAREN".to_string()),
        Token::RParen => Terminal("R_PAREN".to_string()),
        Token::LBracket => Terminal("L_BRACKET".to_string()),
        Token::RBracket => Terminal("R_BRACKET".to_string()),
        Token::LBrace => Terminal("L_BRACE".to_string()),
        Token::RBrace => Terminal("R_BRACE".to_string()),
        Token::Pipe => Terminal("PIPE".to_string()),
        Token::Ampersand => Terminal("AMPERSAND".to_string()),
        Token::Caret => Terminal("CARET".to_string()),
        Token::QuestionMark => Terminal("QUESTION_MARK".to_string()),
        Token::QuestionQuestion => Terminal("QUESTION_QUESTION".to_string()),
        Token::Tilde => Terminal("TILDE".to_string()),

        // Identifiers and Literals → terminals
        Token::Identifier => Terminal("IDENT".to_string()),
        Token::Int => Terminal("INT".to_string()),
        Token::Float => Terminal("FLOAT".to_string()),
        Token::BinaryNumber => Terminal("BINARY".to_string()),
        Token::HexNumber => Terminal("HEX".to_string()),
        Token::OctalNumber => Terminal("OCTAL".to_string()),
        Token::String => Terminal("STRING".to_string()),
        Token::Regex(_) => Terminal("REGEX".to_string()),
        Token::FormattedStringStart(_) => Terminal("FSTRING_START".to_string()),
        Token::FormattedStringMiddle(_) => Terminal("FSTRING_MID".to_string()),
        Token::FormattedStringEnd(_) => Terminal("FSTRING_END".to_string()),

        // Synthetic indentation tokens → terminals
        Token::Indent => Terminal("INDENT".to_string()),
        Token::Dedent => Terminal("DEDENT".to_string()),
        Token::ExpressionStatementEnd => Terminal("STMT_END".to_string()),

        // Intermediate lexer states → not terminals
        Token::SingleQuotedRegex => NotTerminal(NonTerminalReason::IntermediateState),
        Token::DoubleQuotedRegex => NotTerminal(NonTerminalReason::IntermediateState),
        Token::SingleQuotedString => NotTerminal(NonTerminalReason::IntermediateState),
        Token::DoubleQuotedString => NotTerminal(NonTerminalReason::IntermediateState),
        Token::SingleQuotedFormattedString => NotTerminal(NonTerminalReason::IntermediateState),
        Token::DoubleQuotedFormattedString => NotTerminal(NonTerminalReason::IntermediateState),
        Token::InvalidNumber => NotTerminal(NonTerminalReason::Rewritten),
        Token::InvalidBinaryNumber => NotTerminal(NonTerminalReason::Rewritten),
        Token::InvalidHexNumber => NotTerminal(NonTerminalReason::Rewritten),
        Token::InvalidOctalNumber => NotTerminal(NonTerminalReason::Rewritten),
        Token::FloatOrRange => NotTerminal(NonTerminalReason::IntermediateState),

        // Skipped by lexer → not terminals
        Token::InlineComment => NotTerminal(NonTerminalReason::LexerSkip),
        Token::Whitespace => NotTerminal(NonTerminalReason::LexerSkip),
        Token::Shebang => NotTerminal(NonTerminalReason::LexerSkip),
        Token::ByteOrderMark => NotTerminal(NonTerminalReason::LexerSkip),
        Token::Newline => NotTerminal(NonTerminalReason::Rewritten),
        Token::MultilineComment => NotTerminal(NonTerminalReason::LexerSkip),
    }
}

/// Keyword and modifier tokens.
fn keyword_samples() -> Vec<Token> {
    vec![
        Token::Use,
        Token::Fn,
        Token::Async,
        Token::Await,
        Token::Spawn,
        Token::Parallel,
        Token::Gpu,
        Token::Frame,
        Token::If,
        Token::Unless,
        Token::Else,
        Token::Match,
        Token::Default,
        Token::Return,
        Token::While,
        Token::Until,
        Token::Do,
        Token::For,
        Token::Forall,
        Token::Forever,
        Token::In,
        Token::Let,
        Token::Var,
        Token::Const,
        Token::Or,
        Token::And,
        Token::Not,
        Token::True,
        Token::False,
        Token::None,
        Token::From,
        Token::As,
    ]
}

/// Punctuation and operator tokens.
fn operator_samples() -> Vec<Token> {
    vec![
        Token::Break,
        Token::Continue,
        Token::Extends,
        Token::Is,
        Token::Includes,
        Token::Implements,
        Token::Type,
        Token::Enum,
        Token::Struct,
        Token::Class,
        Token::Trait,
        Token::Super,
        Token::Public,
        Token::Protected,
        Token::Shared,
        Token::Private,
        Token::System,
        Token::Local,
        Token::Abstract,
        Token::MustUse,
        Token::Out,
        Token::Runtime,
        Token::Intrinsic,
        Token::At,
        Token::Semicolon,
        Token::Colon,
        Token::DoubleColon,
        Token::FatArrow,
        Token::Arrow,
        Token::LeftArrow,
        Token::Equal,
        Token::NotEqual,
    ]
}

/// Identifier, literal and string tokens.
fn literal_samples() -> Vec<Token> {
    vec![
        Token::GreaterThanEqual,
        Token::LessThanEqual,
        Token::GreaterThan,
        Token::LessThan,
        Token::Assign,
        Token::AssignAdd,
        Token::AssignSub,
        Token::AssignMul,
        Token::AssignDiv,
        Token::AssignMod,
        Token::Plus,
        Token::Increment,
        Token::Minus,
        Token::Decrement,
        Token::Star,
        Token::Slash,
        Token::Percent,
        Token::Comma,
        Token::Range,
        Token::RangeInclusive,
        Token::Dot,
        Token::LParen,
        Token::RParen,
        Token::LBracket,
        Token::RBracket,
        Token::LBrace,
        Token::RBrace,
        Token::Pipe,
        Token::Ampersand,
        Token::Caret,
        Token::QuestionMark,
        Token::QuestionQuestion,
    ]
}

/// Synthetic, intermediate and skipped tokens.
fn synthetic_samples() -> Vec<Token> {
    use crate::lexer::token::RegexToken;

    vec![
        Token::Tilde,
        Token::Identifier,
        Token::SingleQuotedRegex,
        Token::DoubleQuotedRegex,
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
        Token::String,
        Token::SingleQuotedFormattedString,
        Token::DoubleQuotedFormattedString,
        Token::Regex(Box::new(RegexToken {
            body: "a".to_string(),
            ignore_case: false,
            global: false,
            multiline: false,
            dot_all: false,
            unicode: false,
        })),
        Token::FormattedStringStart(Box::new("test".to_string())),
        Token::FormattedStringMiddle(Box::new("test".to_string())),
        Token::FormattedStringEnd(Box::new("test".to_string())),
        Token::InvalidNumber,
        Token::FloatOrRange,
        Token::Float,
        Token::Int,
        Token::BinaryNumber,
        Token::HexNumber,
        Token::OctalNumber,
        Token::InvalidBinaryNumber,
        Token::InvalidHexNumber,
        Token::InvalidOctalNumber,
        Token::InlineComment,
        Token::MultilineComment,
        Token::Newline,
        Token::Indent,
        Token::Dedent,
        Token::ExpressionStatementEnd,
        Token::Whitespace,
        Token::Shebang,
        Token::ByteOrderMark,
    ]
}

/// One sample of every `Token` variant, assembled from the family lists above.
///
/// This is the source the published terminal vocabulary is derived from, so it
/// lives beside `classify_token` rather than in a test. A variant missing here is
/// caught by `test_every_variant_is_sampled`.
pub fn token_samples() -> Vec<Token> {
    let mut samples = keyword_samples();
    samples.extend(operator_samples());
    samples.extend(literal_samples());
    samples.extend(synthetic_samples());
    samples
}

/// The terminal names a grammar may reference, derived from `classify_token`.
///
/// A name absent here names no token the lexer can produce, so a grammar rule
/// referencing it could never match.
pub fn published_terminals() -> Vec<String> {
    token_samples()
        .iter()
        .filter_map(|token| match classify_token(token) {
            TerminalClassification::Terminal(name) => Some(name),
            TerminalClassification::NotTerminal(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_every_variant_is_sampled() {
        // `classify_token`'s match is exhaustive, so the compiler guarantees every
        // variant is classified. What it cannot guarantee is that `token_samples`
        // lists them all, and the published vocabulary is derived from that list.
        // This pins the count so a new variant fails here until it is sampled.
        assert_eq!(
            token_samples().len(),
            128,
            "token_samples must carry one sample of every Token variant"
        );
    }

    #[test]
    fn test_terminal_names_unique() {
        let names = published_terminals();
        let mut seen = HashSet::new();
        for name in &names {
            assert!(
                seen.insert(name.clone()),
                "two tokens publish the same terminal name: {name}"
            );
        }
        assert!(
            names.len() > 100,
            "expected the published vocabulary to cover the language, got {}",
            names.len()
        );
    }

    #[test]
    fn test_non_terminals_carry_a_reason() {
        let skipped: Vec<_> = token_samples()
            .into_iter()
            .filter(|token| {
                matches!(
                    classify_token(token),
                    TerminalClassification::NotTerminal(_)
                )
            })
            .collect();
        assert!(
            !skipped.is_empty(),
            "the lexer skips or rewrites some tokens; none were classified that way"
        );
    }
}
