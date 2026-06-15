// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use logos::Logos;

use super::{BracketLevel, LexAction, LexerWork};
use crate::error::syntax::{Span, SyntaxErrorKind};

/// A parsed regex literal with its flags.
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct RegexToken {
    pub body: String,
    pub ignore_case: bool,
    pub global: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
}

/// All token types produced by the lexer.
#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token {
    // Keywords
    #[token("use")]
    Use,
    #[token("fn")]
    Fn,
    #[token("async")]
    Async,
    #[token("await")]
    Await,
    #[token("spawn")]
    Spawn,
    #[token("parallel")]
    Parallel,
    #[token("gpu")]
    Gpu,
    #[token("frame")]
    Frame,
    #[token("if")]
    If,
    #[token("unless")]
    Unless,
    #[token("else")]
    Else,
    #[token("match")]
    Match,
    #[token("default")]
    Default,
    #[token("return")]
    Return,
    #[token("while")]
    While,
    #[token("until")]
    Until,
    #[token("do")]
    Do,
    #[token("for")]
    For,
    #[token("forall")]
    Forall,
    #[token("forever")]
    Forever,
    #[token("in")]
    In,
    #[token("let")]
    Let,
    #[token("var")]
    Var,
    #[token("const")]
    Const,
    #[token("or")]
    Or,
    #[token("and")]
    And,
    #[token("not")]
    Not,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("None")]
    None,
    #[token("from")]
    From,
    #[token("as")]
    As,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("extends")]
    Extends,
    #[token("is")]
    Is,
    #[token("includes")]
    Includes,
    #[token("implements")]
    Implements,
    #[token("type")]
    Type,
    #[token("enum")]
    Enum,
    #[token("struct")]
    Struct,
    #[token("class")]
    Class,
    #[token("trait")]
    Trait,
    #[token("super")]
    Super,
    #[token("public")]
    Public,
    #[token("protected")]
    Protected,
    #[token("shared")]
    Shared,
    #[token("private")]
    Private,
    #[token("system")]
    System,
    #[token("local")]
    Local,
    #[token("abstract")]
    Abstract,
    #[token("must_use")]
    MustUse,
    #[token("out")]
    Out,
    #[token("runtime")]
    Runtime,
    #[token("intrinsic")]
    Intrinsic,

    // Symbols and Operators
    #[token(";")]
    Semicolon,
    #[token(":")]
    Colon,
    #[token("::")]
    DoubleColon,
    #[token("=>")]
    FatArrow,
    #[token("->")]
    Arrow,
    #[token("<-")]
    LeftArrow,
    #[token("==")]
    Equal,
    #[token("!=")]
    NotEqual,
    #[token(">=")]
    GreaterThanEqual,
    #[token("<=")]
    LessThanEqual,
    #[token(">")]
    GreaterThan,
    #[token("<")]
    LessThan,
    #[token("=")]
    Assign,
    #[token("+=")]
    AssignAdd,
    #[token("-=")]
    AssignSub,
    #[token("*=")]
    AssignMul,
    #[token("/=")]
    AssignDiv,
    #[token("%=")]
    AssignMod,
    #[token("+")]
    Plus,
    #[token("++")]
    Increment,
    #[token("-")]
    Minus,
    #[token("--")]
    Decrement,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token(",")]
    Comma,
    #[token("..")]
    Range,
    #[token("..=")]
    RangeInclusive,
    #[token(".")]
    Dot,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("|")]
    Pipe,
    #[token("&")]
    Ampersand,
    #[token("^")]
    Caret,
    #[token("?")]
    QuestionMark,
    #[token("??")]
    QuestionQuestion,
    #[token("~")]
    Tilde,

    // Identifiers and Literals
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")]
    Identifier,
    #[regex(r#"re'[^'\\]*(?:\\.[^'\\]*)*'[igmsu]*"#)]
    SingleQuotedRegex,
    #[regex(r#"re"[^"\\]*(?:\\.[^"\\]*)*"[igmsu]*"#)]
    DoubleQuotedRegex,
    Regex(Box<RegexToken>),

    #[regex(r#"'[^'\\]*(?:\\.[^'\\]*)*'"#)]
    SingleQuotedString,
    #[regex(r#""[^"\\]*(?:\\.[^"\\]*)*""#)]
    DoubleQuotedString,
    String,

    #[regex(r#"f'[^'\\]*(?:\\.[^'\\]*)*'"#)]
    SingleQuotedFormattedString,
    #[regex(r#"f"[^"\\]*(?:\\.[^"\\]*)*""#)]
    DoubleQuotedFormattedString,
    FormattedStringStart(Box<String>),
    FormattedStringMiddle(Box<String>),
    FormattedStringEnd(Box<String>),

    #[regex(
        r"[0-9]+(?:_[0-9]+)*(\\.[0-9]+(?:_[0-9]+)*)?([eE][+-]?[0-9]+(?:_[0-9]+)*)?_+",
        priority = 5
    )]
    #[regex(
        r"_+[0-9]+(?:_[0-9]+)*(\\.[0-9]+(?:_[0-9]+)*)?([eE][+-]?[0-9]+(?:_[0-9]+)*)?",
        priority = 5
    )]
    InvalidNumber,

    #[regex("[0-9]+(?:_[0-9]+)*\\.", priority = 4)]
    FloatOrRange,

    #[regex(r"\.[0-9]+(?:_[0-9]+)*([eE][+-]?[0-9]+(?:_[0-9]+)*)?", priority = 3)]
    #[regex(
        "[0-9]+(?:_[0-9]+)*(\\.[0-9]+(?:_[0-9]+)*)?([eE][+-]?[0-9]+(?:_[0-9]+)*)?",
        priority = 2
    )]
    Float,
    #[regex("[0-9]+(?:_[0-9]+)*", priority = 3)]
    Int,
    #[regex("0[bB][0-1_]+", priority = 2)]
    BinaryNumber,
    #[regex("0[xX][0-9a-fA-F_]+", priority = 2)]
    HexNumber,
    #[regex("0[oO][0-7_]+", priority = 2)]
    OctalNumber,

    #[regex("0[bB](?:[0-1_]*[^0-1_\\s]+)?")]
    #[regex("0[bB]_+[0-1_]*")]
    InvalidBinaryNumber,

    #[regex("0[xX](?:[0-9a-fA-F_]*[^0-9a-fA-F_\\s]+)?")]
    #[regex("0[xX]_+[0-9a-fA-F_]*")]
    InvalidHexNumber,

    #[regex("0[oO](?:[0-7_]*[^0-7_\\s]+)?")]
    #[regex("0[oO]_+[0-7_]*")]
    InvalidOctalNumber,

    // Comments and Whitespace
    #[regex("//.*", logos::skip, allow_greedy = true)]
    InlineComment,
    #[regex(r"/\*")]
    MultilineComment,
    #[regex("\r?\n")]
    Newline,

    Indent,
    Dedent,
    /// Marks the end of an expression statement (one logical line).
    ExpressionStatementEnd,

    #[regex("[ \t\r]+", logos::skip)]
    Whitespace,
    #[regex("#!.*", logos::skip, allow_greedy = true)]
    Shebang,
    #[token("\u{FEFF}", logos::skip)]
    ByteOrderMark,
}

/// A token paired with its source span.
pub type TokenSpan = (Token, Span);

impl Token {
    /// Classifies how the indentation-aware lexer must handle this raw token from logos.
    /// The match is exhaustive so adding a new `Token` variant forces a deliberate update
    /// here rather than a silent pass-through.
    #[rustfmt::skip]
    pub(crate) fn lex_action(&self) -> LexAction {
        match self {
            Token::MultilineComment => LexAction::ContinueWith(LexerWork::NestedComment),
            Token::Newline => LexAction::ContinueWith(LexerWork::Newline),
            Token::FloatOrRange => LexAction::ContinueWith(LexerWork::FloatOrRange),
            Token::LParen => LexAction::TrackOpen(BracketLevel::Paren),
            Token::RParen => LexAction::TrackClose(BracketLevel::Paren),
            Token::LBracket => LexAction::TrackOpen(BracketLevel::Bracket),
            Token::RBracket => LexAction::TrackClose(BracketLevel::Bracket),
            Token::LBrace => LexAction::TrackOpen(BracketLevel::Brace),
            Token::RBrace => LexAction::TrackClose(BracketLevel::Brace),
            Token::SingleQuotedRegex => LexAction::Regex('\''),
            Token::DoubleQuotedRegex => LexAction::Regex('"'),
            Token::SingleQuotedString | Token::DoubleQuotedString => LexAction::PromoteToString,
            Token::SingleQuotedFormattedString => LexAction::FormattedString('\''),
            Token::DoubleQuotedFormattedString => LexAction::FormattedString('"'),
            Token::InvalidNumber => LexAction::Invalid(SyntaxErrorKind::InvalidNumberLiteral),
            Token::InvalidBinaryNumber => LexAction::Invalid(SyntaxErrorKind::InvalidBinaryLiteral),
            Token::InvalidHexNumber => LexAction::Invalid(SyntaxErrorKind::InvalidHexLiteral),
            Token::InvalidOctalNumber => LexAction::Invalid(SyntaxErrorKind::InvalidOctalLiteral),
            Token::Use | Token::Fn | Token::Async | Token::Await | Token::Spawn | Token::Parallel | Token::Gpu | Token::Frame
            | Token::If | Token::Unless | Token::Else | Token::Match | Token::Default | Token::Return
            | Token::While | Token::Until | Token::Do | Token::For | Token::Forall | Token::Forever | Token::In
            | Token::Let | Token::Var | Token::Const | Token::Or | Token::And | Token::Not
            | Token::True | Token::False | Token::None | Token::From | Token::As | Token::Break | Token::Continue
            | Token::Extends | Token::Is | Token::Includes | Token::Implements | Token::Type | Token::Enum
            | Token::Struct | Token::Class | Token::Trait | Token::Super | Token::Public | Token::Protected
            | Token::Shared | Token::Private | Token::System | Token::Local | Token::Abstract | Token::MustUse
            | Token::Out | Token::Runtime | Token::Intrinsic
            | Token::Semicolon | Token::Colon | Token::DoubleColon | Token::FatArrow | Token::Arrow | Token::LeftArrow
            | Token::Equal | Token::NotEqual | Token::GreaterThanEqual | Token::LessThanEqual | Token::GreaterThan | Token::LessThan
            | Token::Assign | Token::AssignAdd | Token::AssignSub | Token::AssignMul | Token::AssignDiv | Token::AssignMod
            | Token::Plus | Token::Increment | Token::Minus | Token::Decrement | Token::Star | Token::Slash | Token::Percent
            | Token::Comma | Token::Range | Token::RangeInclusive | Token::Dot
            | Token::Pipe | Token::Ampersand | Token::Caret | Token::QuestionMark | Token::QuestionQuestion | Token::Tilde
            | Token::Identifier | Token::Int | Token::Float | Token::BinaryNumber | Token::HexNumber | Token::OctalNumber
            | Token::String | Token::Regex(_)
            | Token::FormattedStringStart(_) | Token::FormattedStringMiddle(_) | Token::FormattedStringEnd(_)
            | Token::Indent | Token::Dedent | Token::ExpressionStatementEnd
            | Token::InlineComment | Token::Whitespace | Token::Shebang | Token::ByteOrderMark => LexAction::EmitAsIs,
        }
    }
}
