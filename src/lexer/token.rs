// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use logos::Logos;

use crate::error::syntax::Span;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct RegexToken {
    pub body: String,
    pub ignore_case: bool,
    pub global: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
}

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
    #[token("forever")]
    Forever,
    #[token("in")]
    In,
    #[token("let")]
    Let,
    #[token("var")]
    Var,
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
    #[token("~")]
    Tilde,

    // Identifiers and Literals
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")]
    Identifier,
    #[regex(":[a-zA-Z_][a-zA-Z0-9_]*")]
    Symbol,

    #[regex(r#"re'[^'\\]*(?:\\.[^'\\]*)*'[igmsu]*"#)]
    SingleQuotedRegex,
    #[regex(r#"re"[^"\\]*(?:\\.[^"\\]*)*"[igmsu]*"#)]
    DoubleQuotedRegex,
    Regex(RegexToken),

    #[regex(r#"'[^'\\]*(?:\\.[^'\\]*)*'"#)]
    SingleQuotedString,
    #[regex(r#""[^"\\]*(?:\\.[^"\\]*)*""#)]
    DoubleQuotedString,
    String,

    #[regex(r#"f'[^'\\]*(?:\\.[^'\\]*)*'"#)]
    SingleQuotedFormattedString,
    #[regex(r#"f"[^"\\]*(?:\\.[^"\\]*)*""#)]
    DoubleQuotedFormattedString,
    FormattedStringStart(String),
    FormattedStringMiddle(String),
    FormattedStringEnd(String),

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
    #[regex("\\.[0-9]+(?:_[0-9]+)*([eE][+-]?[0-9]+(?:_[0-9]+)*)?", priority = 3)]
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
    ExpressionStatementEnd, // Used to mark the end of an expression statement (one code line)

    #[regex("[ \t\r]+", logos::skip)]
    Whitespace,
    #[regex("#!.*", logos::skip, allow_greedy = true)]
    Shebang,
    #[token("\u{FEFF}", logos::skip)]
    ByteOrderMark,
}

pub type TokenSpan = (Token, Span);
