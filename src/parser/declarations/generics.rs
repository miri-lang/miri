// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::super::utils::is_inheritance_modifier;
use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn generic_types_declaration(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(&Token::LessThan)?;

        let mut types = vec![self.generic_type()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            let generic = self.generic_type()?;
            refuse_repeated_type_parameter(&types, &generic)?;
            types.push(generic);
        }

        self.eat_token(&Token::GreaterThan)?;
        Ok(types)
    }

    pub(crate) fn generic_type(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        if self.match_lookahead_type(|t| t == &Token::Is) {
            self.eat_token(&Token::Is)?;
            let set = self.type_set_expression()?.map(Box::new);
            return Ok(ast::generic_type_expression(
                identifier,
                set,
                TypeDeclarationKind::Is,
            ));
        }
        if self.lookahead.is_none() || !self.lookahead_is_inheritance_modifier() {
            return Ok(ast::generic_type_expression(
                identifier,
                None,
                TypeDeclarationKind::None,
            ));
        }

        let token_span = self.eat(is_inheritance_modifier, || {
            "extends, includes or implements".to_string()
        })?;
        let kind = match token_span.0 {
            Token::Extends => TypeDeclarationKind::Extends,
            Token::Implements => TypeDeclarationKind::Implements,
            Token::Includes => TypeDeclarationKind::Includes,
            _ => TypeDeclarationKind::None,
        };

        let typ = self.type_expression()?.map(Box::new);

        Ok(ast::generic_type_expression(identifier, typ, kind))
    }
}

/// The name a generic parameter declares, with where it is written.
fn type_parameter_name(generic: &Expression) -> Option<(&str, Span)> {
    let ExpressionKind::GenericType(name, _, _) = &generic.node else {
        return None;
    };
    match &name.node {
        ExpressionKind::Identifier(name_text, _) => Some((name_text.as_str(), name.span)),
        _ => None,
    }
}

/// Refuses `generic` when an earlier parameter of its list declares its name:
/// the declaration could only ever mean one of the two.
fn refuse_repeated_type_parameter(
    earlier: &[Expression],
    generic: &Expression,
) -> Result<(), SyntaxError> {
    let Some((name, span)) = type_parameter_name(generic) else {
        return Ok(());
    };
    let repeated = earlier
        .iter()
        .filter_map(type_parameter_name)
        .any(|(other, _)| other == name);
    if !repeated {
        return Ok(());
    }
    Err(SyntaxError::new(
        SyntaxErrorKind::DuplicateTypeParameter {
            name: name.to_string(),
        },
        span,
    ))
}
