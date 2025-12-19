// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::{token_to_string, Token};

use super::utils::{
    is_additive_op, is_assignment_op, is_equality_op, is_logical_and_op, is_logical_or_op,
    is_multiplicative_op, is_relational_op,
};
use super::Parser;

impl<'source> Parser<'source> {
    /*
        Expression
            : AssignmentExpression
            ;
    */
    pub(crate) fn expression(&mut self) -> Result<Expression, SyntaxError> {
        self.assignment_expression()
    }

    /*
        ConditionalExpression
            : LogicalOrExpression
            | LogicalOrExpression 'if' Expression ('else' Expression)
            | LogicalOrExpression 'unless' Expression ('else' Expression)
            ;
    */
    pub(crate) fn conditional_expression(&mut self) -> Result<Expression, SyntaxError> {
        let expression = self.logical_or_expression()?;

        if !self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
            return Ok(expression);
        }

        let if_statement_type = if self.match_lookahead_type(|t| t == &Token::If) {
            self.eat_token(&Token::If)?;
            IfStatementType::If
        } else {
            self.eat_token(&Token::Unless)?;
            IfStatementType::Unless
        };

        // The condition is also a full expression, which will be parsed with its own precedence.
        let condition = self.conditional_expression()?;

        // The `else` part is optional for a postfix modifier `if`.
        let else_branch = if self.match_lookahead_type(|t| t == &Token::Else) {
            self.eat_token(&Token::Else)?;
            Some(self.conditional_expression()?)
        } else {
            None
        };

        let span = expression.span.start..(if let Some(ref e) = else_branch {
            e.span.end
        } else {
            condition.span.end
        });
        let expression =
            ast::conditional_with_span(expression, condition, else_branch, if_statement_type, span);

        Ok(expression)
    }

    /*
        AssignmentExpression
            : ConditionalExpression
            | LeftHandSideExpression ASSIGNMENT_OPERATOR AssignmentExpression
            ;
    */
    pub(crate) fn assignment_expression(&mut self) -> Result<Expression, SyntaxError> {
        let left = self.conditional_expression()?;

        if !self.lookahead_is_assignment_op() {
            return Ok(left);
        }

        let op = match self.eat_binary_op(is_assignment_op) {
            Ok(token) => match token.0 {
                Token::Assign => AssignmentOp::Assign,
                Token::AssignAdd => AssignmentOp::AssignAdd,
                Token::AssignSub => AssignmentOp::AssignSub,
                Token::AssignMul => AssignmentOp::AssignMul,
                Token::AssignDiv => AssignmentOp::AssignDiv,
                Token::AssignMod => AssignmentOp::AssignMod,
                _ => return Err(self.error_unexpected_operator(token, "=, +=, -=, *=, /=, %=")),
            },
            Err(err) => return Err(err),
        };

        let left = match &left.node {
            ExpressionKind::Identifier(_, class) => {
                if class.is_some() {
                    // A left-hand side identifier cannot be namespaced.
                    return Err(self.error_invalid_left_hand_side_expression());
                }
                ast::lhs_identifier_from_expr(left)
            }
            ExpressionKind::Member(_, _) => ast::lhs_member_from_expr(left),
            ExpressionKind::Index(_, _) => ast::lhs_index_from_expr(left),
            // Other left-hand side expression types can be added here in the future
            _ => return Err(self.error_invalid_left_hand_side_expression()),
        };

        let right = self.assignment_expression()?;

        let span = left.span().start..right.span.end;
        let assignment_expression = ast::assign_with_span(left, op, right, span);

        Ok(assignment_expression)
    }

    /*
        x > y
        x < y
        x >= y
        x <= y

        RelationalExpression
            : RangeExpression
            | RangeExpression RELATIONAL_OPERATOR RelationalExpression
            ;
    */
    pub(crate) fn relational_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::range_expression,
            is_relational_op,
            Self::eat_relational_op,
            ast::binary_with_span,
        )
    }

    /*
        x == y
        x != y

        EqualityExpression
            : RelationalExpression EQUALITY_OPERATOR EqualityExpression
            | RelationalExpression
            ;
    */
    pub(crate) fn equality_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::relational_expression,
            is_equality_op,
            Self::eat_equality_op,
            ast::binary_with_span,
        )
    }

    /*
        x and y

        LogicalAndExpression
            : EqualityExpression AND LogicalAndExpression
            | EqualityExpression
            ;
    */
    pub(crate) fn logical_and_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::equality_expression,
            is_logical_and_op,
            Self::eat_logical_and_op,
            ast::logical_with_span,
        )
    }

    /*
        x or y

        LogicalOrExpression
            : LogicalAndExpression OR LogicalOrExpression
            | LogicalOrExpression
            ;
    */
    pub(crate) fn logical_or_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::logical_and_expression,
            is_logical_or_op,
            Self::eat_logical_or_op,
            ast::logical_with_span,
        )
    }

    /*
        LeftHandSideExpression
            : CallMemberExpression
            ;
    */
    pub(crate) fn left_hand_side_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.call_member_expression()
    }

    /*
        CallMemberExpression
            : PrimaryExpression
            | CallMemberExpression '.' Identifier
            | CallMemberExpression '[' Expression ']'
            | CallMemberExpression '(' Arguments ')'
            ;
    */
    pub(crate) fn call_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.primary_expression()?;

        loop {
            if !self.lookahead_is_member_expression_boundary() {
                break;
            }

            expression = match &self._lookahead {
                Some((Token::Dot, _)) => {
                    self.eat_token(&Token::Dot)?;
                    let property = self.identifier()?;
                    let span = expression.span.start..property.span.end;
                    ast::member_with_span(expression, property, span)
                }
                Some((Token::LBracket, _)) => {
                    self.eat_token(&Token::LBracket)?;
                    let index = self.expression()?;
                    let (_, rbracket_span) = self.eat_token(&Token::RBracket)?;
                    let span = expression.span.start..rbracket_span.end;
                    ast::index_with_span(expression, index, span)
                }
                Some((Token::LParen, _)) => {
                    let (args, rparen_span) = self.arguments()?;
                    let span = expression.span.start..rparen_span.end;
                    ast::call_with_span(expression, args, span)
                }
                _ => break,
            };
        }

        Ok(expression)
    }

    /*
        Arguments
            : '(' ')'
            | '(' ArgumentList ')'
    */
    pub(crate) fn arguments(&mut self) -> Result<(Vec<Expression>, Span), SyntaxError> {
        self.eat_token(&Token::LParen)?;

        let argument_list = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.argument_list()?
        };

        let (_, span) = self.eat_token(&Token::RParen)?;
        Ok((argument_list, span))
    }

    /*
        ArgumentList
            : AssignmentExpression
            | ArgumentList ',' AssignmentExpression
    */
    pub(crate) fn argument_list(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        let mut args = Vec::new();

        args.push(self.assignment_expression()?);

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            // Allow an optional trailing comma before the closing parenthesis.
            if self.lookahead_is_rparen() {
                break;
            }
            args.push(self.assignment_expression()?);
        }

        Ok(args)
    }

    /*
        Identifier
            : IDENTIFIER
            | IDENTIFIER '::' IDENTIFIER
            ;
    */
    pub(crate) fn identifier(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let (_, span) = self.eat_token(&Token::Identifier)?;
                let (name, class, full_span) = match &self._lookahead {
                    Some((Token::DoubleColon, _)) => {
                        self.eat_token(&Token::DoubleColon)?;
                        let (_, second_span) = self.eat_token(&Token::Identifier)?;

                        (
                            self.source[second_span.start..second_span.end].to_string(),
                            Some(self.source[span.start..span.end].to_string()),
                            span.start..second_span.end,
                        )
                    }
                    _ => (self.source[span.start..span.end].to_string(), None, span),
                };
                Ok(ast::identifier_with_class_and_span(&name, class, full_span))
            }
            _ => Err(self.error_unexpected_lookahead_token("identifier")),
        }
    }

    pub(crate) fn parse_simple_identifier(&mut self) -> Result<String, SyntaxError> {
        let identifier_expr = self.identifier()?;
        if let ExpressionKind::Identifier(id, class_opt) = identifier_expr.node {
            if let Some(class) = class_opt {
                // A simple identifier cannot be namespaced.
                return Err(self
                    .error_unexpected_token("a simple identifier", &format!("{}::{}", class, id)));
            }
            Ok(id)
        } else {
            // This case should ideally not be reachable if identifier() works correctly
            Err(self.error_unexpected_token("identifier", &format!("{:?}", identifier_expr)))
        }
    }

    /*
        AdditiveExpression
            : MultiplicativeExpression
            | AdditiveExpression ADDITIVE_OPERATOR MultiplicativeExpression
            ;
    */
    pub(crate) fn additive_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::multiplicative_expression,
            is_additive_op,
            Self::eat_additive_op,
            ast::binary_with_span,
        )
    }

    /*
        MultiplicativeExpression
            : UnaryExpression
            | MultiplicativeExpression MULTIPLICATIVE_OPERATOR UnaryExpression
            ;
    */
    pub(crate) fn multiplicative_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::unary_expression,
            is_multiplicative_op,
            Self::eat_multiplicative_op,
            ast::binary_with_span,
        )
    }

    pub(crate) fn _binary_expression<F, G, E>(
        &mut self,
        mut create_branch: F,
        op_predicate: fn(&Token) -> bool,
        mut eat_op: G,
        mut create_expression: E,
    ) -> Result<Expression, SyntaxError>
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
        E: FnMut(Expression, BinaryOp, Expression, Span) -> Expression,
    {
        let mut left = create_branch(self)?;

        while self.match_lookahead_type(op_predicate) {
            let op = match eat_op(self) {
                Ok(value) => value,
                Err(value) => return value,
            };

            let right = create_branch(self)?;

            let span = left.span.start..right.span.end;
            left = create_expression(left, op, right, span);
        }

        Ok(left)
    }

    /*
        UnaryExpression
            : LeftHandSideExpression
            | ADDITIVE_OPERATOR UnaryExpression
            | NOT UnaryExpression
            | AWAIT UnaryExpression
            ;
    */
    pub(crate) fn unary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Plus, _)) => self.create_unary_expression(&Token::Plus, UnaryOp::Plus),
            Some((Token::Minus, _)) => self.create_unary_expression(&Token::Minus, UnaryOp::Negate),
            Some((Token::Not, _)) => self.create_unary_expression(&Token::Not, UnaryOp::Not),
            Some((Token::Tilde, _)) => {
                self.create_unary_expression(&Token::Tilde, UnaryOp::BitwiseNot)
            }
            Some((Token::Decrement, _)) => {
                self.create_unary_expression(&Token::Decrement, UnaryOp::Decrement)
            }
            Some((Token::Increment, _)) => {
                self.create_unary_expression(&Token::Increment, UnaryOp::Increment)
            }
            Some((Token::Await, _)) => self.create_unary_expression(&Token::Await, UnaryOp::Await),
            _ => self.left_hand_side_expression(),
        }
    }

    pub(crate) fn create_unary_expression(
        &mut self,
        token: &Token,
        op: UnaryOp,
    ) -> Result<Expression, SyntaxError> {
        let (_, span) = self.eat_token(token)?;
        let operand = self.unary_expression()?;
        let full_span = span.start..operand.span.end;
        Ok(ast::unary_with_span(op, operand, full_span))
    }

    /*
        PrimaryExpression
            : Literal
            | ParenthesizedExpression
            | Identifier
            ;
    */
    pub(crate) fn primary_expression(&mut self) -> Result<Expression, SyntaxError> {
        if self._lookahead.is_none() {
            return Err(self.error_eof());
        }

        if self.lookahead_is_literal() {
            return self.literal_expression();
        }

        match &self._lookahead {
            Some((Token::LParen, _)) => self.parenthesized_expression(),
            Some((Token::Identifier, _)) => self.identifier(),
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.lambda_expression()
            }
            Some((Token::LBracket, _)) => self.list_literal_expression(),
            Some((Token::LBrace, _)) => self.brace_expression(),
            Some((Token::Match, _)) => self.match_expression(),
            Some((Token::FormattedStringStart(_), _)) => self.formatted_string_expression(),
            _ => Err(self.error_unexpected_lookahead_token("an expression")),
        }
    }

    /*
        FormattedStringExpression
            : FormattedStringStart Expression (FormattedStringMiddle Expression)* FormattedStringEnd
            ;
    */
    pub(crate) fn formatted_string_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut parts = Vec::new();

        let start_token_str = &token_to_string(&Token::FormattedStringStart("".to_string()));
        if let Some((Token::FormattedStringStart(start_text), _)) = self._lookahead.clone() {
            self.eat(
                |t| matches!(t, Token::FormattedStringStart(_)),
                start_token_str,
            )?;
            if !start_text.is_empty() {
                parts.push(ast::literal(ast::string_literal(&start_text)));
            }
        } else {
            return Err(self.error_unexpected_lookahead_token(start_token_str));
        }

        while self._lookahead.is_some() {
            parts.push(self.expression()?);

            if let Some((Token::FormattedStringMiddle(middle_text), _)) = self._lookahead.clone() {
                self.eat(
                    |t| matches!(t, Token::FormattedStringMiddle(_)),
                    &token_to_string(&Token::FormattedStringMiddle("".to_string())),
                )?;
                if !middle_text.is_empty() {
                    parts.push(ast::literal(ast::string_literal(&middle_text)));
                }
            } else if let Some((Token::FormattedStringEnd(end_text), _)) = self._lookahead.clone() {
                self.eat(
                    |t| matches!(t, Token::FormattedStringEnd(_)),
                    &token_to_string(&Token::FormattedStringEnd("".to_string())),
                )?;
                if !end_text.is_empty() {
                    parts.push(ast::literal(ast::string_literal(&end_text)));
                }
                break; // End of the f-string
            } else {
                return Err(
                    self.error_unexpected_lookahead_token("middle or end of a formatted string")
                );
            }
        }

        Ok(ast::f_string(parts))
    }

    /*
        MatchExpression
            : 'match' Expression ':' MatchBranchList
            | 'match' Expression INDENT MatchBranchList DEDENT
            ;
    */
    pub(crate) fn match_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Match)?;
        let value = self.expression()?;
        let mut branches = Vec::new();

        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;
            if self._lookahead.is_some() {
                branches.extend(self.match_branch_list(true)?);
            }
        } else if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;
            if self.lookahead_is_indent() {
                self.eat_token(&Token::Indent)?;
                branches.extend(self.match_branch_list(false)?);
                self.eat_token(&Token::Dedent)?;
            }
        } else {
            return Err(self.error_unexpected_lookahead_token(
                "':' for an inline match or a new line for a block match",
            ));
        }

        if branches.is_empty() {
            return Err(self.error_missing_match_branches());
        }

        // Check for duplicate (pattern, guard) combinations.
        // This catches simple duplicates like `1: ... 1: ...` or
        // `x if x > 10: ... x if x > 10: ...`.
        // A more complex semantic analysis for overlapping or unreachable
        // patterns is left to a later compiler stage.
        let mut seen_pattern_guards = std::collections::HashSet::new();
        for branch in &branches {
            for pattern in &branch.patterns {
                let key = (pattern.clone(), branch.guard.clone());
                if !seen_pattern_guards.insert(key) {
                    // This exact pattern and guard combination has been seen before.
                    return Err(self.error_duplicate_match_pattern());
                }
            }
        }

        Ok(ast::match_expression(value, branches))
    }

    /*
        MatchBranchList
            : MatchBranch (','? MatchBranch)*
            ;
    */
    pub(crate) fn match_branch_list(
        &mut self,
        inline_mode: bool,
    ) -> Result<Vec<MatchBranch>, SyntaxError> {
        let mut branches = vec![self.match_branch()?];

        while (inline_mode && self.lookahead_is_comma())
            || (!inline_mode && self._lookahead.is_some() && !self.lookahead_is_dedent())
        {
            if inline_mode {
                self.eat_token(&Token::Comma)?;
            }
            branches.push(self.match_branch()?);
        }

        Ok(branches)
    }

    /*
        MatchBranch
            : Pattern ('|' Pattern)* ('if' Expression)? (':' Expression | INDENT StatementList DEDENT) EXPRESSION_END
            ;
    */
    pub(crate) fn match_branch(&mut self) -> Result<MatchBranch, SyntaxError> {
        let mut patterns = vec![self.pattern()?];
        while self.match_lookahead_type(|t| t == &Token::Pipe) {
            self.eat_token(&Token::Pipe)?;
            patterns.push(self.pattern()?);
        }

        let guard = if self.match_lookahead_type(|t| t == &Token::If) {
            self.eat_token(&Token::If)?;
            Some(Box::new(self.expression()?))
        } else {
            None
        };

        let body_parsing_error = self.error_unexpected_lookahead_token(
            "a colon for an inline body or an indented block for a block body",
        );
        let body = match &self._lookahead {
            Some((Token::Colon, _)) => {
                self.eat_token(&Token::Colon)?;
                let expr = self.expression()?;
                ast::expression_statement(expr)
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.block_statement()?
                } else {
                    return Err(body_parsing_error);
                }
            }
            _ => return Err(body_parsing_error),
        };
        self.try_eat_expression_end();

        Ok(MatchBranch {
            patterns,
            guard,
            body: Box::new(body),
        })
    }

    /*
        Pattern
            : Literal
            | Identifier
            | TuplePattern
            | 'default'
            ;
    */
    pub(crate) fn pattern(&mut self) -> Result<Pattern, SyntaxError> {
        match &self._lookahead {
            Some((Token::Default, _)) => {
                self.eat_token(&Token::Default)?;
                Ok(Pattern::Default)
            }
            Some((Token::Identifier, _)) => {
                let name = self.parse_simple_identifier()?;
                Ok(Pattern::Identifier(name))
            }
            Some((Token::LParen, _)) => self.tuple_pattern(),
            Some((Token::Regex(_), _)) => {
                if let Literal::Regex(regex_token) = self.regex_literal()? {
                    Ok(Pattern::Regex(regex_token))
                } else {
                    unreachable!()
                }
            }
            _ if self.lookahead_is_literal() => {
                let literal = self.literal()?;
                Ok(Pattern::Literal(literal))
            }
            _ => Err(self
                .error_unexpected_lookahead_token("a pattern (literal, identifier, or default)")),
        }
    }

    /*
        TuplePattern
            : '(' (Pattern (',' Pattern)* ','?)? ')'
            ;
    */
    pub(crate) fn tuple_pattern(&mut self) -> Result<Pattern, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let mut patterns = Vec::new();
        if !self.lookahead_is_rparen() {
            patterns.push(self.pattern()?);
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.lookahead_is_rparen() {
                    break;
                } // Allow trailing comma
                patterns.push(self.pattern()?);
            }
        }
        self.eat_token(&Token::RParen)?;
        Ok(Pattern::Tuple(patterns))
    }

    /*
        ListLiteralExpression
            : '[' (Expression (',' Expression)* ','? )? ']'
            ;
    */
    pub(crate) fn list_literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBracket)?;

        let mut elements = vec![];
        while self.match_lookahead_type(|t| t != &Token::RBracket) {
            elements.push(self.expression()?);
            if !self.lookahead_is_comma() {
                break;
            }
            self.eat_token(&Token::Comma)?;
        }

        self.eat_token(&Token::RBracket)?;
        Ok(ast::list(elements))
    }

    /*
        BraceExpression
            : MapLiteralExpression
            | SetLiteralExpression
            ;
    */
    pub(crate) fn brace_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBrace)?;

        // If the next token is a closing brace, it's an empty map.
        if self.match_lookahead_type(|t| t == &Token::RBrace) {
            self.eat_token(&Token::RBrace)?;
            return Ok(ast::map(vec![]));
        }

        // Parse the first expression.
        let first_expr = self.expression()?;

        // Look ahead for a colon to distinguish between a map and a set.
        if self.lookahead_is_colon() {
            // It's a map.
            self.eat_token(&Token::Colon)?;
            let first_value = self.expression()?;
            let mut pairs = vec![(first_expr, first_value)];

            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.match_lookahead_type(|t| t == &Token::RBrace) {
                    break;
                } // Trailing comma
                let key = self.expression()?;
                self.eat_token(&Token::Colon)?;
                let value = self.expression()?;
                pairs.push((key, value));
            }
            self.eat_token(&Token::RBrace)?;
            Ok(ast::map(pairs))
        } else {
            // It's a set.
            let mut elements = vec![first_expr];
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.match_lookahead_type(|t| t == &Token::RBrace) {
                    break;
                } // Trailing comma
                elements.push(self.expression()?);
            }
            self.eat_token(&Token::RBrace)?;
            Ok(ast::set(elements))
        }
    }

    /*
        LambdaExpression
            : 'async'? 'gpu'? 'fn' [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END BlockStatement
            | 'async'? 'gpu'? 'fn' [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] ':' ExpressionStatement EXPRESSION_END
            ;
    */
    pub(crate) fn lambda_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut properties = FunctionProperties {
            is_async: false,
            is_gpu: false,
            visibility: MemberVisibility::Public,
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
                _ => break,
            }
        }

        self.eat_token(&Token::Fn)?;

        let generic_types = self.generic_types_expression()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        let body_parsing_error = self.error_unexpected_lookahead_token(
            "a colon for an inline body or an indented block for a block body",
        );
        let body = match &self._lookahead {
            Some((Token::Colon, _)) => {
                self.eat_token(&Token::Colon)?;
                let expr = self.expression()?;
                ast::expression_statement(expr)
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.block_statement()?
                } else if self.lookahead_is_dedent() || self._lookahead.is_none() {
                    Statement::Empty // No body, just an expression end
                } else {
                    return Err(body_parsing_error);
                }
            }
            _ => return Err(body_parsing_error),
        };

        Ok(ast::lambda_expression(
            generic_types,
            parameters,
            return_type,
            body,
            properties,
        ))
    }

    /*
        ParenthesizedExpression
            : '(' Expression ')'
            ;
        TupleLiteralExpression
            : '(' (Expression (',' Expression)* ','? )? ')'
            ;
    */
    pub(crate) fn parenthesized_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LParen)?;

        // Handle the empty tuple `()` case.
        if self.match_lookahead_type(|t| t == &Token::RParen) {
            self.eat_token(&Token::RParen)?;
            return Ok(ast::tuple(vec![]));
        }

        let first_expr = self.expression()?;

        // The presence of a comma is what distinguishes a tuple from a grouping parenthesis.
        if !self.lookahead_is_comma() {
            // No comma, so this is a grouping parenthesized expression.
            self.eat_token(&Token::RParen)?;
            return Ok(first_expr);
        }

        // It's a tuple. Start with the first expression we already parsed.
        let mut elements = vec![first_expr];

        // Loop through the rest of the comma-separated expressions.
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            // Handle optional trailing comma before the closing parenthesis.
            if self.match_lookahead_type(|t| t == &Token::RParen) {
                break;
            }
            elements.push(self.expression()?);
        }

        self.eat_token(&Token::RParen)?;
        Ok(ast::tuple(elements))
    }

    /*
        LiteralExpression
            : Literal
            ;
    */
    pub(crate) fn literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        let span = if let Some((_, span)) = &self._lookahead {
            span.clone()
        } else {
            return Err(self.error_eof());
        };
        let literal = self.literal()?;
        Ok(ast::literal_with_span(literal, span))
    }

    /*
        RangeExpression
            : AdditiveExpression
            | AdditiveExpression .. AdditiveExpression
            | AdditiveExpression ..= AdditiveExpression
            ;
    */
    pub(crate) fn range_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.additive_expression()?;

        match &self._lookahead {
            Some((Token::Range, _)) => {
                self.eat_token(&Token::Range)?;
                let end = self.additive_expression()?;
                Ok(ast::range(
                    start,
                    Some(Box::new(end)),
                    RangeExpressionType::Exclusive,
                ))
            }
            Some((Token::RangeInclusive, _)) => {
                self.eat_token(&Token::RangeInclusive)?;
                let end = self.additive_expression()?;
                Ok(ast::range(
                    start,
                    Some(Box::new(end)),
                    RangeExpressionType::Inclusive,
                ))
            }
            _ => Ok(start),
        }
    }

    pub(crate) fn generic_types_expression(
        &mut self,
    ) -> Result<Option<Vec<Expression>>, SyntaxError> {
        let generic_types = if self.lookahead_is_less_than() {
            Some(self.generic_types_declaration()?)
        } else {
            None
        };
        Ok(generic_types)
    }

    pub(crate) fn function_params_expression(&mut self) -> Result<Vec<Parameter>, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let parameters = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.parameter_list()?
        };
        self.eat_token(&Token::RParen)?;

        Ok(parameters)
    }

    pub(crate) fn return_type_expression(
        &mut self,
    ) -> Result<Option<Box<Expression>>, SyntaxError> {
        let return_type = self.type_expression()?.map(Box::new);
        Ok(return_type)
    }
}
