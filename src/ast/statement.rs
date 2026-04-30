// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::common::{FunctionProperties, MemberVisibility, Parameter, RuntimeKind};
use crate::ast::expression::Expression;
use crate::ast::node::IdNode;

/// Data for a function declaration, boxed to reduce `StatementKind` enum size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionDeclarationData {
    pub name: String,
    pub generics: Option<Vec<Expression>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<Box<Expression>>,
    /// Body is None for abstract functions in traits/abstract classes.
    pub body: Option<Box<Statement>>,
    pub properties: FunctionProperties,
}

/// Data for a class declaration, boxed to reduce `StatementKind` enum size.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassData {
    pub name: Box<Expression>,
    pub generics: Option<Vec<Expression>>,
    pub base_class: Option<Box<Expression>>,
    pub traits: Vec<Expression>,
    pub body: Vec<Statement>,
    pub visibility: MemberVisibility,
    pub is_abstract: bool,
}

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
    Constant,
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

    /// A function declaration. Boxed to reduce enum size.
    FunctionDeclaration(Box<FunctionDeclarationData>),

    /// A return statement.
    Return(Option<Box<Expression>>),

    /// A use statement (import).
    Use(Box<Expression>, Option<Box<Expression>>),

    /// A type alias declaration.
    Type(Vec<Expression>, MemberVisibility),

    /// An enum declaration.
    Enum(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        MemberVisibility,
    ),

    /// A struct declaration.
    /// (name, generics, fields, methods, visibility)
    Struct(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        Vec<Statement>,
        MemberVisibility,
    ),

    /// A class declaration. Boxed to reduce enum size.
    Class(Box<ClassData>),

    /// A trait declaration.
    /// (name, generics, parent_traits, body, visibility)
    Trait(
        Box<Expression>,         // Trait name
        Option<Vec<Expression>>, // Generic type parameters
        Vec<Expression>,         // Parent traits (multiple, via extends)
        Vec<Statement>,          // Trait body (method signatures)
        MemberVisibility,        // Trait visibility
    ),

    /// A runtime function declaration (extern binding to a runtime library).
    /// These functions have no body, no generics, no modifiers, and are always
    /// private to their declaring scope.
    /// (runtime_kind, name, params, return_type)
    RuntimeFunctionDeclaration(
        RuntimeKind,             // Which runtime this function lives in
        String,                  // Function name (e.g., "miri_rt_string_new")
        Vec<Parameter>,          // Parameters
        Option<Box<Expression>>, // Return type
    ),
}

/// Represents a statement
pub type Statement = IdNode<StatementKind>;
