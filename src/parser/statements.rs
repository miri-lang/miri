// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::foreign_syntax::ForeignForm;
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::literals::unescape_string;
use super::Parser;

/// The mutability marker other languages place after `let`. Miri has no such
/// keyword — `var` carries the meaning — so a binding named `mut` and followed
/// by another identifier is that spelling rather than a name and its type.
const MUT_BINDING: &str = "mut";

impl<'source> Parser<'source> {
    /// Parses a sequence of attributes at the start of a statement.
    /// Each attribute is on its own line and starts with `@`.
    /// Returns an empty Vec if no attributes are present.
    pub(crate) fn attributes(&mut self) -> Result<Vec<Attribute>, SyntaxError> {
        let mut attrs = Vec::new();

        while self.match_lookahead_type(|t| t == &Token::At) {
            let Some((_, marker_span)) = self.lookahead else {
                return Err(self.error_unexpected_lookahead_token("an attribute"));
            };
            self.eat_token(&Token::At)?;

            let name = self.attribute_name()?;
            let argument = self.attribute_argument()?;

            attrs.push(Attribute::new(
                name,
                argument,
                marker_span,
                AttributeSpelling::Attribute,
            ));

            if !self.lookahead_is_expression_end() {
                break;
            }
            self.try_eat_expression_end()?;
        }

        Ok(attrs)
    }

    /// The identifier following `@`. Keywords that name a registered attribute
    /// are accepted here so `@must_use` spells the same as the keyword it replaces.
    fn attribute_name(&mut self) -> Result<String, SyntaxError> {
        let Some((token, span)) = self.lookahead.clone() else {
            return Err(self.error_unexpected_lookahead_token("attribute name"));
        };
        match token {
            Token::Identifier | Token::MustUse => {
                self.eat_token(&token)?;
            }
            _ => return Err(self.error_unexpected_lookahead_token("attribute name")),
        }
        Ok(self.source[span.start..span.end].to_string())
    }

    /// Records attributes on the declaration they precede.
    ///
    /// Only enums, functions and classes carry attributes today. Any other
    /// statement is rejected here rather than silently dropping them, so an
    /// attribute never becomes a no-op.
    pub(crate) fn attach_attributes(
        &self,
        mut statement: Statement,
        attributes: Vec<Attribute>,
    ) -> Result<Statement, SyntaxError> {
        if attributes.is_empty() {
            return Ok(statement);
        }
        let span = attributes[0].span;
        match &mut statement.node {
            StatementKind::Enum(_, _, _, _, _, existing) => existing.extend(attributes),
            StatementKind::FunctionDeclaration(declaration) => {
                declaration.attributes.extend(attributes)
            }
            StatementKind::Class(class_data) => class_data.attributes.extend(attributes),
            _ => {
                return Err(SyntaxError::new(
                    SyntaxErrorKind::UnsupportedAttributeTarget,
                    span,
                ))
            }
        }
        Ok(statement)
    }

    /// The optional single string-literal argument, with its delimiters removed
    /// and escapes resolved. Interpolated strings are not accepted.
    fn attribute_argument(&mut self) -> Result<Option<String>, SyntaxError> {
        if !self.match_lookahead_type(|t| t == &Token::LParen) {
            return Ok(None);
        }
        self.eat_token(&Token::LParen)?;

        let Some((Token::String, span)) = self.lookahead else {
            return Err(self.error_unexpected_lookahead_token("a string literal"));
        };
        let raw = &self.source[span.start..span.end];
        let Some(inner) = raw.get(1..raw.len().saturating_sub(1)) else {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidStringLiteral,
                span,
            ));
        };
        let argument = unescape_string(inner).into_owned();

        self.eat_token(&Token::String)?;
        self.eat_token(&Token::RParen)?;
        Ok(Some(argument))
    }

    /*
        StatementList
            : Statement
            | StatementList Statement
            ;
    */
    pub(crate) fn statement_list(&mut self) -> Result<Vec<Statement>, SyntaxError> {
        let mut statements = vec![];

        while self.lookahead.is_some() && !self.lookahead_is_dedent() {
            statements.push(self.statement()?);
            self.try_eat_expression_end()?;
        }

        Ok(statements)
    }

    /*
        Statement
            : ExpressionStatement
            | BlockStatement
            | VariableStatement
            | IfStatement
            | WhileStatement
            | ForStatement
            | ForeverStatement
            | FunctionDeclaration
            | ReturnStatement
            | UseStatement
            | TypeStatement
            | BreakStatement
            | ContinueStatement
            | EnumStatement
            | StructStatement
            | EmptyStatement
            ;
    */
    pub(crate) fn statement(&mut self) -> Result<Statement, SyntaxError> {
        self.enter_recursion()?;
        let res = self.dispatch_statement();
        self.exit_recursion();
        res
    }

    fn dispatch_statement(&mut self) -> Result<Statement, SyntaxError> {
        if self.lookahead.is_none() {
            return Ok(ast::empty_statement());
        }

        let attributes = self.attributes()?;

        let statement = match &self.lookahead {
            Some((Token::Public, _)) => {
                self.eat_token(&Token::Public)?;
                self.class_member_statement(MemberVisibility::Public)?
            }
            Some((Token::Protected, span)) => {
                let span = *span;
                self.eat_token(&Token::Protected)?;
                return Err(self.error_unexpected_token_with_span(
                    "public or private visibility",
                    "protected (only valid for class members)",
                    span,
                ));
            }
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                self.class_member_statement(MemberVisibility::Private)?
            }
            Some((Token::Indent, _)) => self.block_statement()?,
            Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _)) => {
                self.variable_statement(MemberVisibility::Public)?
            }
            Some((Token::Shared, _)) => self.shared_variable_statement(MemberVisibility::Public)?,
            Some((Token::If, _)) => self.if_statement(IfStatementType::If)?,
            Some((Token::Unless, _)) => self.if_statement(IfStatementType::Unless)?,
            Some((Token::While, _)) => self.while_statement(WhileStatementType::While)?,
            Some((Token::Until, _)) => self.while_statement(WhileStatementType::Until)?,
            Some((Token::Do, _)) => self.while_statement(WhileStatementType::DoWhile)?,
            Some((Token::Forever, _)) => self.while_statement(WhileStatementType::Forever)?,
            Some((Token::For, _)) => self.for_statement()?,
            Some((Token::Forall, _)) => self.forall_statement(AcceleratorTarget::Inferred)?,
            Some((Token::Gpu, _)) => self.gpu_statement(MemberVisibility::Public)?,
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Parallel, _)) => {
                self.function_declaration(MemberVisibility::Public)?
            }
            Some((Token::Runtime, _)) => self.runtime_function_declaration()?,
            Some((Token::Intrinsic, _)) => {
                self.intrinsic_function_declaration(MemberVisibility::Public)?
            }
            Some((Token::Return, _)) => self.return_statement()?,
            Some((Token::Use, _)) => self.use_statement()?,
            Some((Token::Type, _)) => self.type_statement(MemberVisibility::Public)?,
            Some((Token::Break, _)) => self.break_statement()?,
            Some((Token::Continue, _)) => self.continue_statement()?,
            Some((Token::Enum, _)) | Some((Token::MustUse, _)) => {
                self.enum_statement(MemberVisibility::Public, Vec::new())?
            }
            Some((Token::Struct, _)) => self.struct_statement(MemberVisibility::Public)?,
            Some((Token::Class, _)) => self.class_statement(MemberVisibility::Public)?,
            Some((Token::Trait, _)) => self.trait_statement(MemberVisibility::Public)?,
            Some((Token::Abstract, _)) => {
                self.eat_token(&Token::Abstract)?;
                self.abstract_class_statement(MemberVisibility::Public)?
            }
            _ => self.expression_statement()?,
        };
        self.attach_attributes(statement, attributes)
    }

    /*
        VariableStatement
            : 'let' VariableDeclarationList EXPRESSION_END
            | 'var' VariableDeclarationList EXPRESSION_END
            | 'const' Identifier ['=' Expression] EXPRESSION_END
            ;
    */
    pub(crate) fn variable_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let (token, declaration_type, keyword_span) = match &self.lookahead {
            Some((Token::Let, span)) => (Token::Let, VariableDeclarationType::Immutable, *span),
            Some((Token::Var, span)) => (Token::Var, VariableDeclarationType::Mutable, *span),
            Some((Token::Const, span)) => (Token::Const, VariableDeclarationType::Constant, *span),
            _ => Err(self.error_unexpected_lookahead_token("let, var or const"))?,
        };

        self.eat_token(&token)?;
        let declarations = self.variable_declaration_list(&declaration_type, keyword_span)?;
        Ok(ast::variable_statement_with_span(
            declarations,
            visibility,
            keyword_span,
        ))
    }

    /*
        SharedVariableStatement
            : 'shared' Identifier Type EXPRESSION_END
            ;
    */
    pub(crate) fn shared_variable_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Shared)?;

        let name = self.simple_identifier()?;
        let typ_expr = self.type_expression()?;
        let typ = match typ_expr {
            Some(t) => Some(Box::new(t)),
            None => return Err(self.error_unexpected_lookahead_token("type definition")),
        };

        let declaration = VariableDeclaration {
            name,
            typ,
            initializer: None,
            declaration_type: VariableDeclarationType::Mutable,
            is_shared: true,
            residency: crate::ast::statement::BindingResidency::Host,
        };

        self.eat_statement_end()?;

        Ok(ast::variable_statement(vec![declaration], visibility))
    }

    /*
        VariableDeclarationList
            : VariableDeclaration
            | VariableDeclarationList ',' VariableDeclaration
            ;
    */
    pub(crate) fn variable_declaration_list(
        &mut self,
        declaration_type: &VariableDeclarationType,
        keyword_span: Span,
    ) -> Result<Vec<VariableDeclaration>, SyntaxError> {
        let mut declarations = vec![self.variable_declaration(declaration_type, keyword_span)?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.variable_declaration(declaration_type, keyword_span)?);
        }

        Ok(declarations)
    }

    fn for_loop_variable_list(&mut self) -> Result<Vec<VariableDeclaration>, SyntaxError> {
        let mut declarations = vec![self.for_loop_variable()?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.for_loop_variable()?);
        }

        Ok(declarations)
    }

    /*
        VariableDeclaration
            : IDENTIFIER
            | IDENTIFIER TYPE
            | IDENTIFIER '=' Expression
            | IDENTIFIER TYPE '=' Expression
            ;
    */
    pub(crate) fn variable_declaration(
        &mut self,
        declaration_type: &VariableDeclarationType,
        keyword_span: Span,
    ) -> Result<VariableDeclaration, SyntaxError> {
        if let Some(error) = self.parenthesised_binding_error(ForeignForm::DestructuringLet) {
            return Err(error);
        }

        let (name, name_span) = self.declaration_name()?;

        if let Some(error) =
            self.foreign_annotation_error(&name, name_span, declaration_type, keyword_span)
        {
            return Err(error);
        }

        let typ = self.type_expression()?.map(Box::new);
        let initializer = self.optional_initializer()?;

        if matches!(declaration_type, VariableDeclarationType::Constant) && initializer.is_none() {
            return Err(SyntaxError::new(
                SyntaxErrorKind::MissingConstantInitializer { name: name.clone() },
                name_span,
            ));
        }

        Ok(VariableDeclaration {
            name,
            typ,
            initializer,
            declaration_type: declaration_type.clone(),
            is_shared: false,
            residency: crate::ast::statement::BindingResidency::Host,
        })
    }

    /// Rejects a binding that opens with a parenthesis, which reads as an
    /// attempt to bind several names at once.
    fn parenthesised_binding_error(&self, form: ForeignForm) -> Option<SyntaxError> {
        if !self.lookahead_is_lparen() {
            return None;
        }
        let span = self.current_token_span();
        Some(
            self.error_unexpected_token_with_span("identifier", "(", span)
                .with_foreign_form(form),
        )
    }

    /// Rejects the two ways a binding names its mutability or type that Miri
    /// does not share: `let mut x`, and a type introduced by a colon.
    ///
    /// Both are recognised after the name is read, which is what tells `mut`
    /// apart from an ordinary binding: Miri would take it as the name itself,
    /// with the identifier that follows read as its type.
    fn foreign_annotation_error(
        &self,
        name: &str,
        name_span: Span,
        declaration_type: &VariableDeclarationType,
        keyword_span: Span,
    ) -> Option<SyntaxError> {
        let binds_after_mut = name == MUT_BINDING
            && matches!(declaration_type, VariableDeclarationType::Immutable)
            && matches!(self.lookahead, Some((Token::Identifier, _)));
        if binds_after_mut {
            return Some(
                self.error_unexpected_token_with_span(
                    "an end of statement",
                    "identifier",
                    name_span,
                )
                .with_foreign_form(ForeignForm::LetMut {
                    keyword_start: keyword_span.start,
                    mut_end: name_span.end,
                }),
            );
        }

        if !self.lookahead_is_colon() {
            return None;
        }
        let colon = self.current_token_span();
        Some(
            self.error_unexpected_token_with_span("an expression", ":", colon)
                .with_foreign_form(ForeignForm::ColonAnnotation {
                    colon_start: colon.start,
                    colon_end: colon.end,
                }),
        )
    }

    fn for_loop_variable(&mut self) -> Result<VariableDeclaration, SyntaxError> {
        if let Some(error) = self.parenthesised_binding_error(ForeignForm::TupleForBinding) {
            return Err(error);
        }

        let (name, _) = self.declaration_name()?;
        let typ = self.type_expression()?.map(Box::new);
        Ok(VariableDeclaration {
            name,
            typ,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
            is_shared: false,
            residency: crate::ast::statement::BindingResidency::Host,
        })
    }

    fn declaration_name(&mut self) -> Result<(String, crate::error::syntax::Span), SyntaxError> {
        let name_expr = self.identifier()?;
        let span = name_expr.span;
        match name_expr.node {
            ExpressionKind::Identifier(id, None) => Ok((id, span)),
            ExpressionKind::Identifier(id, Some(class)) => {
                Err(self
                    .error_unexpected_token("a simple identifier", &format!("{}::{}", class, id)))
            }
            _ => Err(self.error_unexpected_token("identifier", &format!("{:?}", name_expr))),
        }
    }

    fn optional_initializer(&mut self) -> Result<Option<Box<Expression>>, SyntaxError> {
        match &self.lookahead {
            Some((Token::Assign, _)) => {
                self.eat_token(&Token::Assign)?;
                Ok(opt_expr(self.expression()?))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn statement_body(&mut self) -> Result<Statement, SyntaxError> {
        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;

            if self.lookahead_is_expression_end() {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    return self.block_statement();
                }
                return Ok(ast::empty_statement());
            }

            if self.lookahead.is_none() || self.lookahead_is_dedent() || self.lookahead_is_else() {
                return Ok(ast::empty_statement());
            }
        } else if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;

            if !self.lookahead_is_indent() {
                return Ok(ast::empty_statement());
            }
        } else if self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
            // `else if` chains back into a fresh statement.
            return self.statement();
        } else if self.match_lookahead_type(|t| t == &Token::Arrow) {
            let arrow_span = self
                .lookahead
                .as_ref()
                .map(|(_, span)| *span)
                .unwrap_or_default();
            let err = SyntaxError::new(
                SyntaxErrorKind::UnexpectedToken {
                    expected: "a colon or an expression end".to_string(),
                    found: "->".to_string(),
                },
                arrow_span,
            )
            .with_foreign_form(
                crate::error::foreign_syntax::ForeignForm::ArrowReturnType {
                    arrow_start: arrow_span.start,
                    arrow_end: arrow_span.end,
                },
            );
            return Err(err);
        } else if self.match_lookahead_type(|t| t == &Token::LBrace) {
            let brace_span = self
                .lookahead
                .as_ref()
                .map(|(_, span)| *span)
                .unwrap_or_default();
            let err = SyntaxError::new(
                SyntaxErrorKind::UnexpectedToken {
                    expected: "a colon or an expression end".to_string(),
                    found: "{".to_string(),
                },
                brace_span,
            )
            .with_foreign_form(crate::error::foreign_syntax::ForeignForm::BraceBlock);
            return Err(err);
        } else {
            return Err(self.error_unexpected_lookahead_token("a colon or an expression end"));
        }

        if self.lookahead.is_some() {
            return self.statement();
        }

        Ok(ast::empty_statement())
    }

    /*
        IfStatement
            : 'if' Expression ':' ExpressionStatement EXPRESSION_END ('else' ExpressionStatement EXPRESSION_END)?
            | 'if' Expression EXPRESSION_END BlockStatement ('else' EXPRESSION_END BlockStatement)?
            ;
    */
    pub(crate) fn if_statement(
        &mut self,
        if_statement_type: IfStatementType,
    ) -> Result<Statement, SyntaxError> {
        if if_statement_type == IfStatementType::Unless {
            self.eat_token(&Token::Unless)?;
        } else {
            self.eat_token(&Token::If)?;
        }

        if matches!(
            &self.lookahead,
            Some((Token::Let, _)) | Some((Token::Var, _))
        ) {
            return self.if_let_statement();
        }

        let condition = self.expression()?;
        let then_block = self.statement_body()?;

        self.try_eat_expression_end()?;

        let else_block = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            Some(self.statement_body()?)
        } else {
            None
        };

        if if_statement_type == IfStatementType::Unless {
            Ok(ast::unless_statement(condition, then_block, else_block))
        } else {
            Ok(ast::if_statement(condition, then_block, else_block))
        }
    }

    fn if_let_statement(&mut self) -> Result<Statement, SyntaxError> {
        let is_mutable = matches!(&self.lookahead, Some((Token::Var, _)));
        let token = if is_mutable { Token::Var } else { Token::Let };
        self.eat_token(&token)?;

        let pattern = self.pattern()?;
        self.eat_token(&Token::Assign)?;
        let value = self.expression()?;
        let then_body = self.statement_body()?;

        self.try_eat_expression_end()?;

        let else_body = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            Some(self.statement_body()?)
        } else {
            None
        };

        // `Some(x)` is the MIR catch-all, so use `None` as its complement; any
        // other pattern falls through to `_` (Default).
        let else_pattern = option_pattern_complement(&pattern);

        let then_branch = MatchBranch {
            patterns: vec![pattern],
            guard: None,
            body: Box::new(then_body),
            is_mutable,
        };
        let else_branch = MatchBranch {
            patterns: vec![else_pattern],
            guard: None,
            body: Box::new(else_body.unwrap_or_else(ast::empty_statement)),
            is_mutable: false,
        };
        let match_expr = ast::match_expression(value, vec![then_branch, else_branch]);
        Ok(ast::expression_statement(match_expr))
    }

    /*
        WhileStatement
            : 'while' Expression ':' ExpressionStatement EXPRESSION_END
            | 'while' Expression EXPRESSION_END BlockStatement
            | 'until' Expression ':' ExpressionStatement EXPRESSION_END
            | 'until' Expression EXPRESSION_END BlockStatement
            : 'do' ':' ExpressionStatement 'while' Expression EXPRESSION_END
            : 'do' ExpressionStatement 'while' Expression EXPRESSION_END
            | 'forever' ':' ExpressionStatement EXPRESSION_END
            | 'forever' EXPRESSION_END BlockStatement
            ;
    */
    pub(crate) fn while_statement(
        &mut self,
        statement_type: WhileStatementType,
    ) -> Result<Statement, SyntaxError> {
        let (condition, then_block, kind) = match statement_type {
            WhileStatementType::Until => {
                self.eat_token(&Token::Until)?;
                (self.expression()?, self.statement_body()?, statement_type)
            }
            WhileStatementType::Forever => {
                self.eat_token(&Token::Forever)?;
                let body = self.statement_body()?;
                (ast::literal(ast::boolean(true)), body, statement_type)
            }
            WhileStatementType::DoWhile => self.do_loop()?,
            WhileStatementType::While => {
                self.eat_token(&Token::While)?;
                if matches!(
                    &self.lookahead,
                    Some((Token::Let, _)) | Some((Token::Var, _))
                ) {
                    return self.while_let_statement();
                }
                (self.expression()?, self.statement_body()?, statement_type)
            }
            WhileStatementType::DoUntil => {
                return Err(self.error_unexpected_lookahead_token(
                    "do-until cannot be entered directly; it is produced by `do … until`",
                ));
            }
        };

        Ok(ast::while_statement_with_type(condition, then_block, kind))
    }

    fn do_loop(&mut self) -> Result<(Expression, Statement, WhileStatementType), SyntaxError> {
        self.eat_token(&Token::Do)?;
        let body = self.statement_body()?;

        let kind = match &self.lookahead {
            Some((Token::While, _)) => {
                self.eat_token(&Token::While)?;
                WhileStatementType::DoWhile
            }
            Some((Token::Until, _)) => {
                self.eat_token(&Token::Until)?;
                WhileStatementType::DoUntil
            }
            _ => return Err(self.error_unexpected_lookahead_token("while or until")),
        };
        let condition = self.expression()?;
        Ok((condition, body, kind))
    }

    fn while_let_statement(&mut self) -> Result<Statement, SyntaxError> {
        let is_mutable = matches!(&self.lookahead, Some((Token::Var, _)));
        let token = if is_mutable { Token::Var } else { Token::Let };
        self.eat_token(&token)?;

        let pattern = self.pattern()?;
        self.eat_token(&Token::Assign)?;
        let value = self.expression()?;
        let body = self.statement_body()?;

        let break_pattern = option_pattern_complement(&pattern);

        let match_branch = MatchBranch {
            patterns: vec![pattern],
            guard: None,
            body: Box::new(body),
            is_mutable,
        };
        let break_branch = MatchBranch {
            patterns: vec![break_pattern],
            guard: None,
            body: Box::new(ast::break_statement()),
            is_mutable: false,
        };
        let match_expr = ast::match_expression(value, vec![match_branch, break_branch]);
        let loop_body = ast::block(vec![ast::expression_statement(match_expr)]);

        Ok(ast::while_statement_with_type(
            ast::literal(ast::boolean(true)),
            loop_body,
            WhileStatementType::Forever,
        ))
    }

    /*
        ForStatement
            : 'for' VariableDeclarationList 'in' RangeExpression ':' ExpressionStatement EXPRESSION_END
            | 'for' VariableDeclarationList 'in' RangeExpression EXPRESSION_END BlockStatement
            ;
    */
    /// Dispatches the `gpu`-prefixed statement forms by examining the token
    /// that follows `gpu`:
    ///   * `gpu forall ...` → [`StatementKind::Forall`] with device = Gpu.
    ///   * `gpu let ...` / `gpu var ...` → an ordinary variable statement
    ///     whose declarations carry [`BindingResidency::Gpu`].
    ///   * anything else continues into the function declaration grammar
    ///     with `is_gpu` already set, so combinations like `gpu parallel fn`
    ///     still parse.
    pub(crate) fn gpu_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Gpu)?;

        if self.match_lookahead_type(|t| t == &Token::For) {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidModifierCombination {
                    combination: "gpu for".to_string(),
                    reason: "unexpected 'for' after 'gpu'; use 'forall' or 'gpu forall'"
                        .to_string(),
                },
                self.current_token_span(),
            ));
        }

        if self.match_lookahead_type(|t| t == &Token::Forall) {
            return self.forall_statement(AcceleratorTarget::Gpu);
        }

        if self.match_lookahead_type(|t| t == &Token::Frame) {
            return self.gpu_frame_statement();
        }

        if self.match_lookahead_type(|t| matches!(t, Token::Let | Token::Var)) {
            return self.gpu_variable_statement(visibility);
        }

        if self.match_lookahead_type(|t| t == &Token::Const) {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidModifierCombination {
                    combination: "gpu const".to_string(),
                    reason: "Residency on a compile-time constant has no meaning.".to_string(),
                },
                self.current_token_span(),
            ));
        }

        let mut properties = FunctionProperties {
            is_async: false,
            is_parallel: false,
            is_gpu: true,
            is_static: false,
            visibility,
        };
        self.continue_function_modifiers(&mut properties)?;
        self.function_declaration_after_modifiers(
            super::declarations::class::BodyMode::Required,
            properties,
        )
    }

    /// Parses `let`/`var` after the `gpu` keyword has been consumed and
    /// stamps every declaration with [`BindingResidency::Gpu`]. `gpu const`
    /// is rejected — residency on a compile-time constant has no meaning.
    fn gpu_variable_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let stmt = self.variable_statement(visibility)?;
        let span = stmt.span;
        let StatementKind::Variable(decls, vis) = stmt.node else {
            return Err(self.error_unexpected_lookahead_token("let or var after 'gpu'"));
        };
        let decls = decls
            .into_iter()
            .map(|mut d| {
                d.residency = crate::ast::statement::BindingResidency::Gpu;
                d
            })
            .collect();
        Ok(crate::ast::factory::stmt_with_span(
            StatementKind::Variable(decls, vis),
            span,
        ))
    }

    pub(crate) fn continue_function_modifiers(
        &mut self,
        properties: &mut FunctionProperties,
    ) -> Result<(), SyntaxError> {
        while self.lookahead_is_function_modifier() {
            match &self.lookahead {
                Some((Token::Async, _)) => {
                    self.eat_token(&Token::Async)?;
                    properties.is_async = true;
                }
                Some((Token::Parallel, _)) => {
                    self.eat_token(&Token::Parallel)?;
                    properties.is_parallel = true;
                }
                Some((Token::Gpu, _)) => {
                    return Err(SyntaxError::new(
                        SyntaxErrorKind::InvalidModifierCombination {
                            combination: "gpu gpu".to_string(),
                            reason: "The 'gpu' modifier may appear only once.".to_string(),
                        },
                        self.current_token_span(),
                    ));
                }
                _ => {
                    return Err(self.error_unexpected_lookahead_token(
                        "function modifier (async or parallel) or 'fn'",
                    ));
                }
            }
        }
        if properties.is_async && properties.is_gpu {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidModifierCombination {
                    combination: "async gpu".to_string(),
                    reason: "GPU kernels are inherently asynchronous.".to_string(),
                },
                self.current_token_span(),
            ));
        }
        Ok(())
    }

    pub(crate) fn forall_statement(
        &mut self,
        device: AcceleratorTarget,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Forall)?;

        let variable_declarations = self.for_loop_variable_list()?;
        let dims = variable_declarations.len();

        if dims > 3 {
            return Err(self.error_unexpected_token(
                "at most 3 loop variables",
                &format!("{} variables", dims),
            ));
        }

        self.eat_token(&Token::In)?;

        let iterable = self.forall_iterable(dims)?;

        let body = self.statement_body()?;
        Ok(ast::forall_statement(
            device,
            variable_declarations,
            iterable,
            body,
        ))
    }

    fn forall_iterable(&mut self, dims: usize) -> Result<Expression, SyntaxError> {
        let first_range = self.range_expression()?;
        let first_range_span = first_range.span;

        if dims == 1 {
            return Ok(self.normalized_range(first_range));
        }

        // dims >= 2: expect comma-separated ranges
        let mut ranges = vec![self.normalized_range(first_range)];

        for _ in 1..dims {
            if !self.match_lookahead_type(|t| t == &Token::Comma) {
                return Err(self.error_unexpected_lookahead_token(&format!(
                    "{}D forall requires {} comma-separated ranges",
                    dims, dims
                )));
            }
            self.eat_token(&Token::Comma)?;
            let range = self.range_expression()?;
            ranges.push(self.normalized_range(range));
        }

        Ok(ast::tuple_with_span(ranges, first_range_span))
    }

    fn normalized_range(&self, expr: Expression) -> Expression {
        if matches!(&expr.node, ExpressionKind::Range(_, _, _)) {
            expr
        } else {
            let span = expr.span;
            ast::range_with_span(expr, None, RangeExpressionType::IterableObject, span)
        }
    }

    pub(crate) fn gpu_frame_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Frame)?;

        // Check if the next token is not an identifier (block form).
        // Block form: `gpu frame` followed by indent/newline and then forall statements.
        // Single-pass form: `gpu frame i in 0..4: body`
        if !self.match_lookahead_type(|t| t == &Token::Identifier) {
            // Block form: indent should follow, parse as block
            let block = self.statement_body()?;
            return Ok(ast::gpu_frame_block(block));
        }

        // Single-pass form: `gpu frame <var> in <range>: body`.
        let variable_declarations = self.for_loop_variable_list()?;

        if variable_declarations.len() != 1 {
            return Err(self.error_unexpected_token(
                "exactly 1 loop variable for gpu frame",
                &format!("{} variables", variable_declarations.len()),
            ));
        }

        self.eat_token(&Token::In)?;

        let first_range = self.range_expression()?;
        let iterable = self.normalized_range(first_range);

        let body = self.statement_body()?;
        Ok(ast::gpu_frame_statement(
            variable_declarations,
            iterable,
            body,
        ))
    }

    pub(crate) fn for_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::For)?;

        let variable_declarations = self.for_loop_variable_list()?;
        self.eat_token(&Token::In)?;
        let iterable_expr = self.range_expression()?;

        let iterable = if let ExpressionKind::Range(_, _, _) = &iterable_expr.node {
            iterable_expr
        } else {
            let span = iterable_expr.span;
            ast::range_with_span(
                iterable_expr,
                None,
                RangeExpressionType::IterableObject,
                span,
            )
        };

        if let ExpressionKind::Range(_, _, range_type) = &iterable.node {
            if *range_type != RangeExpressionType::IterableObject && variable_declarations.len() > 1
            {
                return Err(self.error_unexpected_token(
                    "a single loop variable for a numeric range",
                    &format!("{} variables", variable_declarations.len()),
                ));
            }
        }

        let body = self.statement_body()?;

        Ok(ast::for_statement(variable_declarations, iterable, body))
    }

    /*
        ReturnStatement
            : 'return' Expression EXPRESSION_END
            ;
    */
    pub(crate) fn return_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Return)?;
        let expression = if self.lookahead_is_expression_end() || self.lookahead_is_postfix_guard()
        {
            None
        } else {
            opt_expr(self.expression()?)
        };
        let statement = self.apply_postfix_jump_guard(ast::return_statement(expression))?;
        self.eat_statement_end()?;
        Ok(statement)
    }

    /*
        BreakStatement
            : 'break' ('if' | 'unless' Expression)? EXPRESSION_END
            ;
    */
    pub(crate) fn break_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Break)?;
        let statement = self.apply_postfix_jump_guard(ast::break_statement())?;
        self.eat_statement_end()?;
        Ok(statement)
    }

    /*
        ContinueStatement
            : 'continue' ('if' | 'unless' Expression)? EXPRESSION_END
            ;
    */
    pub(crate) fn continue_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Continue)?;
        let statement = self.apply_postfix_jump_guard(ast::continue_statement())?;
        self.eat_statement_end()?;
        Ok(statement)
    }

    fn lookahead_is_postfix_guard(&self) -> bool {
        self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless)
    }

    /// Wraps a jump statement in a trailing `if`/`unless` guard when one
    /// follows: `break if cond` is equivalent to `if cond: break`. Returns the
    /// jump unchanged when no guard is present.
    fn apply_postfix_jump_guard(&mut self, jump: Statement) -> Result<Statement, SyntaxError> {
        if self.match_lookahead_type(|t| t == &Token::If) {
            self.eat_token(&Token::If)?;
            let condition = self.expression()?;
            return Ok(ast::if_statement(condition, jump, None));
        }
        if self.match_lookahead_type(|t| t == &Token::Unless) {
            self.eat_token(&Token::Unless)?;
            let condition = self.expression()?;
            return Ok(ast::unless_statement(condition, jump, None));
        }
        Ok(jump)
    }

    /*
        UseStatement
            : 'use' ImportPathExpression ( 'as' Identifier )?
            ;
    */
    pub(crate) fn use_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Use)?;
        let import_path = self.import_path_expression()?;
        let alias = if self.match_lookahead_type(|t| t == &Token::As) {
            self.eat_token(&Token::As)?;
            opt_expr(self.identifier()?)
        } else {
            None
        };
        Ok(ast::use_statement(import_path, alias))
    }

    /*
        ImportPathExpression
            : Identifier ('.' Identifier)* ('.' ('*' | '{' ImportList '}'))?
            ;
    */
    pub(crate) fn import_path_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut segments = vec![];

        if self.match_lookahead_type(|t| t == &Token::System) {
            let (_, span) = self.eat_token(&Token::System)?;
            segments.push(ast::identifier_with_span("system", span));
        } else {
            segments.push(self.import_path_segment()?);
        }
        let mut kind = ImportPathKind::Simple;

        while self.match_lookahead_type(|t| t == &Token::Dot) {
            self.eat_token(&Token::Dot)?;

            if self.match_lookahead_type(|t| t == &Token::Star) {
                self.eat_token(&Token::Star)?;
                kind = ImportPathKind::Wildcard;
                break;
            }

            if self.match_lookahead_type(|t| t == &Token::LBrace) {
                self.eat_token(&Token::LBrace)?;
                let mut multi_imports = vec![self.multi_import_segment()?];
                while self.lookahead_is_comma() {
                    self.eat_token(&Token::Comma)?;
                    if self.match_lookahead_type(|t| t == &Token::RBrace) {
                        break;
                    }
                    multi_imports.push(self.multi_import_segment()?);
                }
                self.eat_token(&Token::RBrace)?;
                kind = ImportPathKind::Multi(multi_imports);
                break;
            }

            segments.push(self.import_path_segment()?);
        }
        Ok(ast::import_path_expression(segments, kind))
    }

    fn import_path_segment(&mut self) -> Result<Expression, SyntaxError> {
        if self.match_lookahead_type(|t| t == &Token::Gpu) {
            let (_, span) = self.eat_token(&Token::Gpu)?;
            return Ok(ast::identifier_with_span("gpu", span));
        }
        if self.match_lookahead_type(|t| t == &Token::Local) {
            let (_, span) = self.eat_token(&Token::Local)?;
            return Ok(ast::identifier_with_span("local", span));
        }
        self.identifier()
    }

    pub(crate) fn multi_import_segment(
        &mut self,
    ) -> Result<(Expression, Option<Box<Expression>>), SyntaxError> {
        let path = self.import_path_segment()?;
        let alias = if self.match_lookahead_type(|t| t == &Token::As) {
            self.eat_token(&Token::As)?;
            Some(Box::new(self.identifier()?))
        } else {
            None
        };
        Ok((path, alias))
    }

    /*
        ExpressionStatement
            : Expression EXPRESSION_END
            ;
    */
    pub(crate) fn expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let expression = self.expression()?;

        // Match block expressions consume their own Dedent; the trailing
        // ExpressionStatementEnd may be absent.
        if matches!(expression.node, ExpressionKind::Match(..)) {
            self.try_eat_expression_end()?;
            return Ok(ast::expression_statement(expression));
        }

        self.eat_statement_end()?;
        Ok(ast::expression_statement(expression))
    }

    /*
        BlockStatement
            : Indent OptionalStatementList Dedent
            ;
    */
    pub(crate) fn block_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Indent)?;
        let body = match &self.lookahead {
            Some((Token::Dedent, _)) => vec![],
            _ => self.statement_list()?,
        };
        self.eat_token(&Token::Dedent)?;
        Ok(ast::block(body))
    }
}

fn option_pattern_complement(pattern: &Pattern) -> Pattern {
    if matches!(
        pattern,
        Pattern::EnumVariant(parent, _)
            if matches!(parent.as_ref(), Pattern::Identifier(n) if n == "Some")
    ) {
        Pattern::Literal(crate::ast::literal::Literal::None)
    } else {
        Pattern::Default
    }
}
