// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::super::Parser;

/// Whether a function body is required (concrete class) or optional (trait/abstract class).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyMode {
    Required,
    Optional,
}

struct ClassHeader {
    name: Expression,
    generics: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
}

impl<'source> Parser<'source> {
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
            residency: crate::ast::statement::BindingResidency::Host,
        };
        Ok(ast::variable_statement(vec![decl], visibility))
    }

    pub(crate) fn class_member_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let statement = match &self.lookahead {
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
            Some((Token::Intrinsic, _)) => self.intrinsic_function_declaration(visibility)?,
            Some((Token::Enum, _)) | Some((Token::MustUse, _)) => {
                self.enum_statement(visibility)?
            }
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

    pub(crate) fn class_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Class)?;
        let header = self.class_header()?;
        let body = self.class_body(BodyMode::Required)?;

        Ok(ast::class_statement(
            header.name,
            header.generics,
            header.base_class,
            header.traits,
            body,
            visibility,
        ))
    }

    pub(crate) fn abstract_class_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        // The `abstract` keyword is consumed by the caller.
        self.eat_token(&Token::Class)?;
        let header = self.class_header()?;
        let body = self.class_body(BodyMode::Optional)?;

        Ok(ast::abstract_class_statement(
            header.name,
            header.generics,
            header.base_class,
            header.traits,
            body,
            visibility,
        ))
    }

    pub(crate) fn trait_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Trait)?;
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
        let parent_traits = self.inheritance_clause(&Token::Extends)?;
        let body = self.class_body(BodyMode::Optional)?;

        Ok(ast::trait_statement(
            name,
            generic_types,
            parent_traits,
            body,
            visibility,
        ))
    }

    fn class_header(&mut self) -> Result<ClassHeader, SyntaxError> {
        let name = self.identifier()?;
        let generics = self.generic_types_expression()?;

        let base_class = if self.match_lookahead_type(|t| t == &Token::Extends) {
            self.eat_token(&Token::Extends)?;
            Some(Box::new(self.inheritance_identifier()?))
        } else {
            None
        };

        let traits = self.inheritance_clause(&Token::Implements)?;

        Ok(ClassHeader {
            name,
            generics,
            base_class,
            traits,
        })
    }

    fn inheritance_clause(&mut self, keyword: &Token) -> Result<Vec<Expression>, SyntaxError> {
        let matches = matches!(&self.lookahead, Some((t, _)) if t == keyword);
        if !matches {
            return Ok(vec![]);
        }
        self.eat_token(keyword)?;

        let mut list = vec![self.inheritance_identifier()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            if self.lookahead_is_expression_end() {
                break;
            }
            list.push(self.inheritance_identifier()?);
        }
        Ok(list)
    }

    fn class_body(&mut self, mode: BodyMode) -> Result<Vec<Statement>, SyntaxError> {
        self.eat_expression_end()?;

        if !self.lookahead_is_indent() {
            return Ok(vec![]);
        }

        self.eat_token(&Token::Indent)?;
        let mut statements = vec![];

        while !self.lookahead_is_dedent() && self.lookahead.is_some() {
            let stmt = self.class_member(mode)?;
            statements.push(stmt);
            self.try_eat_expression_end()?;
        }

        self.eat_token(&Token::Dedent)?;
        Ok(statements)
    }

    fn class_member(&mut self, mode: BodyMode) -> Result<Statement, SyntaxError> {
        let visibility = self.member_visibility()?;
        let is_abstract_method = self.try_eat_abstract_modifier()?;
        let effective_mode = if is_abstract_method {
            BodyMode::Optional
        } else {
            mode
        };

        match &self.lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _)) => {
                if is_abstract_method {
                    return Err(self.error_unexpected_token(
                        "method declaration",
                        "variable declaration after 'abstract'",
                    ));
                }
                self.variable_statement(visibility)
            }
            Some((Token::Async, _))
            | Some((Token::Fn, _))
            | Some((Token::Gpu, _))
            | Some((Token::Parallel, _)) => {
                self.function_declaration_with_mode(visibility, effective_mode)
            }
            Some((Token::Type, _)) => {
                if is_abstract_method {
                    return Err(self.error_unexpected_token(
                        "method declaration",
                        "type declaration after 'abstract'",
                    ));
                }
                self.type_statement(visibility)
            }
            Some((Token::Runtime, _)) => {
                if is_abstract_method {
                    return Err(self.error_unexpected_token(
                        "method declaration",
                        "runtime function after 'abstract'",
                    ));
                }
                self.runtime_function_declaration()
            }
            Some((Token::Identifier, _)) => {
                if is_abstract_method {
                    return Err(self.error_unexpected_token(
                        "method declaration",
                        "field declaration after 'abstract'",
                    ));
                }
                self.typed_field_declaration(visibility)
            }
            _ => Err(self.error_unexpected_lookahead_token(
                "class member (let, var, const, fn, async, gpu, type, runtime, or field declaration)",
            )),
        }
    }

    fn member_visibility(&mut self) -> Result<MemberVisibility, SyntaxError> {
        let visibility = match &self.lookahead {
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
            _ => MemberVisibility::Public,
        };
        Ok(visibility)
    }

    fn try_eat_abstract_modifier(&mut self) -> Result<bool, SyntaxError> {
        if self.match_lookahead_type(|t| t == &Token::Abstract) {
            self.eat_token(&Token::Abstract)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub(crate) fn inheritance_identifier(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        match &name.node {
            ExpressionKind::Identifier(_, Some(_)) => {
                Err(self.error_invalid_inheritance_identifier())
            }
            ExpressionKind::Identifier(_, None) => {
                // Generic args here are type arguments (e.g. `Iterable<List<int>>`),
                // not generic-parameter declarations, so parse them as type
                // expressions to preserve nested generics.
                if self.lookahead_is_less_than() {
                    let generic_args = self.multiple_element_type_expressions(
                        "trait argument",
                        &Token::LessThan,
                        &Token::GreaterThan,
                    )?;
                    let span = name.span;
                    Ok(ast::expr_with_span(
                        ExpressionKind::TypeDeclaration(
                            Box::new(name),
                            Some(generic_args),
                            crate::ast::types::TypeDeclarationKind::None,
                            None,
                        ),
                        span,
                    ))
                } else {
                    Ok(name)
                }
            }
            _ => Err(self.error_unexpected_token("identifier", format!("{:?}", name).as_str())),
        }
    }
}
