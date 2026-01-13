// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::TypeDeclarationKind;
use crate::ast::*;
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::utils::{is_guard, is_inheritance_modifier};
use super::{DeclarationBlockConfig, Parser};

impl<'source> Parser<'source> {
    pub(crate) fn class_member_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let statement = match &self._lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement(visibility)?,
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.function_declaration(visibility)?
            }
            Some((Token::Enum, _)) => self.enum_statement(visibility)?,
            Some((Token::Struct, _)) => self.struct_statement(visibility)?,
            Some((Token::Type, _)) => self.type_statement(visibility)?,
            _ => {
                return Err(self.error_unexpected_lookahead_token(
                    "let, var, async, def, gpu, enum, type or struct",
                ));
            }
        };
        Ok(statement)
    }

    /*
        FunctionDeclaration
            : 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END BlockStatement
            | 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] ':' ExpressionStatement EXPRESSION_END
            | 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END  // Abstract (no body) - only in traits/abstract classes
            ;
    */
    pub(crate) fn function_declaration(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.function_declaration_with_context(visibility, false)
    }

    /// Parses a function declaration, optionally allowing abstract functions (no body).
    /// Abstract functions are only valid in traits and abstract classes.
    pub(crate) fn function_declaration_with_context(
        &mut self,
        visibility: MemberVisibility,
        allow_abstract: bool,
    ) -> Result<Statement, SyntaxError> {
        let mut properties = FunctionProperties {
            is_async: false,
            is_parallel: false,
            is_gpu: false,
            visibility,
        };

        while self.lookahead_is_function_modifier() {
            match &self._lookahead {
                Some((Token::Async, _)) => {
                    self.eat_token(&Token::Async)?;
                    properties.is_async = true;
                }
                Some((Token::Parallel, _)) => {
                    self.eat_token(&Token::Parallel)?;
                    properties.is_parallel = true;
                }
                Some((Token::Gpu, _)) => {
                    self.eat_token(&Token::Gpu)?;
                    properties.is_gpu = true;
                }
                _ => {
                    return Err(self.error_unexpected_lookahead_token(
                        "function modifier (async, parallel or gpu)",
                    ))
                }
            }
        }

        // Validate modifier combinations
        if properties.is_async && properties.is_gpu {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidModifierCombination {
                    combination: "async gpu".to_string(),
                    reason: "GPU kernels are inherently asynchronous.".to_string(),
                },
                self.current_token_span(), // Approximation, ideally we track spans of modifiers
            ));
        }
        if properties.is_async && properties.is_parallel {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidModifierCombination {
                    combination: "async parallel".to_string(),
                    reason: "Parallel functions represent a different execution model and cannot be async.".to_string(),
                },
                self.current_token_span(),
            ));
        }

        self.eat_token(&Token::Fn)?;

        let name = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let token = self.eat_token(&Token::Identifier)?;
                self.source[token.1.start..token.1.end].to_string()
            }
            Some((Token::LessThan, _)) | Some((Token::LParen, _)) => {
                // No name, it's a lambda
                "".to_string()
            }
            _ => return Err(self.error_unexpected_lookahead_token("a function name, '(' or '<'")),
        };

        let generic_types = self.generic_types_expression()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        let body = if name.is_empty() {
            // This is a lambda expression. Its body parsing is special.
            if self.lookahead_is_colon() {
                self.eat_token(&Token::Colon)?;
                // An inline lambda body is a single expression, not a full statement.
                // We parse it and wrap it in an ExpressionStatement for the AST.
                let expr = self.expression()?;
                Some(ast::expression_statement(expr))
            } else {
                // A block lambda body is a normal block statement, which statement_body handles correctly.
                Some(self.statement_body()?)
            }
        } else {
            // This is a named function. Parse its body.
            let body_stmt = self.statement_body()?;

            // Check if this is an abstract function (returns empty statement in abstract context)
            if allow_abstract && matches!(body_stmt.node, StatementKind::Empty) {
                // Abstract function - no body
                None
            } else {
                Some(body_stmt)
            }
        };

        if name.is_empty() {
            return Ok(ast::expression_statement(ast::lambda_expression(
                generic_types,
                parameters,
                return_type,
                body.expect("Lambda must have a body"),
                properties,
            )));
        }

        match body {
            Some(body_stmt) => Ok(ast::function_declaration(
                &name,
                generic_types,
                parameters,
                return_type,
                body_stmt,
                properties,
            )),
            None => Ok(ast::abstract_function_declaration(
                &name,
                generic_types,
                parameters,
                return_type,
                properties,
            )),
        }
    }

    /*
        GenericTypesDeclaration
            : '<' GenericType (',' GenericType)* '>'
            ;
    */
    pub(crate) fn generic_types_declaration(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(&Token::LessThan)?;

        let mut types = vec![self.generic_type()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            types.push(self.generic_type()?);
        }

        self.eat_token(&Token::GreaterThan)?;
        Ok(types)
    }

    /*
        GenericType
            : Identifier ('extends' | 'implements' | 'includes' TypeExpression)?
            ;
    */
    pub(crate) fn generic_type(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        if self._lookahead.is_none() || !self.lookahead_is_inheritance_modifier() {
            return Ok(ast::generic_type_expression(
                identifier,
                None,
                TypeDeclarationKind::None,
            ));
        }

        let token_span = self.eat(is_inheritance_modifier, "extends, includes or implements")?;
        let kind = match token_span.0 {
            Token::Extends => TypeDeclarationKind::Extends,
            Token::Implements => TypeDeclarationKind::Implements,
            Token::Includes => TypeDeclarationKind::Includes,
            _ => TypeDeclarationKind::None,
        };

        let typ = self.type_expression()?.map(Box::new);

        Ok(ast::generic_type_expression(identifier, typ, kind))
    }

    /*
        ParameterList
            : Parameter
            | ParameterList ',' Parameter
            ;
    */
    pub(crate) fn parameter_list(&mut self) -> Result<Vec<Parameter>, SyntaxError> {
        let mut parameters = vec![self.parameter()?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            // Allow an optional trailing comma before the closing parenthesis.
            if self.lookahead_is_rparen() {
                break;
            }
            parameters.push(self.parameter()?);
        }

        Ok(parameters)
    }

    /*
        Parameter
            : Identifier [TypeExpression] [Guard]
            ;
    */
    pub(crate) fn parameter(&mut self) -> Result<Parameter, SyntaxError> {
        let name = self.parse_simple_identifier()?;

        let typ = match self.type_expression()? {
            Some(typ) => Box::new(typ),
            None => {
                // Miri doesn't support untyped parameters
                return Err(self.error_missing_type_expression());
            }
        };

        let guard = if self._lookahead.is_some() && self.lookahead_is_guard() {
            opt_expr(self.guard_expression()?)
        } else {
            None
        };

        let default_value = if self.match_lookahead_type(|t| t == &Token::Assign) {
            self.eat_token(&Token::Assign)?;
            Some(Box::new(self.expression()?))
        } else {
            None
        };

        Ok(Parameter {
            name,
            typ,
            guard,
            default_value,
        })
    }

    /*
        GuardExpression
            : '>' Expression
            | '>=' Expression
            | '<' Expression
            | '<=' Expression
            | 'in' Expression
            | 'not' Expression
            | 'not in' Expression
            ;
    */
    pub(crate) fn guard_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut guard_op = match &self._lookahead {
            Some((Token::GreaterThan, _)) => GuardOp::GreaterThan,
            Some((Token::GreaterThanEqual, _)) => GuardOp::GreaterThanEqual,
            Some((Token::LessThan, _)) => GuardOp::LessThan,
            Some((Token::LessThanEqual, _)) => GuardOp::LessThanEqual,
            Some((Token::In, _)) => GuardOp::In,
            Some((Token::Not, _)) => GuardOp::Not,
            _ => return Err(self.error_unexpected_lookahead_token("guard operator")),
        };

        self.eat(is_guard, "guard operator")?;
        if self._lookahead.is_some() && self.lookahead_is_in() {
            self.eat_token(&Token::In)?;
            guard_op = GuardOp::NotIn;
        }

        let expression = self.expression()?;
        Ok(ast::guard(guard_op, expression))
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
        EnumStatement
            : 'enum' Identifier: EnumValue (',' EnumValue)*
            | 'enum' Identifier INDENT EnumValue EXPRESSION_END (EnumValue EXPRESSION_END)* DEDENT
            ;
    */
    pub(crate) fn enum_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Enum)?;
        let name = self.identifier()?;
        self.parse_declaration_block(
            Self::enum_value_expression,
            |n, _, vals: Vec<Expression>, vis| ast::enum_statement(n, vals, vis),
            name,
            visibility,
            DeclarationBlockConfig {
                inline_error: "either a colon for inline enums or an indentation for block enums",
                missing_members_error: SyntaxErrorKind::MissingEnumMembers,
            },
            None,
        )
    }

    /*
        EnumValue
            : Identifier
            | Identifier '(' TypeExpression (',' TypeExpression)* ')'
            ;
    */
    pub fn enum_value_expression(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        let types = if self.match_lookahead_type(|t| t == &Token::LParen) {
            self.multiple_element_type_expressions(
                "Enum value type",
                &Token::LParen,
                &Token::RParen,
            )?
        } else {
            vec![]
        };

        Ok(ast::enum_value_expression(identifier, types))
    }

    /*
        StructStatement
            : 'struct' Identifier: StructMember (',' StructMember)*
            | 'struct' Identifier INDENT StructMember EXPRESSION_END (StructMember EXPRESSION_END)* DEDENT
            ;
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
        ClassStatement
            : 'class' Identifier [GenericTypesDeclaration] ['extends' Identifier]
              ['implements' IdentifierList] EXPRESSION_END BlockStatement
            ;
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
        AbstractClassStatement
            : 'abstract' 'class' Identifier [GenericTypesDeclaration] ['extends' Identifier]
              ['implements' IdentifierList] EXPRESSION_END BlockStatement
            ;
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
        TraitStatement
            : 'trait' Identifier [GenericTypesDeclaration] ['extends' IdentifierList]
              EXPRESSION_END BlockStatement
            ;
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
    #[allow(dead_code)]
    fn class_member_or_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.class_member_or_statement_with_abstract(false)
    }

    /// Parses a class member (field or method) with optional visibility modifier.
    /// If allow_abstract is true, functions without bodies are allowed.
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
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement(visibility),
            Some((Token::Async, _))
            | Some((Token::Fn, _))
            | Some((Token::Gpu, _))
            | Some((Token::Parallel, _)) => {
                self.function_declaration_with_context(visibility, allow_abstract)
            }
            Some((Token::Type, _)) => self.type_statement(visibility),
            _ => Err(self.error_unexpected_lookahead_token(
                "class member (let, var, fn, async, gpu, or type)",
            )),
        }
    }

    /*
        StructMember
            : Identifier TypeExpression
            ;
    */
    pub(crate) fn struct_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let typ = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_struct_member_type())?;
        Ok(ast::struct_member_expression(name, typ))
    }

    /*
        TypeStatement
            : 'type' TypeDeclaration, (',' TypeDeclaration)*
            ;
    */
    pub(crate) fn type_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Type)?;
        let mut declarations = vec![self.type_declaration()?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            if self.lookahead_is_expression_end() {
                break; // Allow trailing comma
            }
            declarations.push(self.type_declaration()?);
        }
        self.eat_statement_end()?;
        Ok(ast::type_statement(declarations, visibility))
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

    /*
        TypeDeclaration
            : Identifier ('is' | 'extends' | 'implements' | 'includes') TypeExpression
            ;
    */
    pub(crate) fn type_declaration(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
        let kind = match self._lookahead {
            Some((Token::Is, _)) => {
                self.eat_token(&Token::Is)?;
                TypeDeclarationKind::Is
            }
            Some((Token::Extends, _)) => {
                self.eat_token(&Token::Extends)?;
                TypeDeclarationKind::Extends
            }
            Some((Token::Implements, _)) => {
                self.eat_token(&Token::Implements)?;
                TypeDeclarationKind::Implements
            }
            Some((Token::Includes, _)) => {
                self.eat_token(&Token::Includes)?;
                TypeDeclarationKind::Includes
            }
            Some((Token::Comma, _)) | Some((Token::ExpressionStatementEnd, _)) => {
                // If we see a comma or the end of the statement, it means this is a continuation of a type declaration list
                return Ok(ast::type_declaration_expression(
                    name,
                    generic_types,
                    TypeDeclarationKind::None,
                    None,
                ));
            }
            _ => {
                return Err(self.error_unexpected_token(
                    "is, implements, includes or extends",
                    &self.lookahead_as_string(),
                ))
            }
        };
        let type_expr = self.type_expression()?.map(Box::new);
        Ok(ast::type_declaration_expression(
            name,
            generic_types,
            kind,
            type_expr,
        ))
    }
}
