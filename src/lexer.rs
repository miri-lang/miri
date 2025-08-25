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

    #[regex(r#"re'[^'\\]*(?:\\.[^'\\]*)*'[igmsu]*"#)] SingleQuotedRegex,
    #[regex(r#"re"[^"\\]*(?:\\.[^"\\]*)*"[igmsu]*"#)] DoubleQuotedRegex,
    Regex(RegexToken),

    #[regex(r#"'[^'\\]*(?:\\.[^'\\]*)*'"#)] SingleQuotedString,
    #[regex(r#""[^"\\]*(?:\\.[^"\\]*)*""#)] DoubleQuotedString,
    String,

    #[regex(r#"f'[^'\\]*(?:\\.[^'\\]*)*'"#)] SingleQuotedFormattedString,
    #[regex(r#"f"[^"\\]*(?:\\.[^"\\]*)*""#)] DoubleQuotedFormattedString,
    FormattedStringStart(String),
    FormattedStringMiddle(String),
    FormattedStringEnd(String),

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

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
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
    pending_tokens_stack: Vec<TokenSpan>,
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
            pending_tokens_stack: Vec::new(),
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
            if let Some(item) = self.pending_tokens_stack.pop() {
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
                            self.pending_tokens_stack.push((Token::Dedent, source_len..source_len));
                            self.indent_stack.pop();
                        }
                        
                        // Return the first pending dedent token if any
                        return self.pending_tokens_stack.pop().map(Ok);
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
                        self.pending_tokens_stack.push((Token::Else, span.clone()));
                        return Some(Ok((Token::ExpressionStatementEnd, span)));
                    }
                    return Some(Ok((Token::Else, span)));
                },
                Token::SingleQuotedRegex | Token::DoubleQuotedRegex => {
                    let quote_char = if token == Token::SingleQuotedRegex { '\'' } else { '"' };
                    match self.parse_regex_literal(quote_char) {
                        Ok(regex) => return Some(Ok((Token::Regex(regex), span))),
                        Err(e) => return Some(Err(e)),
                    }
                },
                Token::SingleQuotedString | Token::DoubleQuotedString => {
                    return Some(Ok((Token::String, span)));
                },
                Token::SingleQuotedFormattedString | Token::DoubleQuotedFormattedString => {
                    let quote_char = if token == Token::SingleQuotedFormattedString { '\'' } else { '"' };
                    if let Err(e) = self.parse_formatted_string(quote_char) {
                        return Some(Err(e));
                    }
                    continue;
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
                self.pending_tokens_stack.push((Token::ExpressionStatementEnd, i..i));
            }
        }

        let bump_len = i - self.inner.span().start - 1;
        self.inner.bump(bump_len);

        Ok(())
    }

    fn parse_regex_literal(&mut self, quote_character: char) -> Result<RegexToken, SyntaxError> {
        let slice = self.inner.slice(); // Example: re"\d+"ig
        let without_prefix = &slice[2..]; // remove `re`

        let quote_start = without_prefix.find(quote_character);
        let quote_end = without_prefix.rfind(quote_character);

        if quote_start.is_none() || quote_end.is_none() || quote_start == quote_end {
            return Err(SyntaxError::new(SyntaxErrorKind::InvalidRegexLiteral, self.inner.span()));
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

        Ok(regex)
    }

    fn parse_formatted_string2(&mut self, quote_character: char) -> Result<(), SyntaxError> {
        let slice = self.inner.slice(); // Example: f"Hello, {name}!"
        let without_prefix = &slice[1..]; // remove `f`
        let escape_sequence = format!("\\{}", quote_character);
        let quote_start = without_prefix.find(quote_character);
        let quote_end = without_prefix.rfind(quote_character);

        if quote_start.is_none() || quote_end.is_none() || quote_start == quote_end {
            return Err(SyntaxError::new(SyntaxErrorKind::InvalidFormattedString, self.inner.span()));
        }

        let string_body = &without_prefix[quote_start.unwrap() + 1..quote_end.unwrap()];
        let mut start = 0;
        let token_offset = self.inner.span().start + 2; // position after f" or f'
        let mut tokens: Vec<TokenSpan> = Vec::new();
        let mut open_braces = 0;

        for (i, c) in string_body.char_indices() {
            if c == '{' {
                if i == 0 || &string_body[i - 1..i] != "\\" {
                    open_braces += 1;
                    if open_braces == 1 {
                        if start != 0 {
                            let literal = &string_body[start..i];
                            tokens.push((Token::FormattedStringMiddle(literal.to_string()), token_offset + start..token_offset + i));
                        } else if start == 0 {
                            let literal = &string_body[0..i];
                            tokens.push((Token::FormattedStringStart(literal.to_string()), token_offset..token_offset + i));
                        }
                        start = i + 1;
                    }
                }
            } else if c == '}' {
                if i == 0 || &string_body[i - 1..i] != "\\" {
                    open_braces -= 1;
                    if open_braces == 0 {
                        let expression = &string_body[start..i].replace(
                            escape_sequence.as_str(), 
                            quote_character.to_string().as_str()
                        ); // we should replace the escape sequence with the actual quote character, so that the lexer can properly tokenize strings
                        let mut lexer = Lexer::new(expression);
                        while let Some(token) = lexer.next() {
                            match token {
                                Ok((token, span)) => {
                                    let offset_span = (token_offset + span.start + start)..(token_offset + span.end + start);
                                    tokens.push((token, offset_span));
                                },
                                Err(e) => return Err(e),
                            }
                        }
                        start = i + 1;
                    }
                }
            }
        }

        if open_braces != 0 {
            return Err(
                SyntaxError::new(SyntaxErrorKind::InvalidFormattedStringExpression, self.inner.span())
            );
        }

        let end_literal = if start < string_body.len() {
            &string_body[start..]
        } else {
            ""
        };

        // If we didn't see expressions, FormattedStringStart should still be emitted, 
        // so that the parser can properly handle the string.
        let last_token = if tokens.len() > 0 {
            (Token::FormattedStringEnd(end_literal.to_string()), token_offset + start..token_offset + string_body.len())
        } else {
            (Token::FormattedStringStart(end_literal.to_string()), token_offset + start..token_offset + string_body.len())
        };
        tokens.push(last_token);

        // Push all collected tokens onto the pending stack. We need to reverse the order, because it's a stack.
        for (token, span) in tokens.into_iter().rev() {
            self.pending_tokens_stack.push((token, span));
        }

        Ok(())
    }

    fn parse_formatted_string(&mut self, quote_character: char) -> Result<(), SyntaxError> {
        let slice = self.inner.slice(); // Example: f"Hello, {name}!"
        let without_prefix = &slice[1..]; // remove `f`
        let escape_sequence = format!("\\{}", quote_character);
        let quote_start = without_prefix.find(quote_character);
        let quote_end = without_prefix.rfind(quote_character);

        if quote_start.is_none() || quote_end.is_none() || quote_start == quote_end {
            return Err(SyntaxError::new(SyntaxErrorKind::InvalidFormattedString, self.inner.span()));
        }

        let string_body = &without_prefix[quote_start.unwrap() + 1..quote_end.unwrap()];
        let mut cursor = 0;
        let token_offset = self.inner.span().start + 2; // position after f" or f'
        let mut tokens: Vec<TokenSpan> = Vec::new();
        let mut is_first_part = true;

        while cursor < string_body.len() {
            // Manually search for the next unescaped brace from the current cursor.
            let mut next_brace_pos: Option<usize> = None;
            let mut search_cursor = cursor;
            loop {
                if let Some(pos) = string_body[search_cursor..].find('{') {
                    let absolute_pos = search_cursor + pos;
                    // Check if the brace is escaped.
                    if absolute_pos > 0 && &string_body[absolute_pos - 1..absolute_pos] == "\\" {
                        // It's escaped. Continue searching after this escaped brace.
                        search_cursor = absolute_pos + 1;
                        continue;
                    } else {
                        // Found a real, unescaped brace.
                        next_brace_pos = Some(absolute_pos);
                        break;
                    }
                } else {
                    // No more braces in the rest of the string.
                    break;
                }
            }

            if let Some(brace_pos) = next_brace_pos {
                // Handle the literal part before the expression
                let literal = &string_body[cursor..brace_pos];
                let literal_span = (token_offset + cursor)..(token_offset + brace_pos);
                if is_first_part {
                    tokens.push((Token::FormattedStringStart(literal.to_string()), literal_span));
                } else {
                    tokens.push((Token::FormattedStringMiddle(literal.to_string()), literal_span));
                }
                is_first_part = false;

                // Find the matching closing brace for the expression
                let mut brace_depth = 1;
                let expr_start = brace_pos + 1;
                let mut expr_end = 0;
                for (i, c) in string_body[expr_start..].char_indices() {
                    if c == '{' { brace_depth += 1; }
                    else if c == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            expr_end = expr_start + i;
                            break;
                        }
                    }
                }

                if expr_end == 0 {
                    return Err(SyntaxError::new(SyntaxErrorKind::InvalidFormattedStringExpression, self.inner.span()));
                }

                // This is the crucial part: lex the expression with span correction.
                let expression_slice = &string_body[expr_start..expr_end];
                
                // 1. Find all positions where an escape was removed. Each replacement shifts subsequent spans.
                let replacement_indices: Vec<usize> = expression_slice.match_indices(&escape_sequence)
                    .map(|(i, _)| i)
                    .collect();

                // 2. Create the clean string for the sub-lexer.
                let clean_expression = expression_slice.replace(&escape_sequence, &quote_character.to_string());

                // 3. Run the sub-lexer.
                let mut sub_lexer = Lexer::new(&clean_expression);
                while let Some(token_result) = sub_lexer.next() {
                    let (token, span) = token_result?;
                    
                    // 4. Correct the spans.
                    // The offset added to a span's position is the number of replacements that occurred before it.
                    let start_offset = replacement_indices.iter().filter(|&&pos| pos < span.start).count();
                    let end_offset = replacement_indices.iter().filter(|&&pos| pos < span.end).count();

                    let original_start = token_offset + expr_start + span.start + start_offset;
                    let original_end = token_offset + expr_start + span.end + end_offset;
                    
                    tokens.push((token, original_start..original_end));
                }

                cursor = expr_end + 1;
            } else {
                // No more expressions, the rest is the final literal part.
                break;
            }
        }

        // Handle the final literal part after the last expression.
        let final_literal = &string_body[cursor..];
        let final_span = (token_offset + cursor)..(token_offset + string_body.len());
        if is_first_part {
            // The string had no expressions at all.
            tokens.push((Token::FormattedStringStart(final_literal.to_string()), final_span));
        } else {
            tokens.push((Token::FormattedStringEnd(final_literal.to_string()), final_span));
        }

        // Push all collected tokens onto the pending stack.
        for (token, span) in tokens.into_iter().rev() {
            self.pending_tokens_stack.push((token, span));
        }

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
        self.pending_tokens_stack.push((Token::Indent, i..i));
        self.indent_stack.push(indent_len);
        self.indent_level += 1;
    }

    fn push_dedent(&mut self, i: usize) {
        self.pending_tokens_stack.push((Token::Dedent, i..i));
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
        Token::String => "string".into(),
        Token::SingleQuotedString => "string".into(),
        Token::DoubleQuotedString => "string".into(),
        Token::Regex(_) => "regular expression".into(),
        Token::FormattedStringStart(_) => "start of a formatted string".into(),
        Token::FormattedStringMiddle(_) => "middle of a formatted string".into(),
        Token::FormattedStringEnd(_) => "end of a formatted string".into(),
        _ => format!("{:?}", token).to_lowercase(),
    }
}


