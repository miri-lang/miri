// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn lambda_expression(&mut self) -> Result<Expression, SyntaxError> {
        let properties = self.function_modifiers(MemberVisibility::Public)?;

        self.eat_token(&Token::Fn)?;

        let generic_types = self.generic_types_expression()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        let body_parsing_error = self.error_unexpected_lookahead_token(
            "a body after ':' on this line, or an indented block on the lines below",
        );
        let body = match &self.lookahead {
            Some((Token::Colon, _)) => {
                self.eat_token(&Token::Colon)?;
                // A colon takes the body on this line or an indented block on
                // the lines below, the same two spellings every other block
                // header accepts.
                if self.lookahead_is_expression_end() {
                    self.indented_lambda_body(body_parsing_error)?
                } else {
                    ast::expression_statement(self.expression()?)
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                self.indented_lambda_body(body_parsing_error)?
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

    /// The block that opens on the line after an anonymous function's
    /// signature, or the empty body of one that has none.
    ///
    /// Reached from both spellings of the signature, so `fn() int:` and a bare
    /// `fn() int` produce the same body.
    fn indented_lambda_body(&mut self, body_error: SyntaxError) -> Result<Statement, SyntaxError> {
        self.eat_expression_end()?;
        if self.lookahead_is_indent() {
            self.block_statement()
        } else if self.lookahead_is_dedent() || self.lookahead.is_none() {
            Ok(ast::empty_statement())
        } else {
            Err(body_error)
        }
    }
}
