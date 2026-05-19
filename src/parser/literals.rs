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

fn format_with_input_precision(value: f32, input: &str) -> String {
    if input.contains('e') || input.contains('E') {
        let significand = input.split(['e', 'E']).next().unwrap_or("");
        let decimal_digits = significand.split('.').nth(1).unwrap_or("").len();
        format!("{:.1$e}", value, decimal_digits)
    } else {
        let decimal_digits = input.split('.').nth(1).unwrap_or("").len();
        format!("{:.1$}", value, decimal_digits)
    }
}

fn normalize_float_str(s: &str) -> String {
    let s = s.to_lowercase();
    if let Some((base, exp)) = s.split_once('e') {
        let base = base.trim_end_matches('0').trim_end_matches('.');
        let exp = exp.trim_start_matches('+');
        format!("{}e{}", base, exp)
    } else {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
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
        match &self.lookahead {
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
        let token = self.eat_token(token_type)?;
        let span = Span::new(token.1.start, token.1.end);
        let raw = &self.source[token.1.start..token.1.end];
        let str_value = strip_underscores(raw);

        let value = match token_type {
            Token::Int => str_value
                .parse::<i128>()
                .map_err(|_| SyntaxError::new(SyntaxErrorKind::InvalidIntegerLiteral, span))?,
            Token::BinaryNumber => i128::from_str_radix(&str_value[2..], 2)
                .map_err(|_| SyntaxError::new(SyntaxErrorKind::InvalidBinaryLiteral, span))?,
            Token::HexNumber => i128::from_str_radix(&str_value[2..], 16)
                .map_err(|_| SyntaxError::new(SyntaxErrorKind::InvalidHexLiteral, span))?,
            Token::OctalNumber => i128::from_str_radix(&str_value[2..], 8)
                .map_err(|_| SyntaxError::new(SyntaxErrorKind::InvalidOctalLiteral, span))?,
            _ => {
                return Err(SyntaxError::new(
                    SyntaxErrorKind::UnexpectedToken {
                        expected: "integer literal".to_string(),
                        found: format!("{:?}", token_type),
                    },
                    span,
                ));
            }
        };

        Ok(ast::int_literal(value))
    }

    /*
        FloatLiteral
            : FLOAT
            ;
    */
    pub(crate) fn float_literal(&mut self) -> Result<Literal, SyntaxError> {
        let token = self.eat_token(&Token::Float)?;
        let err = SyntaxError::new(
            SyntaxErrorKind::InvalidFloatLiteral,
            Span::new(token.1.start, token.1.end),
        );
        let raw = &self.source[token.1.start..token.1.end];
        let str_value = strip_underscores(raw);
        let f32_value = str_value.parse::<f32>().map_err(|_| err.clone())?;
        let f32_str = format_with_input_precision(f32_value, &str_value);

        if normalize_float_str(&str_value) == normalize_float_str(&f32_str) {
            return Ok(ast::float32_literal(f32_value));
        }

        let f64_value = str_value.parse::<f64>().map_err(|_| err.clone())?;
        if f64_value.is_nan() {
            return Err(err);
        }
        Ok(ast::float64_literal(f64_value))
    }

    /*
        StringLiteral
            : DoubleQuotedString
            : SingleQuotedString
            ;
    */
    pub(crate) fn string_literal(&mut self) -> Result<Literal, SyntaxError> {
        let token = self.eat_token(&Token::String)?;
        let raw = &self.source[token.1.start..token.1.end];

        // Strings that come from f-string expressions arrive with escaped quotes.
        let inner = if raw.starts_with('\\') {
            &raw[2..raw.len() - 1]
        } else {
            &raw[1..raw.len() - 1]
        };

        Ok(ast::string_literal(&unescape_string(inner)))
    }

    /*
        BooleanLiteral
            : TRUE
            : FALSE
            ;
    */
    pub(crate) fn boolean_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        let token = self.eat_token(token_type)?;
        let str_value = &self.source[token.1.start..token.1.end];
        match str_value {
            "true" => Ok(ast::boolean(true)),
            "false" => Ok(ast::boolean(false)),
            _ => Err(SyntaxError::new(
                SyntaxErrorKind::InvalidBooleanLiteral,
                Span::new(token.1.start, token.1.end),
            )),
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
