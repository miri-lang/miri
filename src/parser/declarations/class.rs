// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::{DeclarationBlockConfig, Parser};

impl<'source> Parser<'source> {
    /// Parses a typed field declaration (e.g., `name int`) for use inside
    /// class bodies and class member statements.
    fn typed_field_declaration(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let name_expr = self.identifier()?;
        let name = if let ExpressionKind::Identifier(n, _) = name_expr.node {
            n
        } else {
            return Err(self.error_unexpected_token("identifier", "expression"));
        };

        let typ = self
            .type_expression()?
            .map(Box::new)
            .ok_or_else(|| self.error_missing_type_expression())?;

        self.eat_statement_end()?;

        let decl = VariableDeclaration {
            name,
            typ: Some(typ),
            initializer: None,
            declaration_type: VariableDeclarationType::Mutable,
            is_shared: false,
        };
        Ok(ast::variable_statement(vec![decl], visibility))
    }

    pub(crate) fn class_member_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let statement = match &self._lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _)) => {
                self.variable_statement(visibility)?
            }
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.function_declaration(visibility)?
            }
            Some((Token::Runtime, _)) => {
                return Err(self.error_unexpected_lookahead_token(
                    "a declaration (runtime functions cannot have visibility modifiers)",
                ));
            }
            Some((Token::Enum, _)) => self.enum_statement(visibility)?,
            Some((Token::Struct, _)) => self.struct_statement(visibility)?,
            Some((Token::Type, _)) => self.type_statement(visibility)?,
            Some((Token::Class, _)) => self.class_statement(visibility)?,
            Some((Token::Trait, _)) => self.trait_statement(visibility)?,
            Some((Token::Abstract, _)) => {
                self.eat_token(&Token::Abstract)?;
                self.abstract_class_statement(visibility)?
            }
            Some((Token::Identifier, _)) => self.typed_field_declaration(visibility)?,
            _ => {
                return Err(self.error_unexpected_lookahead_token(
                    "let, var, const, async, fn, gpu, runtime, enum, type, struct, class, trait, abstract or field declaration",
                ));
            }
        };
        Ok(statement)
    }

    pub(crate) fn parse_declaration_block<F, C>(
        &mut self,
        item_parser: F,
        creator: C,
        name: Expression,
        visibility: MemberVisibility,
        config: DeclarationBlockConfig,
        generic_types: Option<Vec<Expression>>,
    ) -> Result<Statement, SyntaxError>
    where
        F: Fn(&mut Self) -> Result<Expression, SyntaxError>,
        C: Fn(Expression, Option<Vec<Expression>>, Vec<Expression>, MemberVisibility) -> Statement,
    {
        let mut items = vec![];

        match &self._lookahead {
            Some((Token::Colon, _)) => {
                // Inline form
                self.eat_token(&Token::Colon)?;
                if !self.lookahead_is_expression_end() && self._lookahead.is_some() {
                    items.push(item_parser(self)?);
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        items.push(item_parser(self)?);
                    }
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                // Block form
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.eat_token(&Token::Indent)?;
                    while !self.lookahead_is_dedent() {
                        items.push(item_parser(self)?);
                        self.try_eat_expression_end();
                    }
                    self.eat_token(&Token::Dedent)?;
                }
            }
            _ => return Err(self.error_unexpected_lookahead_token(config.inline_error)),
        };

        if items.is_empty() {
            return Err(self.error_missing_members(config.missing_members_error));
        }

        Ok(creator(name, generic_types, items, visibility))
    }

    /*
    */
    pub(crate) fn class_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Class)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;

        // Parse optional 'extends' clause (single base class)
        let base_class = if self.match_lookahead_type(|t| t == &Token::Extends) {
            self.eat_token(&Token::Extends)?;
            Some(Box::new(self.inheritance_identifier()?))
        } else {
            None
        };

        // Parse optional 'implements' clause (multiple traits)
        let traits = if self.match_lookahead_type(|t| t == &Token::Implements) {
            self.eat_token(&Token::Implements)?;
            let mut trait_list = vec![self.inheritance_identifier()?];
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                // Stop if we hit expression end (for trailing comma support)
                if self.lookahead_is_expression_end() {
                    break;
                }
                trait_list.push(self.inheritance_identifier()?);
            }
            trait_list
        } else {
            vec![]
        };

        // Parse class body (block only, no inline syntax)
        let body = self.class_body()?;

        Ok(ast::class_statement(
            name,
            generic_types,
            base_class,
            traits,
            body,
            visibility,
        ))
    }

    /*
    */
    pub(crate) fn abstract_class_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        // Note: 'abstract' token should already be consumed by caller
        self.eat_token(&Token::Class)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;

        // Parse optional 'extends' clause (single base class)
        let base_class = if self.match_lookahead_type(|t| t == &Token::Extends) {
            self.eat_token(&Token::Extends)?;
            Some(Box::new(self.inheritance_identifier()?))
        } else {
            None
        };

        // Parse optional 'implements' clause (multiple traits)
        let traits = if self.match_lookahead_type(|t| t == &Token::Implements) {
            self.eat_token(&Token::Implements)?;
            let mut trait_list = vec![self.inheritance_identifier()?];
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                // Stop if we hit expression end (for trailing comma support)
                if self.lookahead_is_expression_end() {
                    break;
                }
                trait_list.push(self.inheritance_identifier()?);
            }
            trait_list
        } else {
            vec![]
        };

        // Parse abstract class body (block only) - abstract classes allow abstract functions
        let body = self.class_body_with_abstract(true)?;

        Ok(ast::abstract_class_statement(
            name,
            generic_types,
            base_class,
            traits,
            body,
            visibility,
        ))
    }

    /*
    */
    pub(crate) fn trait_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Trait)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;

        // Parse optional 'extends' clause (multiple parent traits for traits)
        let parent_traits = if self.match_lookahead_type(|t| t == &Token::Extends) {
            self.eat_token(&Token::Extends)?;
            let mut trait_list = vec![self.inheritance_identifier()?];
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.lookahead_is_expression_end() {
                    break;
                }
                trait_list.push(self.inheritance_identifier()?);
            }
            trait_list
        } else {
            vec![]
        };

        // Parse trait body (block only) - traits allow abstract functions
        let body = self.class_body_with_abstract(true)?;

        Ok(ast::trait_statement(
            name,
            generic_types,
            parent_traits,
            body,
            visibility,
        ))
    }

    /// Parses a class or trait body block containing fields and methods.
    fn class_body(&mut self) -> Result<Vec<Statement>, SyntaxError> {
        self.class_body_with_abstract(false)
    }

    /// Parses a class or trait body block containing fields and methods.
    /// If allow_abstract is true, functions without bodies are allowed.
    fn class_body_with_abstract(
        &mut self,
        allow_abstract: bool,
    ) -> Result<Vec<Statement>, SyntaxError> {
        // Expect expression end then indented block
        self.eat_expression_end()?;

        if !self.lookahead_is_indent() {
            // Empty body - return empty vec (will be validated elsewhere)
            return Ok(vec![]);
        }

        self.eat_token(&Token::Indent)?;
        let mut statements = vec![];

        while !self.lookahead_is_dedent() && self._lookahead.is_some() {
            let stmt = self.class_member_or_statement_with_abstract(allow_abstract)?;
            statements.push(stmt);
            self.try_eat_expression_end();
        }

        self.eat_token(&Token::Dedent)?;
        Ok(statements)
    }

    /// Parses a class member (field or method) with optional visibility modifier.
    /// If `allow_abstract` is true, functions without bodies are treated as
    /// abstract declarations rather than producing an error.
    fn class_member_or_statement_with_abstract(
        &mut self,
        allow_abstract: bool,
    ) -> Result<Statement, SyntaxError> {
        // Check for visibility modifiers
        let visibility = match &self._lookahead {
            Some((Token::Public, _)) => {
                self.eat_token(&Token::Public)?;
                MemberVisibility::Public
            }
            Some((Token::Protected, _)) => {
                self.eat_token(&Token::Protected)?;
                MemberVisibility::Protected
            }
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                MemberVisibility::Private
            }
            _ => MemberVisibility::Private, // Default visibility is private
        };

        // Parse based on token
        match &self._lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _)) => {
                self.variable_statement(visibility)
            }
            Some((Token::Async, _))
            | Some((Token::Fn, _))
            | Some((Token::Gpu, _))
            | Some((Token::Parallel, _)) => {
                self.function_declaration_with_context(visibility, allow_abstract)
            }
            Some((Token::Type, _)) => self.type_statement(visibility),
            Some((Token::Runtime, _)) => self.runtime_function_declaration(),
            Some((Token::Identifier, _)) => self.typed_field_declaration(visibility),
            _ => Err(self.error_unexpected_lookahead_token(
                "class member (let, var, const, fn, async, gpu, type, runtime, or field declaration)",
            )),
        }
    }

    pub(crate) fn inheritance_identifier(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        match name.node {
            ExpressionKind::Identifier(_, Some(_)) => {
                Err(self.error_invalid_inheritance_identifier())
            }
            ExpressionKind::Identifier(_, None) => Ok(name),
            _ => Err(self.error_unexpected_token("identifier", format!("{:?}", name).as_str())),
        }
    }

}
