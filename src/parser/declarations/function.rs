// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::RuntimeKind;
use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::Token;

use super::super::utils::is_guard;
use super::super::Parser;

impl<'source> Parser<'source> {
    /// Parses function modifier tokens (`async`, `parallel`, `gpu`) and validates
    /// that the resulting combination is legal. Returns a `FunctionProperties` struct
    /// with the parsed modifiers and the given visibility.
    pub(crate) fn function_modifiers(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<FunctionProperties, SyntaxError> {
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
        if properties.is_async && properties.is_parallel {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidModifierCombination {
                    combination: "async parallel".to_string(),
                    reason: "Parallel functions represent a different execution model and cannot be async.".to_string(),
                },
                self.current_token_span(),
            ));
        }

        Ok(properties)
    }

    /*
     */
    pub(crate) fn function_declaration(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.function_declaration_with_context(visibility, false)
    }

    /*
     */
    pub(crate) fn runtime_function_declaration(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Runtime)?;

        // Parse optional runtime name (string literal). Default to "core".
        let runtime_kind = if self.match_lookahead_type(|t| matches!(t, Token::String)) {
            let token = self.eat_token(&Token::String)?;
            let raw = &self.source[token.1.start..token.1.end];
            // Strip surrounding quotes
            let name = &raw[1..raw.len() - 1];
            RuntimeKind::from_name(name).ok_or_else(|| {
                SyntaxError::new(
                    SyntaxErrorKind::UnknownRuntime {
                        name: name.to_string(),
                    },
                    token.1,
                )
            })?
        } else {
            RuntimeKind::Core
        };

        self.eat_token(&Token::Fn)?;

        let name = self.parse_simple_identifier()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        self.eat_statement_end()?;

        Ok(ast::runtime_function_declaration(
            runtime_kind,
            &name,
            parameters,
            return_type,
        ))
    }

    /// Parses a function declaration, optionally allowing abstract functions (no body).
    /// Abstract functions are only valid in traits and abstract classes.
    pub(crate) fn function_declaration_with_context(
        &mut self,
        visibility: MemberVisibility,
        allow_abstract: bool,
    ) -> Result<Statement, SyntaxError> {
        let properties = self.function_modifiers(visibility)?;

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
            let body =
                body.ok_or_else(|| self.error_unexpected_lookahead_token("a lambda body"))?;
            return Ok(ast::expression_statement(ast::lambda_expression(
                generic_types,
                parameters,
                return_type,
                body,
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
     */
    pub(crate) fn parameter(&mut self) -> Result<Parameter, SyntaxError> {
        let name = self.parse_simple_identifier()?;

        let is_out = if self.match_lookahead_type(|t| matches!(t, Token::Out)) {
            self.eat_token(&Token::Out)?;
            true
        } else {
            false
        };

        let typ = match self.type_expression()? {
            Some(typ) => Box::new(typ),
            None if name == "self" => {
                // `fn drop(self)` — bare self with no type annotation.
                // Synthesize a `Self` type so the type checker can resolve it.
                Box::new(crate::ast::factory::type_expr_non_null(
                    crate::ast::factory::make_type(crate::ast::types::TypeKind::Custom(
                        "Self".to_string(),
                        None,
                    )),
                ))
            }
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
            is_out,
        })
    }

    /*
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

        self.eat(is_guard, || "guard operator".to_string())?;
        if self._lookahead.is_some() && self.lookahead_is_in() {
            self.eat_token(&Token::In)?;
            guard_op = GuardOp::NotIn;
        }

        let expression = self.expression()?;
        Ok(ast::guard(guard_op, expression))
    }
}
