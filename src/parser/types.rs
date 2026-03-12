// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::Parser;

impl<'source> Parser<'source> {
    /*
        TypeExpression
            : Identifier ('<' TypeExpression ',' TypeExpression* '>')? '?'?
            | '[' TypeExpression ']' '?'?
            | '(' TypeExpression ',' TypeExpression* ')' '?'?
            | '{' TypeExpression '}' '?'?
            | '{' TypeExpression ':' TypeExpression* '}' '?'?
            ;
    */
    pub(crate) fn type_expression(&mut self) -> Result<Option<Expression>, SyntaxError> {
        if self._lookahead.is_none() {
            return Ok(None);
        }

        let base_typ_expr: Option<Expression> = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let (type_name, span) = self.identifier_to_type_name()?;
                let typ = self.type_name_to_type(type_name)?;
                Some(ast::type_expression_with_span(typ, false, span))
            }
            Some((Token::LBracket, _)) => {
                self.eat_token(&Token::LBracket)?;
                let element_type = self.element_type_expression("List or Array element type")?;

                if self.match_lookahead_type(|t| t == &Token::Semicolon) {
                    // It's a [Type; Size] array
                    self.eat_token(&Token::Semicolon)?;
                    let size_expr = self.expression()?;
                    self.eat_token(&Token::RBracket)?;
                    Some(ast::type_expr_non_null(ast::make_type(TypeKind::Array(
                        Box::new(element_type),
                        Box::new(size_expr),
                    ))))
                } else {
                    // It's a [Type] list
                    self.eat_token(&Token::RBracket)?;
                    Some(ast::type_expr_non_null(ast::make_type(TypeKind::List(
                        Box::new(element_type),
                    ))))
                }
            }
            Some((Token::LParen, _)) => {
                self.eat_token(&Token::LParen)?;
                if self.lookahead_is_rparen() {
                    self.eat_token(&Token::RParen)?;
                    // Empty tuple type `()`
                    return Ok(Some(ast::type_expr_non_null(ast::make_type(
                        TypeKind::Tuple(vec![]),
                    ))));
                }

                let first_element =
                    self.element_type_expression("Grouped type or tuple element")?;

                if self.lookahead_is_comma() {
                    let mut elements = vec![first_element];
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        if self.lookahead_is_rparen() {
                            break;
                        } // Allow trailing comma
                        elements.push(self.element_type_expression("Tuple element type")?);
                    }
                    self.eat_token(&Token::RParen)?;
                    Some(ast::type_expr_non_null(ast::make_type(TypeKind::Tuple(
                        elements,
                    ))))
                } else {
                    self.eat_token(&Token::RParen)?;
                    Some(first_element)
                }
            }
            Some((Token::LBrace, _)) => {
                self.eat_token(&Token::LBrace)?;
                let key_type = self.element_type_expression("Map key type")?;
                let typ = if self.match_lookahead_type(|t| t == &Token::Colon) {
                    self.eat_token(&Token::Colon)?;
                    let value_type = self.element_type_expression("Map value type")?;
                    self.eat_token(&Token::RBrace)?;
                    TypeKind::Map(Box::new(key_type), Box::new(value_type))
                } else {
                    self.eat_token(&Token::RBrace)?;
                    TypeKind::Set(Box::new(key_type))
                };
                Some(ast::type_expr_non_null(ast::make_type(typ)))
            }
            Some((Token::Fn, _)) => {
                self.eat_token(&Token::Fn)?;
                let generic_types = self.generic_types_expression()?;

                self.eat_token(&Token::LParen)?;
                let mut parameters = Vec::new();
                if !self.lookahead_is_rparen() {
                    loop {
                        if self.lookahead_is_rparen() {
                            break;
                        }

                        // Parse first type expression
                        let first_type_expr = if let Some(typ) = self.type_expression()? {
                            typ
                        } else {
                            return Err(self.error_missing_type_expression());
                        };

                        // Check what follows to decide if first_type_expr is a name or a type
                        let is_named_param =
                            if self.lookahead_is_comma() || self.lookahead_is_rparen() {
                                false
                            } else {
                                // If it's not comma or rparen, it must be the start of another type expression
                                // Check if the next token can start a type expression
                                self.match_lookahead_type(|t| {
                                    matches!(
                                        t,
                                        Token::Identifier
                                            | Token::LBracket
                                            | Token::LParen
                                            | Token::LBrace
                                            | Token::Fn
                                    )
                                })
                            };

                        if is_named_param {
                            // The first expression was the name.
                            let param_name = if let ExpressionKind::Type(ty, is_nullable) =
                                &first_type_expr.node
                            {
                                if *is_nullable {
                                    return Err(self.error_unexpected_token(
                                        "Parameter name cannot be nullable",
                                        "identifier",
                                    ));
                                }
                                match &ty.kind {
                                    TypeKind::Custom(name, None) => name.clone(),
                                    _ => {
                                        return Err(self.error_unexpected_token(
                                            "Parameter name must be a simple identifier",
                                            "identifier",
                                        ));
                                    }
                                }
                            } else {
                                return Err(self.error_unexpected_token(
                                    "Expected parameter name",
                                    "identifier",
                                ));
                            };

                            // Now parse the actual type
                            let param_type = if let Some(typ) = self.type_expression()? {
                                typ
                            } else {
                                return Err(self.error_missing_type_expression());
                            };

                            parameters.push(Parameter {
                                name: param_name,
                                typ: Box::new(param_type),
                                guard: None,
                                default_value: None,
                            });
                        } else {
                            // Unnamed parameter
                            parameters.push(Parameter {
                                name: "".to_string(),
                                typ: Box::new(first_type_expr),
                                guard: None,
                                default_value: None,
                            });
                        }

                        if self.lookahead_is_comma() {
                            self.eat_token(&Token::Comma)?;
                            // Allow trailing comma
                            if self.lookahead_is_rparen() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.eat_token(&Token::RParen)?;

                let return_type = self.return_type_expression()?;
                let typ = TypeKind::Function(Box::new(FunctionTypeData {
                    generics: generic_types,
                    params: parameters,
                    return_type,
                }));
                Some(ast::type_expr_non_null(ast::make_type(typ)))
            }
            _ => return Ok(None),
        };

        let mut final_expr = match base_typ_expr {
            Some(expr) => expr,
            None => return Ok(None),
        };

        if self.match_lookahead_type(|t| t == &Token::QuestionMark) {
            let base_span = final_expr.span;
            let (_, q_span) = self.eat_token(&Token::QuestionMark)?;
            let combined_span = Span::new(base_span.start, q_span.end);
            if let ExpressionKind::Type(inner_type, _) = final_expr.node {
                final_expr = ast::type_expression_with_span(*inner_type, true, combined_span);
            }
        }

        Ok(Some(final_expr))
    }

    pub(crate) fn type_name_to_type(&mut self, type_name: String) -> Result<Type, SyntaxError> {
        Ok(match type_name.as_str() {
            "int" => ast::make_type(TypeKind::Int),
            "i8" => ast::make_type(TypeKind::I8),
            "i16" => ast::make_type(TypeKind::I16),
            "i32" => ast::make_type(TypeKind::I32),
            "i64" => ast::make_type(TypeKind::I64),
            "i128" => ast::make_type(TypeKind::I128),
            "u8" => ast::make_type(TypeKind::U8),
            "u16" => ast::make_type(TypeKind::U16),
            "u32" => ast::make_type(TypeKind::U32),
            "u64" => ast::make_type(TypeKind::U64),
            "u128" => ast::make_type(TypeKind::U128),
            "float" => ast::make_type(TypeKind::Float),
            "f32" => ast::make_type(TypeKind::F32),
            "f64" => ast::make_type(TypeKind::F64),
            "String" => ast::make_type(TypeKind::String),
            "bool" => ast::make_type(TypeKind::Boolean),
            "RawPtr" => ast::make_type(TypeKind::RawPtr),
            "Result" | "result" => self.generic_two_types_expression(
                "Ok result type",
                "Error result type",
                TypeKind::Result,
            )?,
            "Map" => {
                self.generic_two_types_expression("Map key type", "Map value type", TypeKind::Map)?
            }
            "Future" => self.generic_one_type_expression("Future result type", TypeKind::Future)?,
            "Array" => {
                if self.lookahead_is_less_than() {
                    self.generic_array_type_expression()?
                } else {
                    ast::make_type(TypeKind::Custom(type_name, None))
                }
            }
            "List" => {
                if self.lookahead_is_less_than() {
                    self.generic_one_type_expression("List element type", TypeKind::List)?
                } else {
                    ast::make_type(TypeKind::Custom(type_name, None))
                }
            }
            "Set" => self.generic_one_type_expression("Set element type", TypeKind::Set)?,
            // "Option" is handled as a Custom type and resolved by the type checker
            // via resolve_builtin_type_alias, so it falls through to the default case.
            "Tuple" => {
                let inner = self.multiple_element_type_expressions(
                    "Tuple item type",
                    &Token::LessThan,
                    &Token::GreaterThan,
                )?;
                ast::make_type(TypeKind::Tuple(inner))
            }
            "fn" => {
                // fn<T>(int) int
                let generic_types = self.generic_types_expression()?;

                // Parse parameter types, not full parameters
                self.eat_token(&Token::LParen)?;
                let mut parameters = Vec::new();
                if !self.lookahead_is_rparen() {
                    loop {
                        if self.lookahead_is_rparen() {
                            break;
                        }

                        if let Some(typ) = self.type_expression()? {
                            parameters.push(Parameter {
                                name: "".to_string(),
                                typ: Box::new(typ),
                                guard: None,
                                default_value: None,
                            });
                        } else {
                            return Err(self.error_missing_type_expression());
                        }

                        if self.lookahead_is_comma() {
                            self.eat_token(&Token::Comma)?;
                            // Allow trailing comma
                            if self.lookahead_is_rparen() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.eat_token(&Token::RParen)?;

                let return_type = self.return_type_expression()?;
                ast::make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: generic_types,
                    params: parameters,
                    return_type,
                })))
            }
            _ => match &self._lookahead {
                Some((Token::LessThan, _)) => {
                    let inner = self.multiple_element_type_expressions(
                        "Generic type",
                        &Token::LessThan,
                        &Token::GreaterThan,
                    )?;
                    ast::make_type(TypeKind::Custom(type_name, Some(inner)))
                }
                _ => ast::make_type(TypeKind::Custom(type_name, None)),
            },
        })
    }

    pub(crate) fn identifier_to_type_name(&mut self) -> Result<(String, Span), SyntaxError> {
        let ident = self.identifier()?;
        let span = ident.span;
        Ok(match ident.node {
            ExpressionKind::Identifier(id, Some(class)) => {
                // Pre-allocate string instead of using format! to minimize heap allocations
                // and avoid runtime format string parsing in this hot parser path.
                let mut path = String::with_capacity(class.len() + 2 + id.len());
                path.push_str(&class);
                path.push_str("::");
                path.push_str(&id);
                (path, span)
            }
            ExpressionKind::Identifier(id, None) => (id, span),
            _ => return Err(self.error_unexpected_token("identifier", &self.lookahead_as_string())),
        })
    }

    pub(crate) fn element_type_expression(
        &mut self,
        expected: &str,
    ) -> Result<Expression, SyntaxError> {
        let element_type = match self.type_expression()? {
            Some(typ) => typ,
            None => return Err(self.error_invalid_type_declaration(expected)),
        };
        Ok(element_type)
    }

    pub(crate) fn multiple_element_type_expressions(
        &mut self,
        expected: &str,
        left_token: &Token,
        right_token: &Token,
    ) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(left_token)?;

        let element_type = self.element_type_expression(expected)?;
        let mut elements = vec![element_type];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            match &self._lookahead {
                Some((t, _)) if t == right_token => break, // Allow trailing comma
                None => break,
                _ => {
                    let element_type = self.element_type_expression(expected)?;
                    elements.push(element_type);
                }
            }
        }

        self.eat_token(right_token)?;

        Ok(elements)
    }

    pub(crate) fn generic_one_type_expression<F>(
        &mut self,
        expected: &str,
        create_type: F,
    ) -> Result<Type, SyntaxError>
    where
        F: FnOnce(Box<Expression>) -> TypeKind,
    {
        self.eat_token(&Token::LessThan)?;
        let inner_type = self.element_type_expression(expected)?;
        self.eat_token(&Token::GreaterThan)?;
        Ok(ast::make_type(create_type(Box::new(inner_type))))
    }

    pub(crate) fn generic_two_types_expression<F>(
        &mut self,
        expected_a: &str,
        expected_b: &str,
        create_type: F,
    ) -> Result<Type, SyntaxError>
    where
        F: FnOnce(Box<Expression>, Box<Expression>) -> TypeKind,
    {
        self.eat_token(&Token::LessThan)?;
        let a_type = self.element_type_expression(expected_a)?;
        self.eat_token(&Token::Comma)?;
        let b_type = self.element_type_expression(expected_b)?;
        self.eat_token(&Token::GreaterThan)?;

        Ok(ast::make_type(create_type(
            Box::new(a_type),
            Box::new(b_type),
        )))
    }

    pub(crate) fn generic_array_type_expression(&mut self) -> Result<Type, SyntaxError> {
        self.eat_token(&Token::LessThan)?;
        let element_type = self.element_type_expression("Array element type")?;
        self.eat_token(&Token::Comma)?;
        let size_expr = self.additive_expression()?;
        self.eat_token(&Token::GreaterThan)?;

        Ok(ast::make_type(TypeKind::Array(
            Box::new(element_type),
            Box::new(size_expr),
        )))
    }
}
