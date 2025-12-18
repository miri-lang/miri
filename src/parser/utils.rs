// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::lexer::{token_to_string, Token, TokenSpan};
use crate::syntax_error::{Span, SyntaxError, SyntaxErrorKind};

use super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn eat(
        &mut self,
        expected: impl Fn(&Token) -> bool,
        expected_str: &str,
    ) -> Result<TokenSpan, SyntaxError> {
        let token = &self._lookahead;

        match token {
            Some((ref t, ref span)) if expected(t) => {
                let result = (t.clone(), span.clone());
                self._lookahead = self.lexer.next().transpose()?;
                Ok(result)
            }
            Some((found, _)) => Err(SyntaxError::new(
                SyntaxErrorKind::UnexpectedToken {
                    expected: expected_str.to_string(),
                    found: token_to_string(found),
                },
                self.source.len()..self.source.len(),
            )),
            None => {
                if expected(&Token::ExpressionStatementEnd) {
                    // Special case for end of expression
                    self._lookahead = None;
                    return Ok((Token::ExpressionStatementEnd, 0..0));
                }

                Err(self.error_eof())
            }
        }
    }

    pub(crate) fn eat_token(&mut self, expected: &Token) -> Result<TokenSpan, SyntaxError> {
        self.eat(|t| t == expected, &token_to_string(expected))
    }

    pub(crate) fn eat_binary_op(
        &mut self,
        match_token: fn(&Token) -> bool,
    ) -> Result<TokenSpan, SyntaxError> {
        self.eat(match_token, "binary operator")
    }

    pub(crate) fn match_lookahead_type(&self, match_token: fn(&Token) -> bool) -> bool {
        if let Some((token, _)) = &self._lookahead {
            match_token(token)
        } else {
            false
        }
    }

    pub(crate) fn lookahead_is_assignment_op(&self) -> bool {
        self.match_lookahead_type(is_assignment_op)
    }

    pub(crate) fn lookahead_is_literal(&self) -> bool {
        self.match_lookahead_type(is_literal)
    }

    pub(crate) fn lookahead_is_colon(&self) -> bool {
        self.match_lookahead_type(is_colon)
    }

    pub(crate) fn lookahead_is_comma(&self) -> bool {
        self.match_lookahead_type(is_comma)
    }

    pub(crate) fn lookahead_is_expression_end(&self) -> bool {
        self.match_lookahead_type(is_expression_end)
    }

    pub(crate) fn lookahead_is_else(&self) -> bool {
        self.match_lookahead_type(is_else)
    }

    pub(crate) fn lookahead_is_indent(&self) -> bool {
        self.match_lookahead_type(is_indent)
    }

    pub(crate) fn lookahead_is_dedent(&self) -> bool {
        self.match_lookahead_type(is_dedent)
    }

    pub(crate) fn lookahead_as_string(&self) -> String {
        self._lookahead
            .as_ref()
            .map_or("end of file".to_string(), |(t, _)| token_to_string(t))
    }

    pub(crate) fn lookahead_is_guard(&self) -> bool {
        self.match_lookahead_type(is_guard)
    }

    pub(crate) fn lookahead_is_in(&self) -> bool {
        self.match_lookahead_type(is_in)
    }

    pub(crate) fn lookahead_is_rparen(&self) -> bool {
        self.match_lookahead_type(is_rparen)
    }

    pub(crate) fn lookahead_is_less_than(&self) -> bool {
        self.match_lookahead_type(is_less_than)
    }

    pub(crate) fn lookahead_is_member_expression_boundary(&self) -> bool {
        self.match_lookahead_type(is_member_expression_boundary)
    }

    pub(crate) fn lookahead_is_inheritance_modifier(&self) -> bool {
        self.match_lookahead_type(is_inheritance_modifier)
    }

    pub(crate) fn lookahead_is_function_modifier(&self) -> bool {
        self.match_lookahead_type(is_function_modifier)
    }

    pub(crate) fn eat_additive_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_additive_op) {
            Ok(token) => match token.0 {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                Token::Pipe => BinaryOp::BitwiseOr,
                Token::Ampersand => BinaryOp::BitwiseAnd,
                Token::Caret => BinaryOp::BitwiseXor,
                _ => return Err(Err(self.error_unexpected_operator(token, "+, -, |, &, ^"))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    pub(crate) fn eat_relational_op(
        &mut self,
    ) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_relational_op) {
            Ok(token) => match token.0 {
                Token::LessThan => BinaryOp::LessThan,
                Token::LessThanEqual => BinaryOp::LessThanEqual,
                Token::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                Token::GreaterThan => BinaryOp::GreaterThan,
                _ => return Err(Err(self.error_unexpected_operator(token, "<, <=, >, >="))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    pub(crate) fn eat_equality_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_equality_op) {
            Ok(token) => match token.0 {
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                _ => return Err(Err(self.error_unexpected_operator(token, "=, !="))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    pub(crate) fn eat_logical_and_op(
        &mut self,
    ) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_and_op) {
            Ok(token) => match token.0 {
                Token::And => BinaryOp::And,
                _ => return Err(Err(self.error_unexpected_operator(token, "and"))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    pub(crate) fn eat_logical_or_op(
        &mut self,
    ) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_or_op) {
            Ok(token) => match token.0 {
                Token::Or => BinaryOp::Or,
                _ => return Err(Err(self.error_unexpected_operator(token, "or"))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    pub(crate) fn eat_multiplicative_op(
        &mut self,
    ) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_multiplicative_op) {
            Ok(token) => match token.0 {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => return Err(Err(self.error_unexpected_operator(token, "*, /, %"))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    pub(crate) fn eat_expression_end(&mut self) -> Result<TokenSpan, SyntaxError> {
        self.eat_token(&Token::ExpressionStatementEnd)
    }

    pub(crate) fn try_eat_expression_end(&mut self) {
        if self.lookahead_is_expression_end() {
            let _ = self.eat_expression_end();
        }
    }

    pub(crate) fn eat_statement_end(&mut self) -> Result<(), SyntaxError> {
        // A statement must be followed by a token that can validly end it.
        // This includes a newline, the end of the file, the end of a block (Dedent),
        // or a keyword that starts a new clause (like `else`).
        match &self._lookahead {
            // Valid terminators that we consume.
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
            }
            // Valid terminators that we DON'T consume, as they belong to other parsers.
            None
            | Some((Token::Dedent, _))
            | Some((Token::Else, _))
            | Some((Token::While, _))
            | Some((Token::Until, _)) => {
                // Do nothing, the token is a valid boundary.
            }
            // Anything else is an error.
            _ => {
                return Err(self.error_unexpected_lookahead_token("an end of statement"));
            }
        }
        Ok(())
    }

    pub(crate) fn error_unexpected_operator(
        &self,
        token: TokenSpan,
        expected: &str,
    ) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: self.lookahead_as_string(),
            },
            token.1.start..token.1.end,
        )
    }

    pub(crate) fn error_unexpected_token(&self, expected: &str, found: &str) -> SyntaxError {
        self.error_unexpected_token_with_span(expected, found, self.source.len()..self.source.len())
    }

    pub(crate) fn error_invalid_inheritance_identifier(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidInheritanceIdentifier,
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_unexpected_token_with_span(
        &self,
        expected: &str,
        found: &str,
        span: Span,
    ) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: found.to_string(),
            },
            span,
        )
    }

    pub(crate) fn error_unexpected_lookahead_token(&self, expected: &str) -> SyntaxError {
        self.error_unexpected_token(expected, &self.lookahead_as_string())
    }

    pub(crate) fn error_missing_match_branches(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingMatchBranches,
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_duplicate_match_pattern(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::DuplicateMatchPattern,
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_eof(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedEOF,
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_invalid_left_hand_side_expression(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidLeftHandSideExpression,
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_invalid_type_declaration(&self, expected: &str) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidTypeDeclaration {
                expected: expected.to_string(),
            },
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_missing_struct_member_type(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingStructMemberType,
            self.source.len()..self.source.len(),
        )
    }

    pub(crate) fn error_missing_members(&self, kind: SyntaxErrorKind) -> SyntaxError {
        SyntaxError::new(kind, self.source.len()..self.source.len())
    }

    pub(crate) fn error_missing_type_expression(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingTypeExpression,
            self.source.len()..self.source.len(),
        )
    }
}

pub(crate) fn is_additive_op(token: &Token) -> bool {
    matches!(
        token,
        Token::Plus | Token::Minus | Token::Pipe | Token::Ampersand | Token::Caret
    )
}

pub(crate) fn is_relational_op(token: &Token) -> bool {
    matches!(
        token,
        Token::LessThan | Token::LessThanEqual | Token::GreaterThanEqual | Token::GreaterThan
    )
}

pub(crate) fn is_equality_op(token: &Token) -> bool {
    matches!(token, Token::Equal | Token::NotEqual)
}

pub(crate) fn is_logical_and_op(token: &Token) -> bool {
    matches!(token, Token::And)
}

pub(crate) fn is_logical_or_op(token: &Token) -> bool {
    matches!(token, Token::Or)
}

pub(crate) fn is_multiplicative_op(token: &Token) -> bool {
    matches!(token, Token::Star | Token::Slash | Token::Percent)
}

pub(crate) fn is_assignment_op(token: &Token) -> bool {
    matches!(
        token,
        Token::Assign
            | Token::AssignAdd
            | Token::AssignSub
            | Token::AssignMul
            | Token::AssignDiv
            | Token::AssignMod
    )
}

pub(crate) fn is_literal(token: &Token) -> bool {
    matches!(
        token,
        Token::Int
            | Token::BinaryNumber
            | Token::HexNumber
            | Token::OctalNumber
            | Token::Float
            | Token::True
            | Token::False
            | Token::String
            | Token::Symbol
            | Token::Regex(_)
            | Token::None
    )
}

pub(crate) fn is_colon(token: &Token) -> bool {
    matches!(token, Token::Colon)
}

pub(crate) fn is_comma(token: &Token) -> bool {
    matches!(token, Token::Comma)
}

pub(crate) fn is_expression_end(token: &Token) -> bool {
    matches!(token, Token::ExpressionStatementEnd)
}

pub(crate) fn is_else(token: &Token) -> bool {
    matches!(token, Token::Else)
}

pub(crate) fn is_indent(token: &Token) -> bool {
    matches!(token, Token::Indent)
}

pub(crate) fn is_dedent(token: &Token) -> bool {
    matches!(token, Token::Dedent)
}

pub(crate) fn is_guard(token: &Token) -> bool {
    matches!(
        token,
        Token::GreaterThan
            | Token::GreaterThanEqual
            | Token::LessThan
            | Token::LessThanEqual
            | Token::In
            | Token::Not
            | Token::NotEqual
    )
}

pub(crate) fn is_in(token: &Token) -> bool {
    matches!(token, Token::In)
}

pub(crate) fn is_rparen(token: &Token) -> bool {
    matches!(token, Token::RParen)
}

pub(crate) fn is_less_than(token: &Token) -> bool {
    matches!(token, Token::LessThan)
}

pub(crate) fn is_member_expression_boundary(token: &Token) -> bool {
    matches!(token, Token::LBracket | Token::Dot | Token::LParen)
}

pub(crate) fn is_inheritance_modifier(token: &Token) -> bool {
    matches!(token, Token::Extends | Token::Includes | Token::Implements)
}

pub(crate) fn is_function_modifier(token: &Token) -> bool {
    matches!(token, Token::Async | Token::Gpu)
}
