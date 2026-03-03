// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::borrow::Cow;

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::Parser;

/// Processes escape sequences in a string literal, returning a `Cow`
/// to avoid allocation when no escape sequences are present.
pub(crate) fn unescape_string(s: &str) -> Cow<'_, str> {
    if !s.contains('\\') {
        return Cow::Borrowed(s);
    }

    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('0') => result.push('\0'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    Cow::Owned(result)
}

/// Strips underscore separators from a numeric literal, returning a `Cow`
/// to avoid allocation when no underscores are present.
fn strip_underscores(s: &str) -> Cow<'_, str> {
    if s.contains('_') {
        Cow::Owned(s.replace('_', ""))
    } else {
        Cow::Borrowed(s)
    }
}

impl<'source> Parser<'source> {
    /*
        Literal
            : IntegerLiteral
            : FloatLiteral
            : StringLiteral
            : BooleanLiteral
            ;
    */
    pub(crate) fn literal(&mut self) -> Result<Literal, SyntaxError> {
        match &self._lookahead {
            Some((Token::Int, _)) => self.integer_literal(&Token::Int),
            Some((Token::BinaryNumber, _)) => self.integer_literal(&Token::BinaryNumber),
            Some((Token::HexNumber, _)) => self.integer_literal(&Token::HexNumber),
            Some((Token::OctalNumber, _)) => self.integer_literal(&Token::OctalNumber),
            Some((Token::Float, _)) => self.float_literal(),
            Some((Token::True, _)) => self.boolean_literal(&Token::True),
            Some((Token::False, _)) => self.boolean_literal(&Token::False),
            Some((Token::None, _)) => {
                self.eat_token(&Token::None)?;
                Ok(Literal::None)
            }
            Some((Token::String, _)) => self.string_literal(),
            Some((Token::Regex(_), _)) => self.regex_literal(),
            Some((Token::FormattedStringStart(_), _))
            | Some((Token::FormattedStringMiddle(_), _))
            | Some((Token::FormattedStringEnd(_), _)) => {
                // These are handled by formatted_string_expression, not here.
                Err(self.error_unexpected_lookahead_token("a literal"))
            }
            Some((token, span)) => {
                let token_text = &self.source[span.start..span.end];
                Err(self.error_unexpected_token_with_span(
                    "a valid literal",
                    &format!("{:?} with value '{}'", token, token_text),
                    *span,
                ))
            }
            None => Err(self.error_eof()),
        }
    }

    /*
        IntegerLiteral
            : INT
            ;
    */
    pub(crate) fn integer_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let raw = &self.source[token.1.start..token.1.end];
                let str_value = strip_underscores(raw);

                // Parse the value based on the token type
                let value = match token_type {
                    Token::Int => str_value.parse::<i128>().map_err(|_| {
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidIntegerLiteral,
                            Span::new(token.1.start, token.1.end),
                        )
                    })?,
                    Token::BinaryNumber => {
                        // Strip "0b" prefix and parse as base 2
                        i128::from_str_radix(&str_value[2..], 2).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidBinaryLiteral,
                                Span::new(token.1.start, token.1.end),
                            )
                        })?
                    }
                    Token::HexNumber => {
                        // Strip "0x" prefix and parse as base 16
                        i128::from_str_radix(&str_value[2..], 16).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidHexLiteral,
                                Span::new(token.1.start, token.1.end),
                            )
                        })?
                    }
                    Token::OctalNumber => {
                        // Strip "0o" prefix and parse as base 8
                        i128::from_str_radix(&str_value[2..], 8).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidOctalLiteral,
                                Span::new(token.1.start, token.1.end),
                            )
                        })?
                    }
                    _ => {
                        return Err(SyntaxError::new(
                            SyntaxErrorKind::UnexpectedToken {
                                expected: "integer literal".to_string(),
                                found: format!("{:?}", token_type),
                            },
                            Span::new(token.1.start, token.1.end),
                        ));
                    }
                };

                Ok(ast::int_literal(value))
            }
            Err(e) => Err(e),
        }
    }

    /*
        FloatLiteral
            : FLOAT
            ;
    */
    pub(crate) fn float_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::Float) {
            Ok(token) => {
                let err = SyntaxError::new(
                    SyntaxErrorKind::InvalidFloatLiteral,
                    Span::new(token.1.start, token.1.end),
                );
                let raw = &self.source[token.1.start..token.1.end];
                let str_value = strip_underscores(raw);
                let f32_value = str_value.parse::<f32>().map_err(|_| err.clone())?;
                let uses_exponent = str_value.contains('e') || str_value.contains('E');
                let f32_str = if uses_exponent {
                    // Count digits after the decimal in the significand (before 'e')
                    let significand = str_value.split(['e', 'E']).next().unwrap_or("");
                    let decimal_digits = significand.split('.').nth(1).unwrap_or("").len();
                    format!("{:.1$e}", f32_value, decimal_digits)
                } else {
                    let part_after_dot_len = str_value.split('.').nth(1).unwrap_or("").len();
                    format!("{:.1$}", f32_value, part_after_dot_len)
                };

                fn normalize(s: &str) -> String {
                    let s = s.to_lowercase();
                    if let Some((base, exp)) = s.split_once('e') {
                        let base = base.trim_end_matches('0').trim_end_matches('.');
                        let exp = exp.trim_start_matches('+');
                        format!("{}e{}", base, exp)
                    } else {
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    }
                }

                let normalized_input = normalize(&str_value);
                let normalized_f32 = normalize(&f32_str);

                // If the f32 representation matches the original string, return as f32
                if normalized_input == normalized_f32 {
                    Ok(ast::float32_literal(f32_value))
                } else {
                    // Otherwise, parse as f64
                    let f64_value = str_value.parse::<f64>().map_err(|_| err.clone())?;
                    if f64_value.is_finite() {
                        Ok(ast::float64_literal(f64_value))
                    } else {
                        Err(err)
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    /*
        StringLiteral
            : DoubleQuotedString
            : SingleQuotedString
            ;
    */
    pub(crate) fn string_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::String) {
            Ok(token) => {
                let mut str_value = &self.source[token.1.start..token.1.end];

                // Strings that come from f-string expressions will have escaped quotes.
                if str_value.starts_with('\\') {
                    str_value = &str_value[2..str_value.len() - 1];
                } else {
                    str_value = &str_value[1..str_value.len() - 1];
                }

                let unescaped = unescape_string(str_value);
                let literal = ast::string_literal(&unescaped);
                Ok(literal)
            }
            Err(e) => Err(e),
        }
    }

    /*
        BooleanLiteral
            : TRUE
            : FALSE
            ;
    */
    pub(crate) fn boolean_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end];
                let literal = match str_value {
                    "true" => ast::boolean(true),
                    "false" => ast::boolean(false),
                    _ => {
                        return Err(SyntaxError::new(
                            SyntaxErrorKind::InvalidBooleanLiteral,
                            Span::new(token.1.start, token.1.end),
                        ));
                    }
                };
                Ok(literal)
            }
            Err(e) => Err(e),
        }
    }

    /*
        RegexLiteral
            : REGEX
            ;
    */
    pub(crate) fn regex_literal(&mut self) -> Result<Literal, SyntaxError> {
        let token_span = self.eat(
            |t| matches!(t, Token::Regex(_)),
            || "regex literal".to_string(),
        )?;
        if let (Token::Regex(regex_data), _) = token_span {
            Ok(ast::regex_literal_from_token(*regex_data))
        } else {
            Err(self.error_unexpected_lookahead_token("regex literal"))
        }
    }
}
