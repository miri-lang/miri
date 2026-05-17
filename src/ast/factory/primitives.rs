// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::{expr, expr_with_span, stmt};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::program::Program;
use crate::ast::statement::{Statement, StatementKind};
use crate::error::syntax::Span;

/// Creates an identifier expression with a specific span.
pub fn identifier_with_span(name: &str, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Identifier(name.into(), None), span)
}

/// Creates an identifier expression with an optional class qualifier and span.
pub fn identifier_with_class_and_span(name: &str, class: Option<String>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Identifier(name.into(), class), span)
}

/// Creates a literal expression with a specific span.
pub fn literal_with_span(value: Literal, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Literal(value), span)
}

/// Creates an empty statement.
pub fn empty_statement() -> Statement {
    stmt(StatementKind::Empty)
}

/// Creates a program from a list of statements.
pub fn program(statements: Vec<Statement>) -> Program {
    Program { body: statements }
}

/// Creates an initially empty list of statements.
pub fn empty_program() -> Vec<Statement> {
    vec![]
}

/// Creates an identifier expression with an optional class qualifier.
pub fn identifier_with_class(name: &str, class: Option<String>) -> Expression {
    expr(ExpressionKind::Identifier(name.into(), class))
}

/// Creates a simple identifier expression.
pub fn identifier(name: &str) -> Expression {
    identifier_with_class(name, None)
}

/// Creates a literal expression.
pub fn literal(value: Literal) -> Expression {
    expr(ExpressionKind::Literal(value))
}

/// Creates a class identifier (e.g., `Class::StaticMember`).
///
/// Input without a `::` separator returns a plain unqualified identifier.
pub fn class_identifier(name: &str) -> Expression {
    let Some((class, id_name)) = name.split_once("::") else {
        return identifier(name);
    };
    expr(ExpressionKind::Identifier(
        id_name.to_string(),
        Some(class.to_string()),
    ))
}

/// Creates a super expression for calling parent class methods.
pub fn super_expression() -> Expression {
    expr(ExpressionKind::Super)
}

/// Creates a super expression with a specific span.
pub fn super_expression_with_span(span: Span) -> Expression {
    expr_with_span(ExpressionKind::Super, span)
}
