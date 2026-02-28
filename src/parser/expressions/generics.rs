// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn generic_types_expression(
        &mut self,
    ) -> Result<Option<Vec<Expression>>, SyntaxError> {
        let generic_types = if self.lookahead_is_less_than() {
            Some(self.generic_types_declaration()?)
        } else {
            None
        };
        Ok(generic_types)
    }

    pub(crate) fn function_params_expression(&mut self) -> Result<Vec<Parameter>, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let parameters = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.parameter_list()?
        };
        self.eat_token(&Token::RParen)?;

        Ok(parameters)
    }

    pub(crate) fn return_type_expression(
        &mut self,
    ) -> Result<Option<Box<Expression>>, SyntaxError> {
        let return_type = self.type_expression()?.map(Box::new);
        Ok(return_type)
    }
}
