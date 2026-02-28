// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::super::{DeclarationBlockConfig, Parser};

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn struct_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Struct)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
        self.parse_declaration_block(
            Self::struct_member_expression,
            ast::struct_statement,
            name,
            visibility,
            DeclarationBlockConfig {
                inline_error:
                    "either a colon for inline structs or an indentation for block structs",
                missing_members_error: SyntaxErrorKind::MissingStructMembers,
            },
            generic_types,
        )
    }

    /*
     */
    pub(crate) fn struct_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let typ = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_struct_member_type())?;
        Ok(ast::struct_member_expression(name, typ))
    }
}
