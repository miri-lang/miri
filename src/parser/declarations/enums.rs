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
    pub(crate) fn enum_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Enum)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
        self.parse_declaration_block(
            Self::enum_value_expression,
            ast::enum_statement,
            name,
            visibility,
            DeclarationBlockConfig {
                inline_error: "either a colon for inline enums or an indentation for block enums",
                missing_members_error: SyntaxErrorKind::MissingEnumMembers,
            },
            generic_types,
        )
    }

}
