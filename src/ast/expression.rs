// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

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
    /// A literal value (e.g., number, string, boolean).
    Literal(Literal),

    /// An identifier (e.g., variable name) with an optional class qualifier.
    Identifier(String, Option<String>), // name, optional class e.g. x or Http::Status

    /// A binary operation (e.g., `a + b`).
    Binary(Box<Expression>, BinaryOp, Box<Expression>),

    /// A logical operation (e.g., `a && b`).
    Logical(Box<Expression>, BinaryOp, Box<Expression>),

    /// A unary operation (e.g., `-a`, `!a`).
    Unary(UnaryOp, Box<Expression>),

    /// An assignment operation (e.g., `a = b`).
    Assignment(Box<LeftHandSideExpression>, AssignmentOp, Box<Expression>),

    /// A conditional expression (e.g., `a ? b : c` or `if a then b else c`).
    Conditional(
        Box<Expression>,
        Box<Expression>,
        Option<Box<Expression>>,
        IfStatementType,
    ), // then_expr, condition, else_expr

    /// A range expression (e.g., `1..10`).
    Range(
        Box<Expression>,
        Option<Box<Expression>>,
        RangeExpressionType,
    ), // start, end, range_type

    /// A guard expression.
    Guard(GuardOp, Box<Expression>), // guard operator and expression

    /// A member access expression (e.g., `object.property`).
    Member(Box<Expression>, Box<Expression>), // object.property

    /// An index access expression (e.g., `object[index]`).
    Index(Box<Expression>, Box<Expression>), // object[index]

    /// A function call (e.g., `foo(a, b)`).
    Call(Box<Expression>, Vec<Expression>),

    /// An import path (e.g., `use a.b.c`).
    ImportPath(Vec<Expression>, ImportPathKind),

    /// A type expression wrapper (e.g., when a type is used as an expression).
    Type(Box<Type>, bool),

    /// A generic type reference (e.g., `List<T>`).
    GenericType(
        Box<Expression>,
        Option<Box<Expression>>,
        TypeDeclarationKind,
    ), // Represents a generic type, e.g., <T is MyClass>

    /// A type declaration expression.
    TypeDeclaration(
        Box<Expression>,
        Option<Vec<Expression>>,
        TypeDeclarationKind,
        Option<Box<Expression>>,
    ), // T extends SomeClass

    /// An enum value reference (e.g., `Option::Some(5)`).
    EnumValue(Box<Expression>, Vec<Expression>),

    /// A struct member declaration (e.g., in a struct definition `x int`).
    StructMember(Box<Expression>, Box<Expression>),

    /// A lambda function (e.g., `fn (x int) int: x + 1`).
    Lambda(
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
        Box<Statement>,
        FunctionProperties,
    ), // generic_types, parameters, return type, body

    /// A list literal (e.g., `[1, 2, 3]`).
    List(Vec<Expression>),

    /// An array literal (fixed size).
    Array(Vec<Expression>, Box<Expression>),

    /// A map literal (e.g., `{'a': 1, 'b': 2}`).
    Map(Vec<(Expression, Expression)>),

    /// A tuple literal (e.g., `(1, "a")`).
    Tuple(Vec<Expression>),

    /// A set literal (e.g., `{1, 2, 3}`).
    Set(Vec<Expression>),

    /// A match expression.
    Match(Box<Expression>, Vec<MatchBranch>), // value, branches

    /// An interpolated string (f-string e.g. `"hello #{name}"`).
    FormattedString(Vec<Expression>),

    /// A named argument in a function call (e.g., `foo(a: 1)`).
    NamedArgument(String, Box<Expression>),

    /// A super reference for calling parent class methods (e.g., `super.init()`).
    Super,
}

/// Represents an expression
pub type Expression = IdNode<ExpressionKind>;

/// Returns an optional expression
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
