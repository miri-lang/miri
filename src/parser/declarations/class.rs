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
        let name_span = name_expr.span;
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
            name_span,
            typ: Some(typ),
            initializer: None,
            declaration_type: VariableDeclarationType::Unmarked,
            is_shared: false,
            residency: crate::ast::statement::BindingResidency::Host,
        };
        Ok(ast::variable_statement(vec![decl], visibility))
    }

    pub(crate) fn class_member_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let attributes = self.attributes()?;

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
                self.enum_statement(visibility, Vec::new())?
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
        self.attach_attributes(statement, attributes)
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

    pub(crate) fn inheritance_clause(
        &mut self,
        keyword: &Token,
    ) -> Result<Vec<Expression>, SyntaxError> {
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
            // A member is parsed here rather than through `statement`, so the
            // comments written around it are claimed here too.
            let leading = self.lexer.take_leading_comments();
            let mut member = self.class_member(mode)?;
            self.claim_trivia(&mut member, leading);
            statements.push(member);
            self.try_eat_expression_end()?;
        }

        self.eat_token(&Token::Dedent)?;
        Ok(statements)
    }

    fn class_member(&mut self, mode: BodyMode) -> Result<Statement, SyntaxError> {
        let attributes = self.attributes()?;
        let member = self.class_member_declaration(mode)?;
        self.attach_attributes(member, attributes)
    }

    fn class_member_declaration(&mut self, mode: BodyMode) -> Result<Statement, SyntaxError> {
        let (visibility, wrote_public) = self.member_visibility()?;
        let is_abstract_method = self.try_eat_abstract_modifier()?;
        let mut member = self.class_member_after_modifiers(visibility, mode, is_abstract_method)?;
        member.trivia.written_modifiers.public = wrote_public;
        member.trivia.written_modifiers.is_abstract = is_abstract_method;
        Ok(member)
    }

    fn class_member_after_modifiers(
        &mut self,
        visibility: MemberVisibility,
        mode: BodyMode,
        is_abstract_method: bool,
    ) -> Result<Statement, SyntaxError> {
        let effective_mode = if is_abstract_method {
            BodyMode::Optional
        } else {
            mode
        };

        if self.is_static_fn_pattern() {
            self.reject_abstract(is_abstract_method, "static method cannot be abstract")?;
            return self.static_method_declaration(visibility, effective_mode);
        }

        match &self.lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _)) => {
                self.reject_abstract(is_abstract_method, "variable declaration after 'abstract'")?;
                self.variable_statement(visibility)
            }
            Some((Token::Async, _))
            | Some((Token::Fn, _))
            | Some((Token::Gpu, _))
            | Some((Token::Parallel, _)) => {
                self.function_declaration_with_mode(visibility, effective_mode)
            }
            Some((Token::Type, _)) => {
                self.reject_abstract(is_abstract_method, "type declaration after 'abstract'")?;
                self.type_statement(visibility)
            }
            Some((Token::Runtime, _)) => {
                self.reject_abstract(is_abstract_method, "runtime function after 'abstract'")?;
                self.runtime_function_declaration()
            }
            Some((Token::Identifier, _)) => {
                self.reject_abstract(is_abstract_method, "field declaration after 'abstract'")?;
                self.typed_field_declaration(visibility)
            }
            _ => Err(self.error_unexpected_lookahead_token(
                "class member (let, var, const, fn, async, gpu, type, runtime, or field declaration)",
            )),
        }
    }

    /// Refuse `abstract` in front of a member that cannot be one.
    ///
    /// `abstract` says a method has no body here and is supplied elsewhere.
    /// Nothing else a class declares can be supplied elsewhere, so the keyword
    /// in front of one is a mistake rather than a shorthand.
    fn reject_abstract(&self, is_abstract_method: bool, found: &str) -> Result<(), SyntaxError> {
        if is_abstract_method {
            return Err(self.error_unexpected_token("method declaration", found));
        }
        Ok(())
    }

    /// `static fn name(...)`, whose `static` is a contextual keyword rather
    /// than a token, so the modifiers are collected here instead.
    fn static_method_declaration(
        &mut self,
        visibility: MemberVisibility,
        mode: BodyMode,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Identifier)?;

        let mut properties = crate::ast::common::FunctionProperties {
            is_async: false,
            is_parallel: false,
            is_gpu: false,
            is_static: true,
            visibility,
        };
        self.continue_function_modifiers(&mut properties)?;
        self.function_declaration_after_modifiers(mode, properties)
    }

    /// The visibility a member declares, and whether it wrote `public`.
    ///
    /// The two are not the same question: `public` is the default, so a member
    /// that wrote it and a member that wrote nothing are equally public. Only
    /// the formatter cares which, and only so it can give the keyword back.
    fn member_visibility(&mut self) -> Result<(MemberVisibility, bool), SyntaxError> {
        match &self.lookahead {
            Some((Token::Public, _)) => {
                self.eat_token(&Token::Public)?;
                Ok((MemberVisibility::Public, true))
            }
            Some((Token::Protected, _)) => {
                self.eat_token(&Token::Protected)?;
                Ok((MemberVisibility::Protected, false))
            }
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                Ok((MemberVisibility::Private, false))
            }
            _ => Ok((MemberVisibility::Public, false)),
        }
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

    /// Checks if the lookahead is the contextual keyword "static" followed by "fn".
    /// Used to disambiguate `static fn` from a field named `static`.
    fn is_static_fn_pattern(&self) -> bool {
        if let Some((Token::Identifier, span)) = &self.lookahead {
            &self.source[span.start..span.end] == "static"
        } else {
            false
        }
    }
}
