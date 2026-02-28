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
    pub(crate) fn lambda_expression(&mut self) -> Result<Expression, SyntaxError> {
        let properties = self.function_modifiers(MemberVisibility::Public)?;

        self.eat_token(&Token::Fn)?;

        let generic_types = self.generic_types_expression()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        let body_parsing_error = self.error_unexpected_lookahead_token(
            "a colon for an inline body or an indented block for a block body",
        );
        let body = match &self._lookahead {
            Some((Token::Colon, _)) => {
                self.eat_token(&Token::Colon)?;
                let expr = self.expression()?;
                ast::expression_statement(expr)
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.block_statement()?
                } else if self.lookahead_is_dedent() || self._lookahead.is_none() {
                    ast::empty_statement() // No body, just an expression end
                } else {
                    return Err(body_parsing_error);
                }
            }
            _ => return Err(body_parsing_error),
        };

        Ok(ast::lambda_expression(
            generic_types,
            parameters,
            return_type,
            body,
            properties,
        ))
    }
}
