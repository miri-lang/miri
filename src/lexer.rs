use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
pub enum Token {
    // Keywords
    #[token("use")]        Use,
    #[token("async")]      Async,
    #[token("await")]      Await,
    #[token("spawn")]      Spawn,
    #[token("gpu")]        Gpu,
    #[token("if")]         If,
    #[token("else")]       Else,
    #[token("match")]      Match,
    #[token("default")]    Default,
    #[token("return")]     Return,
    #[token("while")]      While,
    #[token("do")]         Do,
    #[token("for")]        For,
    #[token("in")]         In,
    #[token("var")]        Var,
    #[token("or")]         Or,
    #[token("and")]        And,
    #[token("not")]        Not,
    #[token("true")]       True,
    #[token("false")]      False,
    #[token("from")]       From,
    #[token("as")]         As,

    // Symbols and Operators
    #[token(":")]           Colon,
    #[token("=>")]          FatArrow,
    #[token("->")]          Arrow,
    #[token("<-")]          LeftArrow,
    #[token("||")]          Parallel,
    #[token("==")]          Eq,
    #[token("!=")]          Neq,
    #[token(">=")]          Gte,
    #[token("<=")]          Lte,
    #[token(">")]           Gt,
    #[token("<")]           Lt,
    #[token("=")]           Assign,
    #[token("+")]           Plus,
    #[token("-")]           Minus,
    #[token("*")]           Star,
    #[token("/")]           Slash,
    #[token("%")]           Percent,
    #[token(",")]           Comma,
    #[token("..")]          Range,
    #[token(".")]           Dot,
    #[token("(")]           LParen,
    #[token(")")]           RParen,
    #[token("[")]           LBracket,
    #[token("]")]           RBracket,
    #[token("{")]           LBrace,
    #[token("}")]           RBrace,
    #[token("|")]           Pipe,
    #[token("?")]           Try,

    // Identifiers and Literals
    #[regex("[a-zA-Z_][a-zA-Z0-9_]*")] Identifier,
    #[regex(":[a-zA-Z_][a-zA-Z0-9_]*")] Symbol,
    #[regex("::+[a-zA-Z_][a-zA-Z0-9_]*")] IncorrectSymbol,
    #[regex(r#"'[^'\\]*(?:\\.[^'\\]*)*'"#)] SingleQuotedString,
    #[regex(r#""[^"\\]*(?:\\.[^"\\]*)*""#)] DoubleQuotedString,
    #[regex("[0-9]+(?:_[0-9]+)*", priority = 2)] Int,
    #[regex("[0-9]+(?:_[0-9]+)*(\\.[0-9]+(?:_[0-9]+)*)?", priority = 1)] Float,

    // Comments and Whitespace
    #[regex("//.*", logos::skip)] InlineComment,
    #[regex(r"/\*")] MultilineComment,
    #[regex("\n\r?")] Newline,

    Indent,
    Dedent,

    #[regex("[ \t\r]+", logos::skip)] Whitespace,
}

pub struct Lexer<'source> {
    inner: logos::Lexer<'source, Token>,
    source: &'source str,
    pending_tokens: Vec<(usize, Token, usize)>,
    indent_stack: Vec<usize>, // stack of indent levels (in spaces)
    eof_handled: bool,
}

impl<'source> Iterator for Lexer<'source> {
    type Item = (usize, Token, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(item) = self.pending_tokens.pop() {
            return Some(item);
        }

        let next_result = self.inner.next();

        // Handle EOF - generate remaining dedent tokens
        if next_result.is_none() {
            if !self.eof_handled {
                self.eof_handled = true;
                let source_len = self.source.len();
                
                // Generate dedent tokens for all remaining indentation levels
                while self.indent_stack.len() > 1 {
                    self.pending_tokens.push((source_len, Token::Dedent, source_len));
                    self.indent_stack.pop();
                }
                
                // Return the first pending dedent token if any
                return self.pending_tokens.pop();
            }
            return None;
        }

        let token = next_result.unwrap();
        let span = self.inner.span();
        let src = self.inner.source();

        if token.is_err() {
            self.panic_unsupported_token(&span, src);
            return None;
        }

        let unwrapped_token = token.unwrap();
        match unwrapped_token {
            Token::MultilineComment => {
                self.parse_nested_comment();
                return self.next();
            },
            Token::Newline => {
                self.parse_newline();
                return self.next();
            },
            Token::IncorrectSymbol => {
                self.panic_unsupported_token(&span, src);
                return None;
            },
            _ => Some((span.start, unwrapped_token, span.end))
        }
    }
}

impl<'source> Lexer<'source> {
    pub fn new(source: &'source str) -> Self {
        Lexer {
            inner: Token::lexer(source), // Newline ensures we have the last dedent token at the end
            source: source,
            pending_tokens: Vec::new(),
            indent_stack: vec![0],
            eof_handled: false,
        }
    }

    fn parse_nested_comment(&mut self) {
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
                        return;
                    }
                }
                _ => i += 1,
            }
        }
    
        panic!("Unclosed multiline comment starting at {}", self.inner.span().start);
    }
    
    fn parse_newline(&mut self) {
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
                    found_newline = indent_len == 0; // Only consider it a newline if no indentation
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
                // Indentation increase
                self.pending_tokens.push((i, Token::Indent, i));
                self.indent_stack.push(indent_len);
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
                    let line_num = src[..i].matches('\n').count() + 1;
                    panic!("[Lexer] Indentation error: unindent does not match any outer indentation level at line {}", line_num);
                }
                
                // Pop indentation levels and generate Dedent tokens
                while indent_len < *self.indent_stack.last().unwrap() {
                    self.pending_tokens.push((i, Token::Dedent, i));
                    self.indent_stack.pop();
                }
            }
        }

        let bump_len = i - self.inner.span().start - 1;
        self.inner.bump(bump_len);
    }

    fn panic_unsupported_token(&mut self, span: &std::ops::Range<usize>, src: &str) {
        let mut start = span.start;
        while start > 0 && src[start - 1..].chars().next().unwrap() != '\n' {
            start -= 1;
        }
    
        let mut end = span.end;
        while end < src.len() && src[end..].chars().next().unwrap() != '\n' {
            end += 1;
        }
    
        let snippet = &src[start..end].trim();
        let invalid_part = &src[span.clone()].trim();
        panic!("[Lexer] Unsupported token '{}' in line '{}'", invalid_part, snippet);
    }
}

