// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use logos::Logos;

use crate::syntax_error::{Span, SyntaxError, SyntaxErrorKind};


#[derive(Logos, Debug, PartialEq, Clone)]
pub enum Token {
    // Keywords
    #[token("use")]        Use,
    #[token("fn")]         Fn,
    #[token("async")]      Async,
    #[token("await")]      Await,
    #[token("spawn")]      Spawn,
    #[token("gpu")]        Gpu,
    #[token("if")]         If,
    #[token("unless")]     Unless,
    #[token("else")]       Else,
    #[token("match")]      Match,
    #[token("default")]    Default,
    #[token("return")]     Return,
    #[token("while")]      While,
    #[token("until")]      Until,
    #[token("do")]         Do,
    #[token("for")]        For,
    #[token("forever")]    Forever,
    #[token("in")]         In,
    #[token("let")]        Let,
    #[token("var")]        Var,
    #[token("or")]         Or,
    #[token("and")]        And,
    #[token("not")]        Not,
    #[token("true")]       True,
    #[token("false")]      False,
    #[token("from")]       From,
    #[token("as")]         As,
    #[token("break")]      Break,
    #[token("continue")]   Continue,
    #[token("extends")]    Extends,
    #[token("is")]         Is,
    #[token("includes")]   Includes,
    #[token("implements")] Implements,
    #[token("type")]       Type,
    #[token("enum")]       Enum,
    #[token("struct")]     Struct,
    #[token("public")]     Public,
    #[token("protected")]  Protected,
    #[token("private")]    Private,

    // Symbols and Operators
    #[token(":")]           Colon,
    #[token("::")]          DoubleColon,
    #[token("=>")]          FatArrow,
    #[token("->")]          Arrow,
    #[token("<-")]          LeftArrow,
    #[token("||")]          Parallel,
    #[token("==")]          Equal,
    #[token("!=")]          NotEqual,
    #[token(">=")]          GreaterThanEqual,
    #[token("<=")]          LessThanEqual,
    #[token(">")]           GreaterThan,
    #[token("<")]           LessThan,
    #[token("=")]           Assign,
    #[token("+=")]          AssignAdd,
    #[token("-=")]          AssignSub,
    #[token("*=")]          AssignMul,
    #[token("/=")]          AssignDiv,
    #[token("%=")]          AssignMod,
    #[token("+")]           Plus,
    #[token("++")]          Increment,
    #[token("-")]           Minus,
    #[token("--")]          Decrement,
    #[token("*")]           Star,
    #[token("/")]           Slash,
    #[token("%")]           Percent,
    #[token(",")]           Comma,
    #[token("..")]          Range,
    #[token("..=")]         RangeInclusive,
    #[token(".")]           Dot,
    #[token("(")]           LParen,
    #[token(")")]           RParen,
    #[token("[")]           LBracket,
    #[token("]")]           RBracket,
    #[token("{")]           LBrace,
    #[token("}")]           RBrace,
    #[token("|")]           Pipe,
    #[token("&")]           Ampersand,
    #[token("^")]           Caret,
    #[token("?")]           QuestionMark,
    #[token("~")]           Tilde,

    // Identifiers and Literals
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")] Identifier,
    #[regex(":[a-zA-Z_][a-zA-Z0-9_]*")] Symbol,
    #[regex(r#"re"([^"\\]|\\.)*"[igmsu]*"#, parse_regex_literal)] Regex(RegexToken),
    #[regex(r#"'[^'\\]*(?:\\.[^'\\]*)*'"#)] SingleQuotedString,
    #[regex(r#""[^"\\]*(?:\\.[^"\\]*)*""#)] DoubleQuotedString,
    #[regex("[0-9]+(?:_[0-9]+)*(\\.[0-9]+(?:_[0-9]+)*)?([eE][+-]?[0-9]+(?:_[0-9]+)*)?", priority = 1)] Float,
    #[regex("[0-9]+(?:_[0-9]+)*", priority = 2)] Int,
    #[regex("0[bB][0-1_]+", priority = 2)] BinaryNumber,
    #[regex("0[xX][0-9a-fA-F_]+", priority = 2)] HexNumber,
    #[regex("0[oO][0-7_]+", priority = 2)] OctalNumber,

    // Comments and Whitespace
    #[regex("//.*", logos::skip)] InlineComment,
    #[regex(r"/\*")] MultilineComment,
    #[regex("\n\r?")] Newline,

    Indent,
    Dedent,
    ExpressionStatementEnd, // Used to mark the end of an expression statement (one code line)

    #[regex("[ \t\r]+", logos::skip)] Whitespace,
}

pub type TokenSpan = (Token, Span);

#[derive(Debug, PartialEq, Clone)]
pub struct RegexToken {
    pub body: String,
    pub ignore_case: bool,
    pub global: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool
}

pub struct Lexer<'source> {
    inner: logos::Lexer<'source, Token>,
    source: &'source str,
    pending_tokens: Vec<TokenSpan>,
    indent_stack: Vec<usize>, // stack of indent levels (in spaces)
    indent_level: usize, // current indent level
    eof_handled: bool,
    paren_stack: Vec<usize>, // stack of parenthesis levels
    bracket_stack: Vec<usize>, // stack of square bracket levels
    curly_brace_stack: Vec<usize>, // stack of curly brace levels
    previous_tokens: Vec<Token>, // keeps track of previous tokens, primarily for indentation handling
}

impl<'source> Iterator for Lexer<'source> {
    type Item = Result<TokenSpan, SyntaxError>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.generate_token();
        if let Some(Ok((token, _))) = &item {
            self.memorize_token(token.clone());
        }
        item
    }
}

impl<'source> Lexer<'source> {
    const MAX_PREVIOUS_TOKENS: usize = 3;

    pub fn new(source: &'source str) -> Self {
        Lexer {
            inner: Token::lexer(source),
            source: source,
            pending_tokens: Vec::new(),
            indent_stack: vec![0],
            indent_level: 0,
            eof_handled: false,
            paren_stack: Vec::new(),
            bracket_stack: Vec::new(),
            curly_brace_stack: Vec::new(),
            previous_tokens: Vec::new(),
        }
    }

    fn generate_token(&mut self) -> Option<Result<TokenSpan, SyntaxError>> {
        loop {
            if let Some(item) = self.pending_tokens.pop() {
                return Some(Ok(item));
            }

            let token = match self.inner.next() {
                Some(Ok(t)) => t,
                Some(Err(_)) => {
                    // This is where logos itself detects an error.
                    return Some(
                        Err(
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidToken, self.inner.span()
                            )
                        )
                    );
                }
                None => {
                    if !self.eof_handled {
                        self.eof_handled = true;
                        let source_len = self.source.len();
                        
                        // Generate dedent tokens for all remaining indentation levels
                        while self.indent_stack.len() > 1 {
                            self.pending_tokens.push((Token::Dedent, source_len..source_len));
                            self.indent_stack.pop();
                        }
                        
                        // Return the first pending dedent token if any
                        return self.pending_tokens.pop().map(Ok);
                    }
                    return None;
                }
            };

            let span = self.inner.span();

            match token {
                Token::MultilineComment => {
                    if let Err(e) = self.parse_nested_comment() {
                        return Some(Err(e));
                    }
                    continue;
                },
                Token::Newline => {
                    if self.have_previous_tokens() {
                        if let Err(e) = self.parse_newline() {
                            return Some(Err(e));
                        }
                    }
                    continue;
                },
                Token::LParen => {
                    self.paren_stack.push(self.inner.span().start);
                    return Some(Ok((Token::LParen, span)));
                },
                Token::RParen => {
                    self.paren_stack.pop();
                    return Some(Ok((Token::RParen, span)));
                },
                Token::LBracket => {
                    self.bracket_stack.push(self.inner.span().start);
                    return Some(Ok((Token::LBracket, span)));
                },
                Token::RBracket => {
                    self.bracket_stack.pop();
                    return Some(Ok((Token::RBracket, span)));
                },
                Token::LBrace => {
                    self.curly_brace_stack.push(self.inner.span().start);
                    return Some(Ok((Token::LBrace, span)));
                },
                Token::RBrace => {
                    self.curly_brace_stack.pop();
                    return Some(Ok((Token::RBrace, span)));
                },
                Token::Else => {
                    // Add an ExpressionStatementEnd token if the previous token is not a Dedent or ExpressionStatementEnd
                    // This is to ensure that the inline if/else block is treated as a separate statement
                    if self.have_previous_tokens() && !self.match_previous_token(Token::Dedent) && !self.match_previous_token(Token::ExpressionStatementEnd) {
                        self.pending_tokens.push((Token::Else, span.clone()));
                        return Some(Ok((Token::ExpressionStatementEnd, span)));
                    }
                    return Some(Ok((Token::Else, span)));
                },
                _ => return Some(Ok((token, span)))
            }
        }
    }

    fn parse_nested_comment(&mut self) -> Result<(), SyntaxError> {
        let src = self.inner.source();
        let mut depth = 1;
        let mut i = self.inner.span().end;
    
        while i + 1 < src.len() {
            let ch = &src[i..i + 2];
            match ch {
                "/*" => {
                    depth += 1;
                    i += 2;
                }
                "*/" => {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        let bump_len = i - self.inner.span().start - 2;
                        self.inner.bump(bump_len);
                        return Ok(());
                    }
                }
                _ => i += 1,
            }
        }
    
        Err(
            SyntaxError::new(
                SyntaxErrorKind::UnclosedMultilineComment, self.inner.span()
            )
        )
    }

    fn parse_newline(&mut self) -> Result<(), SyntaxError> {
        let src = self.inner.source();
        let mut i = self.inner.span().end;
        let mut indent_len: usize = 0;
        let mut found_comment = false;
        let mut found_newline = false;

        // Count indentation
        while i < src.len() {
            let ch = &src[i..i + 1];
            match ch {
                " " => indent_len += 1,
                "\t" => indent_len += 4,
                "/" => {
                    // Ignore indentation before comments
                    if i + 1 < src.len() {
                        let next_ch = &src[i + 1..i + 2];
                        if next_ch == "/" || next_ch == "*" {
                            found_comment = true;
                        }
                    }
                    break;
                },
                "\n" | "\r" => {
                    found_newline = true;
                    break;
                },
                _ => break
            }
            i += 1;
        }

        if !found_comment && !found_newline {
            // Handle indentation changes
            let last_indent = *self.indent_stack.last().unwrap();
            
            if indent_len > last_indent {
                // If we are not inside parentheses or brackets, treat as an indentation increase
                if self.is_outside_paired_tokens() {
                    // Indentation increase
                    self.push_indent(i, indent_len);
                } else {
                    if self.paren_stack.len() > 0 && self.prev_tokens_match_function_declaration() {
                        // If this is a function declaration within function arguments, treat as an indentation increase
                        self.push_indent(i, indent_len);
                    }
                }
            } else if indent_len < last_indent {
                // Dedentation - must match a previous indentation level
                let mut found_matching_indent = false;
                
                for &level in self.indent_stack.iter() {
                    if level == indent_len {
                        found_matching_indent = true;
                        break;
                    }
                }
                
                if !found_matching_indent {
                    return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::IndentationMismatch, i..i
                        )
                    );
                }
                
                // Pop indentation levels and generate Dedent tokens
                while indent_len < *self.indent_stack.last().unwrap() {
                    self.push_dedent(i);
                }
            }

            if self.is_expression_statement_end() {
                // If this is an expression statement end, return ExpressionStatementEnd token
                self.pending_tokens.push((Token::ExpressionStatementEnd, i..i));
            }
        }

        let bump_len = i - self.inner.span().start - 1;
        self.inner.bump(bump_len);

        Ok(())
    }

    fn memorize_token(&mut self, token: Token) {
        self.previous_tokens.push(token);
        if self.previous_tokens.len() > Self::MAX_PREVIOUS_TOKENS {
            self.previous_tokens.remove(0); // Keep only the limited amount tokens
        }
    }

    fn have_previous_tokens(&self) -> bool {
        !self.previous_tokens.is_empty()
    }

    fn matches_previous_tokens(&self, tokens: &Vec<Token>) -> bool {
        if tokens.len() > Self::MAX_PREVIOUS_TOKENS {
            panic!("[Lexer] BUG: Trying to match {} previous tokens, but only {} allowed", tokens.len(), Self::MAX_PREVIOUS_TOKENS);
        }

        if self.previous_tokens.len() < tokens.len() {
            return false;
        }

        let start_index = self.previous_tokens.len() - tokens.len();
        &self.previous_tokens[start_index..] == tokens
    }
    
    fn match_previous_token(&self, token: Token) -> bool {
        self.previous_tokens.last().map_or(false, |t| *t == token)
    }

    fn prev_tokens_match_function_declaration(&self) -> bool {
        self.matches_previous_tokens(&vec![Token::RParen, Token::Identifier]) ||
            self.matches_previous_tokens(&vec![Token::RParen])
    }

    fn push_indent(&mut self, i: usize, indent_len: usize) {
        self.pending_tokens.push((Token::Indent, i..i));
        self.indent_stack.push(indent_len);
        self.indent_level += 1;
    }

    fn push_dedent(&mut self, i: usize) {
        self.pending_tokens.push((Token::Dedent, i..i));
        self.indent_stack.pop();
        self.indent_level -= 1;
    }

    fn is_outside_paired_tokens(&self) -> bool {
        self.paren_stack.is_empty() && self.bracket_stack.is_empty() && self.curly_brace_stack.is_empty()
    }

    fn is_inside_code_block(&self) -> bool {
        self.indent_level > 0
    }

    fn is_expression_statement_end(&self) -> bool {
        (self.is_outside_paired_tokens() || self.is_inside_code_block()) && 
            !self.match_previous_token(Token::ExpressionStatementEnd)
    }
}

pub fn token_to_string(token: &Token) -> String {
    match token {
        Token::Colon => ":".into(),
        Token::DoubleColon => "::".into(),
        Token::FatArrow => "=>".into(),
        Token::Arrow => "->".into(),
        Token::LeftArrow => "<-".into(),
        Token::Parallel => "||".into(),
        Token::Equal => "==".into(),
        Token::NotEqual => "!=".into(),
        Token::GreaterThanEqual => ">=".into(),
        Token::LessThanEqual => "<=".into(),
        Token::GreaterThan => ">".into(),
        Token::LessThan => "<".into(),
        Token::Assign => "=".into(),
        Token::AssignAdd => "+=".into(),
        Token::AssignSub => "-=".into(),
        Token::AssignMul => "*=".into(),
        Token::AssignDiv => "/=".into(),
        Token::AssignMod => "%=".into(),
        Token::Plus => "+".into(),
        Token::Increment => "++".into(),
        Token::Minus => "-".into(),
        Token::Decrement => "--".into(),
        Token::Star => "*".into(),
        Token::Slash => "/".into(),
        Token::Percent => "%".into(),
        Token::Comma => ",".into(),
        Token::Range => "..".into(),
        Token::RangeInclusive => "..=".into(),
        Token::Dot => ".".into(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::LBracket => "[".into(),
        Token::RBracket => "]".into(),
        Token::LBrace => "{".into(),
        Token::RBrace => "}".into(),
        Token::Pipe => "|".into(),
        Token::Ampersand => "&".into(),
        Token::Caret => "^".into(),
        Token::QuestionMark => "?".into(),
        Token::Tilde => "~".into(),
        Token::ExpressionStatementEnd => "end of expression".into(),
        Token::SingleQuotedString => "string".into(),
        Token::DoubleQuotedString => "string".into(),
        _ => format!("{:?}", token).to_lowercase(),
    }
}

fn parse_regex_literal(lexer: &mut logos::Lexer<Token>) -> Option<RegexToken> {
    let slice = lexer.slice(); // Example: re"\d+"/ig
    let without_prefix = &slice[2..]; // remove `re`

    let quote_start = without_prefix.find('"');
    let quote_end = without_prefix.rfind('"');

    if quote_start.is_none() || quote_end.is_none() || quote_start == quote_end {
        return None; // Invalid regex literal
    }

    let regex_body = &without_prefix[quote_start.unwrap() + 1..quote_end.unwrap()];
    let flags = &without_prefix[quote_end.unwrap() + 1..]; // everything after the final quote

    let mut regex = RegexToken {
        body: regex_body.to_string(),
        ignore_case: false,
        global: false,
        multiline: false,
        dot_all: false,
        unicode: false,
    };

    for flag in flags.chars() {
        match flag {
            'i' => regex.ignore_case = true,
            'g' => regex.global = true,
            'm' => regex.multiline = true,
            's' => regex.dot_all = true,
            'u' => regex.unicode = true,
            _ => {}
        }
    }

    Some(regex)
}