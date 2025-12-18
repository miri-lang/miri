// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::ast_factory as ast;
use crate::lexer::Token;
use crate::syntax_error::{SyntaxError, SyntaxErrorKind};

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
            ;
    */
    pub(crate) fn function_declaration(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let mut properties = FunctionProperties {
            is_async: false,
            is_gpu: false,
            visibility,
        };

        while self.lookahead_is_function_modifier() {
            match &self._lookahead {
                Some((Token::Async, _)) => {
                    self.eat_token(&Token::Async)?;
                    properties.is_async = true;
                }
                Some((Token::Gpu, _)) => {
                    self.eat_token(&Token::Gpu)?;
                    properties.is_gpu = true;
                }
                _ => {
                    return Err(
                        self.error_unexpected_lookahead_token("function modifier (async or gpu)")
                    )
                }
            }
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
                ast::expression_statement(expr)
            } else {
                // A block lambda body is a normal block statement, which statement_body handles correctly.
                self.statement_body()?
            }
        } else {
            // This is a named function. Its body is always a full statement.
            self.statement_body()?
        };

        if name.is_empty() {
            return Ok(ast::expression_statement(ast::lambda_expression(
                generic_types,
                parameters,
                return_type,
                body,
                properties,
            )));
        }

        Ok(ast::function_declaration(
            &name,
            generic_types,
            parameters,
            return_type,
            body,
            properties,
        ))
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
        ExtendsStatement
            : 'extends' Identifier
            ;
    */
    pub(crate) fn extends_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Extends)?;
        let base = self.inheritance_identifier()?;
        self.eat_statement_end()?;
        Ok(ast::extends(base))
    }

    /*
        ImplementsStatement
            : 'implements' Identifier (',' Identifier)*
            ;
    */
    pub(crate) fn implements_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Implements)?;
        let mut trait_names = vec![self.inheritance_identifier()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            trait_names.push(self.inheritance_identifier()?);
        }
        self.eat_statement_end()?;
        Ok(ast::implements(trait_names))
    }

    /*
        IncludesStatement
            : 'includes' Identifier (',' Identifier)*
            ;
    */
    pub(crate) fn includes_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Includes)?;
        let mut module_names = vec![self.inheritance_identifier()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            module_names.push(self.inheritance_identifier()?);
        }
        self.eat_statement_end()?;
        Ok(ast::includes(module_names))
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
