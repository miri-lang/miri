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

/// The kind of range expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RangeExpressionType {
    /// Exclusive range: `1..10`
    Exclusive,
    /// Inclusive range: `1..=10`
    Inclusive,
    /// An iterable object (e.g. a string or collection used in a for loop).
    IterableObject,
}

/// The kind of import path in a `use` statement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImportPathKind {
    /// Single item import: `use foo.bar`
    Simple,
    /// Wildcard import: `use foo.*`
    Wildcard,
    /// Multi-item import: `use foo.{bar, baz as b}`
    Multi(Vec<(Expression, Option<Box<Expression>>)>),
}

/// Represents a left-hand side expression, which can be an identifier or a more complex expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LeftHandSideExpression {
    Identifier(Box<Expression>),
    Member(Box<Expression>),
    Index(Box<Expression>),
}

impl LeftHandSideExpression {
    pub fn span(&self) -> Span {
        match self {
            LeftHandSideExpression::Identifier(e) => e.span,
            LeftHandSideExpression::Member(e) => e.span,
            LeftHandSideExpression::Index(e) => e.span,
        }
    }
}

/// Represents an expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExpressionKind {
    /// A literal value (e.g., number, string, boolean).
    Literal(Literal),

    /// An identifier (e.g., variable name) with an optional class qualifier.
    Identifier(String, Option<String>),

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
    ),

    /// A range expression (e.g., `1..10`).
    Range(
        Box<Expression>,
        Option<Box<Expression>>,
        RangeExpressionType,
    ),

    /// A guard expression.
    Guard(GuardOp, Box<Expression>),

    /// A member access expression (e.g., `object.property`).
    Member(Box<Expression>, Box<Expression>),

    /// An index access expression (e.g., `object[index]`).
    Index(Box<Expression>, Box<Expression>),

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
    ),

    /// A type declaration expression (e.g., `T extends SomeClass`).
    TypeDeclaration(
        Box<Expression>,
        Option<Vec<Expression>>,
        TypeDeclarationKind,
        Option<Box<Expression>>,
    ),

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
    ),

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
    Match(Box<Expression>, Vec<MatchBranch>),

    /// An interpolated string (f-string e.g. `"hello #{name}"`).
    FormattedString(Vec<Expression>),

    /// A named argument in a function call (e.g., `foo(a: 1)`).
    NamedArgument(String, Box<Expression>),

    /// A super reference for calling parent class methods (e.g., `super.init()`).
    Super,

    /// A block expression: a sequence of statements followed by a final expression.
    /// Used for multi-statement blocks in if-expressions, e.g.:
    /// let result = if true
    ///     let y = 20
    ///     x + y
    Block(Vec<Statement>, Box<Expression>),
}

/// An expression node (wraps `ExpressionKind` with an ID and span).
pub type Expression = IdNode<ExpressionKind>;

/// Wraps an expression in `Some(Box<...>)` for use in optional fields.
pub fn opt_expr(expr: Expression) -> Option<Box<Expression>> {
    Some(Box::new(expr))
}

impl fmt::Display for ExpressionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpressionKind::Type(t, _) => write!(f, "{}", t),
            ExpressionKind::Identifier(name, _) => write!(f, "{}", name),
            ExpressionKind::GenericType(name, _, _) => write!(f, "{}", name.node),
            _ => write!(f, "{:?}", self),
        }
    }
}
