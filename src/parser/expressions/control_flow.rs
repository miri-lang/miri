// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn conditional_expression(&mut self) -> Result<Expression, SyntaxError> {
        let expression = self.null_coalesce_expression()?;

        // A match block can't take a postfix `if`/`unless`; the following `if`
        // always starts a new statement.
        if matches!(expression.node, ExpressionKind::Match(..)) {
            return Ok(expression);
        }

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

        let condition = self.conditional_expression()?;

        let else_branch = if self.match_lookahead_type(|t| t == &Token::Else) {
            self.eat_token(&Token::Else)?;
            Some(self.conditional_expression()?)
        } else {
            None
        };

        let span = Span::new(
            expression.span.start,
            if let Some(ref e) = else_branch {
                e.span.end
            } else {
                condition.span.end
            },
        );
        Ok(ast::conditional_with_span(
            expression,
            condition,
            else_branch,
            if_statement_type,
            span,
        ))
    }

    /// Parses an indented block where the last expression is the block's value.
    fn block_expression_body(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Indent)?;

        let mut statements: Vec<Statement> = Vec::new();

        loop {
            if self.lookahead_is_dedent() {
                break;
            }

            let is_statement_start = matches!(
                &self.lookahead,
                Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _))
            );

            if is_statement_start {
                let stmt = self.statement()?;
                self.try_eat_expression_end()?;
                statements.push(stmt);
            } else {
                let expr = self.expression()?;
                self.try_eat_expression_end()?;

                if self.lookahead_is_dedent() {
                    self.eat_token(&Token::Dedent)?;
                    if statements.is_empty() {
                        return Ok(expr);
                    }
                    let span = expr.span;
                    return Ok(ast::expr_with_span(
                        ExpressionKind::Block(statements, Box::new(expr)),
                        span,
                    ));
                }

                statements.push(ast::expression_statement(expr));
            }
        }

        self.eat_token(&Token::Dedent)?;
        Err(self.error_unexpected_lookahead_token("an expression as the last line of the block"))
    }

    /// Parses a prefix `if`/`unless` expression. Inline form: `if c: v else: v`.
    /// Block form: `if c\n  v\nelse\n  v`.
    pub(crate) fn prefix_if_expression(&mut self) -> Result<Expression, SyntaxError> {
        let if_type = if self.match_lookahead_type(|t| t == &Token::Unless) {
            self.eat_token(&Token::Unless)?;
            IfStatementType::Unless
        } else {
            self.eat_token(&Token::If)?;
            IfStatementType::If
        };

        let condition = self.expression()?;
        let then_expr = self.if_expression_branch()?;
        self.try_eat_expression_end()?;
        let else_expr = self.optional_else_branch()?;

        let span = Span::new(
            condition.span.start,
            if let Some(ref e) = else_expr {
                e.span.end
            } else {
                then_expr.span.end
            },
        );

        Ok(ast::conditional_with_span(
            then_expr, condition, else_expr, if_type, span,
        ))
    }

    fn if_expression_branch(&mut self) -> Result<Expression, SyntaxError> {
        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;
            return self.expression();
        }
        if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;
            if self.lookahead_is_indent() {
                return self.block_expression_body();
            }
            return Err(self.error_unexpected_lookahead_token("an indented block"));
        }
        Err(self.error_unexpected_lookahead_token("':' or newline after if condition"))
    }

    fn optional_else_branch(&mut self) -> Result<Option<Expression>, SyntaxError> {
        if !self.lookahead_is_else() {
            return Ok(None);
        }
        self.eat_token(&Token::Else)?;

        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;
            return Ok(Some(self.expression()?));
        }
        if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;
            if self.lookahead_is_indent() {
                return Ok(Some(self.block_expression_body()?));
            }
            return Ok(None);
        }
        if self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
            return Ok(Some(self.prefix_if_expression()?));
        }
        Ok(None)
    }

    pub(crate) fn match_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Match)?;
        let value = self.expression()?;
        let mut branches = Vec::new();

        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;
            if self.lookahead.is_some() {
                branches.extend(self.inline_match_branches()?);
            }
        } else if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;
            if self.lookahead_is_indent() {
                self.eat_token(&Token::Indent)?;
                branches.extend(self.block_match_branches()?);
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

        self.reject_duplicate_branches(&branches)?;

        Ok(ast::match_expression(value, branches))
    }

    fn reject_duplicate_branches(&self, branches: &[MatchBranch]) -> Result<(), SyntaxError> {
        let mut seen = std::collections::HashSet::new();
        for branch in branches {
            for pattern in &branch.patterns {
                let key = (pattern.clone(), branch.guard.clone());
                if !seen.insert(key) {
                    return Err(self.error_duplicate_match_pattern());
                }
            }
        }
        Ok(())
    }

    fn inline_match_branches(&mut self) -> Result<Vec<MatchBranch>, SyntaxError> {
        let mut branches = vec![self.match_branch()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            branches.push(self.match_branch()?);
        }
        Ok(branches)
    }

    fn block_match_branches(&mut self) -> Result<Vec<MatchBranch>, SyntaxError> {
        let mut branches = vec![self.match_branch()?];

        while self.lookahead.is_some() && !self.lookahead_is_dedent() {
            self.try_eat_expression_end()?;
            if self.lookahead.is_none() || self.lookahead_is_dedent() {
                break;
            }
            if self.lookahead_is_indent() {
                // Trailing INDENT/DEDENT pairs leak through from empty blocks
                // (e.g. comment-only lines). Skip them silently.
                self.eat_token(&Token::Indent)?;
                self.eat_token(&Token::Dedent)?;
                continue;
            }
            branches.push(self.match_branch()?);
        }

        Ok(branches)
    }

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
        let body = match &self.lookahead {
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
        self.try_eat_expression_end()?;

        Ok(MatchBranch {
            patterns,
            guard,
            body: Box::new(body),
            is_mutable: false,
        })
    }
}
