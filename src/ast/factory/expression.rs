// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::expr;
use super::expr_with_span;
use crate::ast::expression::{Expression, ExpressionKind, ImportPathKind, RangeExpressionType};
use crate::ast::operator::{AssignmentOp, BinaryOp, GuardOp, UnaryOp};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::IfStatementType;
use crate::ast::types::{Type, TypeDeclarationKind};
use crate::error::syntax::Span;

/// Creates a binary expression with a specific span.
pub fn binary_with_span(
    left: Expression,
    op: BinaryOp,
    right: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Binary(Box::new(left), op, Box::new(right)),
        span,
    )
}

/// Creates a unary expression with a specific span.
pub fn unary_with_span(op: UnaryOp, expr_node: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Unary(op, Box::new(expr_node)), span)
}

/// Creates a logical binary expression with a specific span.
pub fn logical_with_span(
    left: Expression,
    op: BinaryOp,
    right: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Logical(Box::new(left), op, Box::new(right)),
        span,
    )
}

/// Creates a function call expression with a specific span.
pub fn call_with_span(callee: Expression, args: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Call(Box::new(callee), args), span)
}

/// Creates a member access expression with a specific span.
pub fn member_with_span(object: Expression, property: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Member(Box::new(object), Box::new(property)),
        span,
    )
}

/// Creates an index access expression with a specific span.
pub fn index_with_span(object: Expression, index: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Index(Box::new(object), Box::new(index)),
        span,
    )
}

/// Creates an assignment expression with a specific span.
pub fn assign_with_span(
    left: crate::ast::expression::LeftHandSideExpression,
    op: AssignmentOp,
    right: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Assignment(Box::new(left), op, Box::new(right)),
        span,
    )
}

/// Creates a cast expression with a specific span.
pub fn cast_with_span(value: Expression, target_type: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Cast(Box::new(value), Box::new(target_type)),
        span,
    )
}

/// Creates a list literal expression with a specific span.
pub fn list_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::List(elements), span)
}

/// Creates a map literal expression with a specific span.
pub fn map_with_span(pairs: Vec<(Expression, Expression)>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Map(pairs), span)
}

/// Creates a tuple literal expression with a specific span.
pub fn tuple_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Tuple(elements), span)
}

/// Creates a set literal expression with a specific span.
pub fn set_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Set(elements), span)
}

/// Creates a match expression with a specific span.
pub fn match_expression_with_span(
    subject: Expression,
    branches: Vec<MatchBranch>,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::Match(Box::new(subject), branches), span)
}

/// Creates a formatted string expression with a specific span.
pub fn f_string_with_span(parts: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::FormattedString(parts), span)
}

/// Creates a type expression with a specific span.
pub fn type_expression_with_span(inner: Type, is_nullable: bool, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Type(Box::new(inner), is_nullable), span)
}

/// Creates a generic type expression with a specific span.
pub fn generic_type_expression_with_span(
    name_expression: Expression,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::GenericType(Box::new(name_expression), constraint, kind),
        span,
    )
}

/// Creates a generic type expression with a default span.
pub fn generic_type_expression(
    name_expression: Expression,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
) -> Expression {
    generic_type_expression_with_span(name_expression, constraint, kind, Span::new(0, 0))
}

/// Creates a conditional expression with a specific span.
pub fn conditional_with_span(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
    if_type: IfStatementType,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Conditional(
            Box::new(then),
            Box::new(cond),
            else_b.map(Box::new),
            if_type,
        ),
        span,
    )
}

/// Creates a range expression with a specific span.
pub fn range_with_span(
    start: Expression,
    end: Option<Box<Expression>>,
    range_type: RangeExpressionType,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Range(Box::new(start), end, range_type),
        span,
    )
}

/// Creates a guard expression with a specific span.
pub fn guard_with_span(op: GuardOp, expr_node: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Guard(op, Box::new(expr_node)), span)
}

/// Creates an import path expression with a specific span.
pub fn import_path_expression_with_span(
    segments: Vec<Expression>,
    kind: ImportPathKind,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::ImportPath(segments, kind), span)
}

/// Creates a type declaration expression with a specific span.
pub fn type_declaration_expression_with_span(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::TypeDeclaration(Box::new(name), generic_types, kind, type_expr),
        span,
    )
}

/// Creates an enum value expression with a specific span.
pub fn enum_value_expression_with_span(
    name: Expression,
    types: Vec<Expression>,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::EnumValue(Box::new(name), types), span)
}

/// Creates a struct member expression with a specific span.
pub fn struct_member_expression_with_span(
    name: Expression,
    typ: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::StructMember(Box::new(name), Box::new(typ)),
        span,
    )
}

/// Creates a binary expression.
pub fn binary(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    expr(ExpressionKind::Binary(Box::new(left), op, Box::new(right)))
}

/// Creates a unary expression.
pub fn unary(op: UnaryOp, expr_node: Expression) -> Expression {
    expr(ExpressionKind::Unary(op, Box::new(expr_node)))
}

/// Creates a logical binary expression.
pub fn logical(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    expr(ExpressionKind::Logical(Box::new(left), op, Box::new(right)))
}

/// Creates an assignment expression.
pub fn assign(
    left: crate::ast::expression::LeftHandSideExpression,
    op: AssignmentOp,
    right: Expression,
) -> Expression {
    expr(ExpressionKind::Assignment(
        Box::new(left),
        op,
        Box::new(right),
    ))
}

/// Creates a conditional expression (ternary or if-else expr).
pub fn conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
    if_type: IfStatementType,
) -> Expression {
    expr(ExpressionKind::Conditional(
        Box::new(then),
        Box::new(cond),
        else_b.map(Box::new),
        if_type,
    ))
}

/// Creates an `if` expression.
pub fn if_conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
) -> Expression {
    conditional(then, cond, else_b, IfStatementType::If)
}

/// Creates an `unless` expression.
pub fn unless_conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
) -> Expression {
    conditional(then, cond, else_b, IfStatementType::Unless)
}

/// Creates a range expression.
pub fn range(
    start: Expression,
    end: Option<Box<Expression>>,
    range_type: RangeExpressionType,
) -> Expression {
    expr(ExpressionKind::Range(Box::new(start), end, range_type))
}

/// Creates an iterable object expression (from a range).
pub fn iter_obj(start: Expression) -> Expression {
    expr(ExpressionKind::Range(
        Box::new(start),
        None,
        RangeExpressionType::IterableObject,
    ))
}

/// Creates a member access expression.
pub fn member(object: Expression, property: Expression) -> Expression {
    expr(ExpressionKind::Member(Box::new(object), Box::new(property)))
}

/// Creates an index access expression.
pub fn index(object: Expression, index: Expression) -> Expression {
    expr(ExpressionKind::Index(Box::new(object), Box::new(index)))
}

/// Creates a function call expression.
pub fn call(callee: Expression, args: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Call(Box::new(callee), args))
}

/// Creates a guard expression.
pub fn guard(op: GuardOp, expr_node: Expression) -> Expression {
    expr(ExpressionKind::Guard(op, Box::new(expr_node)))
}

/// Creates a list literal expression.
pub fn list(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::List(elements))
}

/// Creates a fixed-size array literal expression.
pub fn array(elements: Vec<Expression>, size: Box<Expression>) -> Expression {
    expr(ExpressionKind::Array(elements, size))
}

/// Creates a map literal expression.
pub fn map(pairs: Vec<(Expression, Expression)>) -> Expression {
    expr(ExpressionKind::Map(pairs))
}

/// Creates a tuple literal expression.
pub fn tuple(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Tuple(elements))
}

/// Creates a set literal expression.
pub fn set(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Set(elements))
}

/// Creates a match expression.
pub fn match_expression(subject: Expression, branches: Vec<MatchBranch>) -> Expression {
    expr(ExpressionKind::Match(Box::new(subject), branches))
}

/// Creates a named argument expression.
pub fn named_argument(name: String, value: Expression) -> Expression {
    expr(ExpressionKind::NamedArgument(name, Box::new(value)))
}

/// Creates a named argument expression with a span.
pub fn named_argument_with_span(name: String, value: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::NamedArgument(name, Box::new(value)), span)
}
