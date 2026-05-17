// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::primitives::identifier;
use super::{expr, stmt, stmt_with_span};
use crate::ast::common::MemberVisibility;
use crate::ast::expression::{Expression, ExpressionKind, ImportPathKind};
use crate::ast::literal::Literal;
use crate::ast::statement::{
    IfStatementType, Statement, StatementKind, VariableDeclaration, VariableDeclarationType,
    WhileStatementType,
};

/// Creates a variable declaration statement.
pub fn variable_statement(
    declarations: Vec<VariableDeclaration>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Variable(declarations, visibility))
}

/// Creates an expression statement (expression used as a statement).
pub fn expression_statement(expression: Expression) -> Statement {
    let span = expression.span;
    stmt_with_span(StatementKind::Expression(expression), span)
}

/// Creates a block statement.
pub fn block(stmts: Vec<Statement>) -> Statement {
    stmt(StatementKind::Block(stmts))
}

/// Creates an `if` statement.
pub fn if_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    stmt(StatementKind::If(
        Box::new(cond),
        Box::new(then),
        else_b.map(Box::new),
        IfStatementType::If,
    ))
}

/// Creates an `unless` statement.
pub fn unless_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    stmt(StatementKind::If(
        Box::new(cond),
        Box::new(then),
        else_b.map(Box::new),
        IfStatementType::Unless,
    ))
}

/// Creates a loop statement of a specific type (while, do-while, etc).
pub fn while_statement_with_type(
    cond: Expression,
    body: Statement,
    while_statement_type: WhileStatementType,
) -> Statement {
    stmt(StatementKind::While(
        Box::new(cond),
        Box::new(body),
        while_statement_type,
    ))
}

/// Creates a `while` loop statement.
pub fn while_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::While)
}

/// Creates a `do-while` loop statement.
pub fn do_while_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::DoWhile)
}

/// Creates an `until` loop statement.
pub fn until_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::Until)
}

/// Creates an infinite loop statement.
pub fn forever_statement(body: Statement) -> Statement {
    while_statement_with_type(
        expr(ExpressionKind::Literal(Literal::Boolean(true))),
        body,
        WhileStatementType::Forever,
    )
}

/// Creates a `for` loop statement.
pub fn for_statement(
    variable_declarations: Vec<VariableDeclaration>,
    iterable: Expression,
    body: Statement,
) -> Statement {
    stmt(StatementKind::For(
        variable_declarations,
        Box::new(iterable),
        Box::new(body),
    ))
}

/// Creates a return statement.
pub fn return_statement(expression: Option<Box<Expression>>) -> Statement {
    stmt(StatementKind::Return(expression))
}

/// Creates a break statement.
pub fn break_statement() -> Statement {
    stmt(StatementKind::Break)
}

/// Creates a continue statement.
pub fn continue_statement() -> Statement {
    stmt(StatementKind::Continue)
}

/// Creates an immutable variable declaration structure.
pub fn let_variable(
    name: &str,
    typ: Option<Box<Expression>>,
    init: Option<Box<Expression>>,
) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Immutable,
        is_shared: false,
    }
}

/// Creates a mutable variable declaration structure.
pub fn var(
    name: &str,
    typ: Option<Box<Expression>>,
    init: Option<Box<Expression>>,
) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Mutable,
        is_shared: false,
    }
}

/// Creates a compile-time constant declaration structure.
pub fn const_variable(
    name: &str,
    typ: Option<Box<Expression>>,
    init: Option<Box<Expression>>,
) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Constant,
        is_shared: false,
    }
}

/// Creates a type alias statement.
pub fn type_statement(declarations: Vec<Expression>, visibility: MemberVisibility) -> Statement {
    stmt(StatementKind::Type(declarations, visibility))
}

/// Creates an import path expression.
pub fn import_path_expression(segments: Vec<Expression>, kind: ImportPathKind) -> Expression {
    expr(ExpressionKind::ImportPath(segments, kind))
}

/// Creates a simple import path from a dotted string.
pub fn import_path(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Simple)
}

/// Creates a wildcard import path.
pub fn import_path_wildcard(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Wildcard)
}

/// Creates a multi-import path.
pub fn import_path_multi(
    path: &str,
    items: Vec<(Expression, Option<Box<Expression>>)>,
) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Multi(items))
}

/// Creates a use statement.
pub fn use_statement(import_path: Expression, alias: Option<Box<Expression>>) -> Statement {
    stmt(StatementKind::Use(Box::new(import_path), alias))
}
