// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::common::{FunctionProperties, Parameter};
use crate::ast::literal::Literal;
use crate::ast::node::IdNode;
use crate::ast::operator::{AssignmentOp, BinaryOp, GuardOp, UnaryOp};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{IfStatementType, Statement};
use crate::ast::types::{Type, TypeDeclarationKind};
use crate::error::syntax::Span;
use std::fmt;

/// Represents the type of a range expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RangeExpressionType {
    Exclusive, // Represents a range like `1..10`
    Inclusive, // Represents a range like `1..=10`
    // TODO: Step,      // Represents a range with a step, e.g., `1..10:2`
    IterableObject, // Represents an iterable object, e.g. a string, or a collection
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImportPathKind {
    Simple,
    Wildcard,
    Multi(Vec<(Expression, Option<Box<Expression>>)>),
}

/// Represents a left-hand side expression, which can be an identifier or a more complex expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LeftHandSideExpression {
    Identifier(Box<Expression>),

    Member(Box<Expression>), // object.property

    Index(Box<Expression>), // object[index]
}

impl LeftHandSideExpression {
    pub fn span(&self) -> Span {
        match self {
            LeftHandSideExpression::Identifier(e) => e.span.clone(),
            LeftHandSideExpression::Member(e) => e.span.clone(),
            LeftHandSideExpression::Index(e) => e.span.clone(),
        }
    }
}

/// Represents an expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExpressionKind {
    Literal(Literal),

    Identifier(String, Option<String>), // name, optional class e.g. x or Http::Status

    Binary(Box<Expression>, BinaryOp, Box<Expression>),

    Logical(Box<Expression>, BinaryOp, Box<Expression>),

    Unary(UnaryOp, Box<Expression>),

    Assignment(Box<LeftHandSideExpression>, AssignmentOp, Box<Expression>),

    Conditional(
        Box<Expression>,
        Box<Expression>,
        Option<Box<Expression>>,
        IfStatementType,
    ), // then_expr, condition, else_expr

    Range(
        Box<Expression>,
        Option<Box<Expression>>,
        RangeExpressionType,
    ), // start, end, range_type

    Guard(GuardOp, Box<Expression>), // guard operator and expression

    Member(Box<Expression>, Box<Expression>), // object.property

    Index(Box<Expression>, Box<Expression>), // object[index]

    Call(Box<Expression>, Vec<Expression>), // function, args

    ImportPath(Vec<Expression>, ImportPathKind), // Represents an import path, e.g., `use a.b.c`

    Type(Box<Type>, bool), // Represents a type expression, e.g., `i32`, `string`, etc.

    GenericType(
        Box<Expression>,
        Option<Box<Expression>>,
        TypeDeclarationKind,
    ), // Represents a generic type, e.g., <T is MyClass>

    TypeDeclaration(
        Box<Expression>,
        Option<Vec<Expression>>,
        TypeDeclarationKind,
        Option<Box<Expression>>,
    ), // T extends SomeClass

    EnumValue(Box<Expression>, Vec<Expression>), // Represents an enum value, e.g., Ok, Err(string)

    StructMember(Box<Expression>, Box<Expression>), // Represents a struct member, e.g., `x int`

    Lambda(
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
        Box<Statement>,
        FunctionProperties,
    ), // generic_types, parameters, return type, body

    List(Vec<Expression>), // A list literal, e.g., [1, 2, 3]

    Map(Vec<(Expression, Expression)>), // A map literal, e.g., {'a': 1, 'b': 2}

    Tuple(Vec<Expression>), // A tuple literal, e.g., (1, 'a', true)

    Set(Vec<Expression>), // A set literal, e.g., {1, 2, 3}

    Match(Box<Expression>, Vec<MatchBranch>), // value, branches

    FormattedString(Vec<Expression>), // "hello #{name}"

    NamedArgument(String, Box<Expression>), // name, value
}

pub type Expression = IdNode<ExpressionKind>;

pub fn opt_expr(expr: Expression) -> Option<Box<Expression>> {
    Some(Box::new(expr))
}

impl fmt::Display for ExpressionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpressionKind::Type(t, _) => write!(f, "{}", t),
            ExpressionKind::Identifier(name, _) => write!(f, "{}", name),
            ExpressionKind::GenericType(name, _, _) => write!(f, "{}", name.node),
            // Fallback for other expressions if they appear in types (shouldn't happen often in error messages for types)
            _ => write!(f, "{:?}", self),
        }
    }
}
