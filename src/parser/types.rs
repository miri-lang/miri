// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::types::{
    primitive_type_kind, BuiltinCollectionKind, FunctionTypeData, Type, TypeKind, RESULT_TYPE_NAME,
    TUPLE_TYPE_NAME,
};
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::Parser;

const FUTURE_TYPE_NAME: &str = "Future";

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
        let base = match &self.lookahead {
            None => return Ok(None),
            Some((Token::Identifier, _)) => self.identifier_type()?,
            Some((Token::LBracket, _)) => self.bracket_type()?,
            Some((Token::LParen, _)) => self.paren_type()?,
            Some((Token::LBrace, _)) => self.brace_type()?,
            Some((Token::Fn, _)) => self.fn_type()?,
            _ => return Ok(None),
        };

        Ok(Some(self.apply_nullable_suffix(base)?))
    }

    fn identifier_type(&mut self) -> Result<Expression, SyntaxError> {
        let (type_name, span) = self.identifier_to_type_name()?;
        let typ = self.type_name_to_type(type_name)?;
        Ok(ast::type_expression_with_span(typ, false, span))
    }

    fn bracket_type(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBracket)?;
        let element_type = self.element_type_expression("List or Array element type")?;

        if self.match_lookahead_type(|t| t == &Token::Semicolon) {
            self.eat_token(&Token::Semicolon)?;
            let size_expr = self.expression()?;
            self.eat_token(&Token::RBracket)?;
            return Ok(ast::type_expr_non_null(ast::make_type(TypeKind::Array(
                Box::new(element_type),
                Box::new(size_expr),
            ))));
        }

        self.eat_token(&Token::RBracket)?;
        Ok(ast::type_expr_non_null(ast::make_type(TypeKind::List(
            Box::new(element_type),
        ))))
    }

    fn paren_type(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LParen)?;

        if self.lookahead_is_rparen() {
            self.eat_token(&Token::RParen)?;
            return Ok(ast::type_expr_non_null(ast::make_type(TypeKind::Tuple(
                vec![],
            ))));
        }

        let first = self.element_type_expression("Grouped type or tuple element")?;

        if !self.lookahead_is_comma() {
            self.eat_token(&Token::RParen)?;
            return Ok(first);
        }

        let mut elements = vec![first];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            if self.lookahead_is_rparen() {
                break;
            }
            elements.push(self.element_type_expression("Tuple element type")?);
        }
        self.eat_token(&Token::RParen)?;

        Ok(ast::type_expr_non_null(ast::make_type(TypeKind::Tuple(
            elements,
        ))))
    }

    fn brace_type(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBrace)?;
        let key_type = self.element_type_expression("Map key type")?;
        let kind = if self.match_lookahead_type(|t| t == &Token::Colon) {
            self.eat_token(&Token::Colon)?;
            let value_type = self.element_type_expression("Map value type")?;
            self.eat_token(&Token::RBrace)?;
            TypeKind::Map(Box::new(key_type), Box::new(value_type))
        } else {
            self.eat_token(&Token::RBrace)?;
            TypeKind::Set(Box::new(key_type))
        };
        Ok(ast::type_expr_non_null(ast::make_type(kind)))
    }

    fn fn_type(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Fn)?;
        let generics = self.generic_types_expression()?;

        self.eat_token(&Token::LParen)?;
        let params = self.fn_type_parameter_list()?;
        self.eat_token(&Token::RParen)?;

        let return_type = self.return_type_expression()?;
        let kind = TypeKind::Function(Box::new(FunctionTypeData {
            generics,
            params,
            return_type,
        }));
        Ok(ast::type_expr_non_null(ast::make_type(kind)))
    }

    fn fn_type_parameter_list(&mut self) -> Result<Vec<Parameter>, SyntaxError> {
        let mut parameters = Vec::new();
        if self.lookahead_is_rparen() {
            return Ok(parameters);
        }
        loop {
            if self.lookahead_is_rparen() {
                break;
            }
            parameters.push(self.fn_type_parameter()?);
            if self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.lookahead_is_rparen() {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(parameters)
    }

    fn fn_type_parameter(&mut self) -> Result<Parameter, SyntaxError> {
        let first = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_type_expression())?;

        if !self.is_named_param_separator() {
            return Ok(unnamed_param(first));
        }

        let param_name = extract_param_name(&first)
            .ok_or_else(|| self.error_unexpected_token("identifier", "parameter name"))?;

        let typ = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_type_expression())?;

        Ok(Parameter {
            name: param_name,
            typ: Box::new(typ),
            guard: None,
            default_value: None,
            is_out: false,
            residency: None,
        })
    }

    fn is_named_param_separator(&self) -> bool {
        if self.lookahead_is_comma() || self.lookahead_is_rparen() {
            return false;
        }
        self.match_lookahead_type(|t| {
            matches!(
                t,
                Token::Identifier | Token::LBracket | Token::LParen | Token::LBrace | Token::Fn
            )
        })
    }

    fn apply_nullable_suffix(&mut self, expr: Expression) -> Result<Expression, SyntaxError> {
        if !self.match_lookahead_type(|t| t == &Token::QuestionMark) {
            return Ok(expr);
        }
        let base_span = expr.span;
        let (_, q_span) = self.eat_token(&Token::QuestionMark)?;
        let combined = Span::new(base_span.start, q_span.end);
        if let ExpressionKind::Type(inner, _) = expr.node {
            return Ok(ast::type_expression_with_span(*inner, true, combined));
        }
        Ok(expr)
    }

    pub(crate) fn type_name_to_type(&mut self, type_name: String) -> Result<Type, SyntaxError> {
        if let Some(kind) = primitive_type_kind(&type_name) {
            return Ok(ast::make_type(kind));
        }
        if type_name == RESULT_TYPE_NAME || type_name == "result" {
            // `Result<T, E>` is normalized to `Custom("Result", [T, E])` so the
            // parser, type checker, and codegen share one representation.
            return self.generic_two_types_expression(
                "Ok result type",
                "Error result type",
                |ok, err| TypeKind::Custom(RESULT_TYPE_NAME.to_string(), Some(vec![*ok, *err])),
            );
        }
        if type_name == FUTURE_TYPE_NAME {
            return self.generic_one_type_expression("Future result type", TypeKind::Future);
        }
        if type_name == TUPLE_TYPE_NAME {
            let inner = self.multiple_element_type_expressions(
                "Tuple item type",
                &Token::LessThan,
                &Token::GreaterThan,
            )?;
            return Ok(ast::make_type(TypeKind::Tuple(inner)));
        }

        if let Some(collection) = BuiltinCollectionKind::from_name(&type_name) {
            return self.builtin_collection_type(collection, type_name);
        }

        self.custom_named_type(type_name)
    }

    fn builtin_collection_type(
        &mut self,
        collection: BuiltinCollectionKind,
        type_name: String,
    ) -> Result<Type, SyntaxError> {
        match collection {
            BuiltinCollectionKind::Array => {
                if self.lookahead_is_less_than() {
                    self.generic_array_type_expression()
                } else {
                    Ok(ast::make_type(TypeKind::Custom(type_name, None)))
                }
            }
            BuiltinCollectionKind::List => {
                if self.lookahead_is_less_than() {
                    self.generic_one_type_expression("List element type", TypeKind::List)
                } else {
                    Ok(ast::make_type(TypeKind::Custom(type_name, None)))
                }
            }
            BuiltinCollectionKind::Map => {
                self.generic_two_types_expression("Map key type", "Map value type", TypeKind::Map)
            }
            BuiltinCollectionKind::Set => {
                self.generic_one_type_expression("Set element type", TypeKind::Set)
            }
        }
    }

    fn custom_named_type(&mut self, type_name: String) -> Result<Type, SyntaxError> {
        match &self.lookahead {
            Some((Token::LessThan, _)) => {
                let inner = self.multiple_generic_arguments()?;
                Ok(ast::make_type(TypeKind::Custom(type_name, Some(inner))))
            }
            _ => Ok(ast::make_type(TypeKind::Custom(type_name, None))),
        }
    }

    /// Parse the `< ... , ... >` argument list of a generic instantiation. Each
    /// argument is either a type expression (the common case: `Foo<int>`,
    /// `Foo<List<T>>`) or a value expression for compile-time value generics
    /// (`Foo<3>`, `Foo<float, 3>`). The decision is structural: anything that
    /// successfully parses as a type via `type_expression()` is treated as a
    /// type arg; otherwise we fall back to `additive_expression()` and store
    /// the value expression verbatim. Substitution downstream (e.g. through
    /// `Array`'s size slot) walks the stored expressions and only swaps in
    /// values where the surrounding type position expects an expression.
    pub(crate) fn multiple_generic_arguments(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(&Token::LessThan)?;
        // Match the legacy "Generic type" error path when the argument list
        // is empty (`Foo<>`). Falling straight into `generic_argument` would
        // surface a confusing additive-expression error after the type
        // expression branch returns None.
        if let Some((Token::GreaterThan, _)) = &self.lookahead {
            return Err(self.error_invalid_type_declaration("Generic type"));
        }
        let mut elements = vec![self.generic_argument()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            match &self.lookahead {
                Some((Token::GreaterThan, _)) => break,
                None => break,
                _ => elements.push(self.generic_argument()?),
            }
        }
        self.eat_token(&Token::GreaterThan)?;
        Ok(elements)
    }

    fn generic_argument(&mut self) -> Result<Expression, SyntaxError> {
        if let Some(type_expr) = self.type_expression()? {
            // A bare-identifier type slot can be the head of a const-foldable
            // value expression (`Array<f32, W * W>`): `type_expression` greedily
            // reads `W` as a named type, so an arithmetic operator following it
            // means the slot is really a value. Resume expression parsing with
            // the identifier as the left operand. A literal-headed slot
            // (`Array<f32, 8 * 8>`) never reaches here — `type_expression`
            // returns `None` for a number and the fallback below parses it.
            if self.lookahead_continues_value_generic() {
                if let Some(operand) = value_operand_from_type(&type_expr) {
                    return self.value_generic_tail(operand);
                }
            }
            return Ok(type_expr);
        }
        // Not a type — must be a value generic (e.g. integer literal for a
        // const-size slot). Fall back to a regular expression. The type
        // checker rejects values that flow into pure-type positions.
        self.additive_expression()
    }

    /// True when the next token is an arithmetic operator that would continue a
    /// value-generic expression whose first operand was already consumed as a
    /// type (`W * W`, `W + 1`). Mirrors `additive_expression` precedence, so
    /// only multiplicative and additive operators count.
    fn lookahead_continues_value_generic(&self) -> bool {
        self.match_lookahead_type(super::utils::is_multiplicative_op)
            || self.match_lookahead_type(super::utils::is_additive_op)
    }

    /// Continue parsing a const-foldable value-generic expression whose first
    /// operand (`left`) was already consumed while probing for a type. Folds the
    /// multiplicative tail first, then additive terms, matching
    /// `additive_expression` precedence exactly (`W * W + 1` → `(W * W) + 1`).
    fn value_generic_tail(&mut self, left: Expression) -> Result<Expression, SyntaxError> {
        let product = self.binary_expression_tail(
            left,
            Self::unary_expression,
            super::utils::is_multiplicative_op,
            Self::eat_multiplicative_op,
            ast::binary_with_span,
        )?;
        self.binary_expression_tail(
            product,
            Self::multiplicative_expression,
            super::utils::is_additive_op,
            Self::eat_additive_op,
            ast::binary_with_span,
        )
    }

    pub(crate) fn identifier_to_type_name(&mut self) -> Result<(String, Span), SyntaxError> {
        let ident = self.identifier()?;
        let span = ident.span;
        match ident.node {
            ExpressionKind::Identifier(id, Some(class)) => {
                let mut path = String::with_capacity(class.len() + 2 + id.len());
                path.push_str(&class);
                path.push_str("::");
                path.push_str(&id);
                Ok((path, span))
            }
            ExpressionKind::Identifier(id, None) => Ok((id, span)),
            _ => Err(self.error_unexpected_token("identifier", &self.lookahead_as_string())),
        }
    }

    pub(crate) fn element_type_expression(
        &mut self,
        expected: &str,
    ) -> Result<Expression, SyntaxError> {
        self.type_expression()?
            .ok_or_else(|| self.error_invalid_type_declaration(expected))
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
            match &self.lookahead {
                Some((t, _)) if t == right_token => break,
                None => break,
                _ => elements.push(self.element_type_expression(expected)?),
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

fn unnamed_param(typ: Expression) -> Parameter {
    Parameter {
        name: String::new(),
        typ: Box::new(typ),
        guard: None,
        default_value: None,
        is_out: false,
        residency: None,
    }
}

/// Convert a type expression parsed from a value-generic slot back into the
/// identifier operand it really was. Only a bare single-segment custom name
/// (`W`) with no generic arguments and no nullable suffix can head a
/// const-foldable value expression; anything else (a nullable type, a generic
/// instantiation, a collection literal) is returned as `None` so the caller
/// keeps the original type and lets the unexpected operator surface as an error.
fn value_operand_from_type(expr: &Expression) -> Option<Expression> {
    let ExpressionKind::Type(ty, is_nullable) = &expr.node else {
        return None;
    };
    if *is_nullable {
        return None;
    }
    match &ty.kind {
        TypeKind::Custom(name, None) if !name.contains("::") => {
            Some(ast::identifier_with_span(name, expr.span))
        }
        _ => None,
    }
}

fn extract_param_name(expr: &Expression) -> Option<String> {
    let ExpressionKind::Type(ty, is_nullable) = &expr.node else {
        return None;
    };
    if *is_nullable {
        return None;
    }
    match &ty.kind {
        TypeKind::Custom(name, None) => Some(name.clone()),
        _ => None,
    }
}
