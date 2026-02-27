// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
    */
    pub(crate) fn range_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.additive_expression()?;

        match &self._lookahead {
            Some((Token::Range, _)) => {
                self.eat_token(&Token::Range)?;
                let end = self.additive_expression()?;
                Ok(ast::range(
                    start,
                    Some(Box::new(end)),
                    RangeExpressionType::Exclusive,
                ))
            }
            Some((Token::RangeInclusive, _)) => {
                self.eat_token(&Token::RangeInclusive)?;
                let end = self.additive_expression()?;
                Ok(ast::range(
                    start,
                    Some(Box::new(end)),
                    RangeExpressionType::Inclusive,
                ))
            }
            _ => Ok(start),
        }
    }

}
