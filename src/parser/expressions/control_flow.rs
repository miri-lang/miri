// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    /*
     */
    pub(crate) fn conditional_expression(&mut self) -> Result<Expression, SyntaxError> {
        let expression = self.null_coalesce_expression()?;

        // Block-like expressions (e.g. match) should not consume a postfix `if`/`unless`,
        // because `if` after a match block is always a new statement.
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

        // The condition is also a full expression, which will be parsed with its own precedence.
        let condition = self.conditional_expression()?;

        // The `else` part is optional for a postfix modifier `if`.
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
        let expression =
            ast::conditional_with_span(expression, condition, else_branch, if_statement_type, span);

        Ok(expression)
    }

    /// Parses a prefix `if`/`unless` expression.
    ///
    /// Supports two forms:
    /// - Inline: `if condition: then_value else: else_value`
    /// - Block: `if condition\n    then_value\nelse\n    else_value`
    ///
    /// Returns a `Conditional` expression node with the condition first.
    /// Parses a block expression body: a sequence of statements/expressions in an
    /// indented block. The last line is treated as the return expression.
    /// Returns a single expression (or a `Block` expression if there are preceding statements).
    fn block_expression_body(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Indent)?;

        let mut statements: Vec<Statement> = Vec::new();

        loop {
            if self.lookahead_is_dedent() {
                break;
            }

            // Try parsing a statement that isn't an expression
            let is_statement_start = matches!(
                &self._lookahead,
                Some((Token::Let, _)) | Some((Token::Var, _)) | Some((Token::Const, _))
            );

            if is_statement_start {
                let stmt = self.statement()?;
                self.try_eat_expression_end();
                statements.push(stmt);
            } else {
                // Parse as expression
                let expr = self.expression()?;
                self.try_eat_expression_end();

                // If next is dedent, this is the final expression
                if self.lookahead_is_dedent() {
                    self.eat_token(&Token::Dedent)?;
                    if statements.is_empty() {
                        return Ok(expr);
                    } else {
                        let span = expr.span;
                        return Ok(ast::expr_with_span(
                            ExpressionKind::Block(statements, Box::new(expr)),
                            span,
                        ));
                    }
                }

                // Not the last line — wrap expression as a statement
                statements.push(ast::expression_statement(expr));
            }
        }

        // Dedent reached without a final expression
        self.eat_token(&Token::Dedent)?;
        Err(self.error_unexpected_lookahead_token("an expression as the last line of the block"))
    }

    pub(crate) fn prefix_if_expression(&mut self) -> Result<Expression, SyntaxError> {
        let if_type = if self.match_lookahead_type(|t| t == &Token::Unless) {
            self.eat_token(&Token::Unless)?;
            IfStatementType::Unless
        } else {
            self.eat_token(&Token::If)?;
            IfStatementType::If
        };

        let condition = self.expression()?;

        // Parse the then branch
        let then_expr = if self.lookahead_is_colon() {
            // Inline form: if condition: value
            self.eat_token(&Token::Colon)?;
            self.expression()?
        } else if self.lookahead_is_expression_end() {
            // Block form: if condition\n    <block>
            self.eat_expression_end()?;
            if self.lookahead_is_indent() {
                self.block_expression_body()?
            } else {
                return Err(self.error_unexpected_lookahead_token("an indented block"));
            }
        } else {
            return Err(self.error_unexpected_lookahead_token("':' or newline after if condition"));
        };

        self.try_eat_expression_end();

        // Parse the else branch
        let else_expr = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            if self.lookahead_is_colon() {
                // Inline: else: value
                self.eat_token(&Token::Colon)?;
                Some(self.expression()?)
            } else if self.lookahead_is_expression_end() {
                // Block: else\n    <block>
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    Some(self.block_expression_body()?)
                } else {
                    None
                }
            } else if self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
                // else if ...
                Some(self.prefix_if_expression()?)
            } else {
                None
            }
        } else {
            None
        };

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

    /*
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
            } else {
                // Consume optional terminator (newline) between branches
                self.try_eat_expression_end();

                // Re-check termination condition after consuming terminator
                if self._lookahead.is_none() || self.lookahead_is_dedent() {
                    break;
                }

                if self.lookahead_is_indent() {
                    // If we encounter an indent here, it must be an empty block (trailing whitespace/comment)
                    // that produced an INDENT-DEDENT pair. We consume it and continue.
                    self.eat_token(&Token::Indent)?;
                    self.eat_token(&Token::Dedent)?;
                    continue;
                }
            }
            branches.push(self.match_branch()?);
        }

        Ok(branches)
    }

    /*
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
            is_mutable: false,
        })
    }
}
