// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::common::{FunctionProperties, MemberVisibility, Parameter};
use crate::ast::expression::Expression;
use crate::ast::node::IdNode;

/// Represents the type of an if statement
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IfStatementType {
    If,
    Unless,
}

/// Represents the type of a while statement
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WhileStatementType {
    While,
    Until,
    DoWhile,
    DoUntil,
    Forever, // Endless loop
}

/// Represents the type of a variable
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VariableDeclarationType {
    Mutable,
    Immutable,
}

/// Represents a variable declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableDeclaration {
    pub name: String,
    pub typ: Option<Box<Expression>>,
    pub initializer: Option<Box<Expression>>,
    pub declaration_type: VariableDeclarationType,
    pub is_shared: bool,
}

/// Represents a statement kind
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub enum StatementKind {
    /// An empty statement (does nothing).
    Empty,

    /// A break statement (for loops).
    Break,

    /// A continue statement (for loops).
    Continue,

    /// A statement consisting of a single expression.
    Expression(Expression),

    /// A block of statements.
    Block(Vec<Statement>),

    /// A variable declaration.
    Variable(Vec<VariableDeclaration>, MemberVisibility),

    /// An if statement (or unless).
    If(
        Box<Expression>,
        Box<Statement>,
        Option<Box<Statement>>,
        IfStatementType,
    ),

    /// A while/until/do-while loop.
    While(Box<Expression>, Box<Statement>, WhileStatementType),

    /// A for loop.
    For(Vec<VariableDeclaration>, Box<Expression>, Box<Statement>),

    /// A function declaration.
    FunctionDeclaration(
        String,
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
        Box<Statement>,
        FunctionProperties,
    ),

    /// A return statement.
    Return(Option<Box<Expression>>),

    /// A use statement (import).
    Use(Box<Expression>, Option<Box<Expression>>),

    /// A type alias declaration.
    Type(Vec<Expression>, MemberVisibility),

    /// An enum declaration.
    Enum(Box<Expression>, Vec<Expression>, MemberVisibility),

    /// A struct declaration.
    Struct(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        MemberVisibility,
    ),

    /// An extends clause (for inheritance).
    Extends(Box<Expression>),

    /// An implements clause (for interfaces).
    Implements(Vec<Expression>),

    /// An includes clause (for mixins/traits).
    Includes(Vec<Expression>),
}

/// Represents a statement
pub type Statement = IdNode<StatementKind>;
