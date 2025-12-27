// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

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
    pub typ: Option<Box<Expression>>, // Type can be specified, e.g., "i32", "String"
    pub initializer: Option<Box<Expression>>, // Optional initializer expression
    pub declaration_type: VariableDeclarationType, // Whether the variable is mutable
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub enum StatementKind {
    Empty, // Represents an empty statement, e.g., when a block is empty

    Break,

    Continue,

    Expression(Expression),

    Block(Vec<Statement>),

    Variable(Vec<VariableDeclaration>, MemberVisibility),

    If(
        Box<Expression>,
        Box<Statement>,
        Option<Box<Statement>>,
        IfStatementType,
    ), // condition, then_block, else_block, type

    While(Box<Expression>, Box<Statement>, WhileStatementType), // condition, then_block, type

    For(Vec<VariableDeclaration>, Box<Expression>, Box<Statement>), // variable_declarations, iterable, body

    FunctionDeclaration(
        String,
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
        Box<Statement>,
        FunctionProperties,
    ), // name, generic_types, parameters, return type, body

    Return(Option<Box<Expression>>), // Optional return expression

    Use(Box<Expression>, Option<Box<Expression>>),

    Type(Vec<Expression>, MemberVisibility), // type X, Y, Z extends A

    Enum(Box<Expression>, Vec<Expression>, MemberVisibility), // enum Colors: Red, Green, Blue(string)

    Struct(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        MemberVisibility,
    ), // struct Point<T>: x T, y int

    Extends(Box<Expression>), // extends BaseClass

    Implements(Vec<Expression>), // implements Trait1, Trait2

    Includes(Vec<Expression>), // includes Module1, Module2
}

pub type Statement = IdNode<StatementKind>;
