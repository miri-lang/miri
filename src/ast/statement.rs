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

/// Where a binding's value physically lives. Residency is a binding
/// attribute orthogonal to the value's type — the same `Array<int, 3>`
/// can be either host- or gpu-resident. The `gpu` keyword on a `let` /
/// `var` is the only source of `Gpu`; absence of the keyword means
/// `Host`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BindingResidency {
    /// Standard host-side binding (`let x = ...`, `var x = ...`).
    #[default]
    Host,
    /// Device-resident binding (`gpu let x = ...`, `gpu var x = ...`).
    Gpu,
}

/// Represents a variable declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableDeclaration {
    pub name: String,
    pub typ: Option<Box<Expression>>,
    pub initializer: Option<Box<Expression>>,
    pub declaration_type: VariableDeclarationType,
    pub is_shared: bool,
    pub residency: BindingResidency,
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

    /// A GPU parallel `for` loop: `gpu for <ident> in <range>`.
    ///
    /// Lowered to a synthesized anonymous `gpu fn` plus a `GpuLaunch`
    /// terminator whose grid is derived from the range length.
    GpuFor(Vec<VariableDeclaration>, Box<Expression>, Box<Statement>),

    /// A GPU frame-step loop: `gpu frame <ident> in <range>`.
    ///
    /// Reads from one gpu buffer and writes to another, implementing a
    /// ping-pong pattern for animations/simulations. Lowered to a synthesized
    /// kernel marked with `is_frame_step=true`.
    GpuFrame(Vec<VariableDeclaration>, Box<Expression>, Box<Statement>),

    /// A function declaration. Boxed to reduce enum size.
    FunctionDeclaration(Box<FunctionDeclarationData>),

    /// A return statement.
    Return(Option<Box<Expression>>),

    /// A use statement (import).
    Use(Box<Expression>, Option<Box<Expression>>),

    /// A type alias declaration.
    Type(Vec<Expression>, MemberVisibility),

    /// An enum declaration.
    /// (name, generics, variants, methods, visibility, must_use)
    Enum(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        Vec<Statement>,
        MemberVisibility,
        bool,
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

    /// An intrinsic function declaration (compiler-implemented function).
    /// These functions have no body and are handled specially by the compiler.
    /// (name, generics, params, return_type, visibility)
    IntrinsicFunctionDeclaration(
        String,                  // Function name
        Option<Vec<Expression>>, // Generics
        Vec<Parameter>,          // Parameters
        Option<Box<Expression>>, // Return type
        MemberVisibility,        // Visibility
    ),
}

/// Represents a statement
pub type Statement = IdNode<StatementKind>;
