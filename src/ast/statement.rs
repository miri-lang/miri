// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::attributes::Attribute;
use crate::ast::common::{FunctionProperties, MemberVisibility, Parameter, RuntimeKind};
use crate::ast::expression::Expression;
use crate::error::syntax::Span;
use crate::lexer::BufferedComment;
use std::hash::{Hash, Hasher};

/// Data for a function declaration, boxed to reduce `StatementKind` enum size.
#[derive(Debug, Clone, Eq)]
pub struct FunctionDeclarationData {
    pub name: String,
    /// Source range of the declared name, so a diagnostic about the function
    /// itself can point at the name rather than at a neighbouring parameter or
    /// return type. Empty for declarations built programmatically, which have
    /// no source text to point at.
    pub name_span: Span,
    pub generics: Option<Vec<Expression>>,
    pub params: Vec<Parameter>,
    pub return_type: Option<Box<Expression>>,
    /// Body is None for abstract functions in traits/abstract classes.
    pub body: Option<Box<Statement>>,
    pub properties: FunctionProperties,
    pub attributes: Vec<Attribute>,
}

/// Equality and hashing ignore `name_span`, matching [`IdNode`], where a node's
/// source location is metadata rather than part of its identity. This keeps a
/// declaration built by the AST factory (which has no source text, so no span)
/// equal to the same declaration produced by the parser.
impl PartialEq for FunctionDeclarationData {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.generics == other.generics
            && self.params == other.params
            && self.return_type == other.return_type
            && self.body == other.body
            && self.properties == other.properties
            && self.attributes == other.attributes
    }
}

impl Hash for FunctionDeclarationData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.generics.hash(state);
        self.params.hash(state);
        self.return_type.hash(state);
        self.body.hash(state);
        self.properties.hash(state);
        self.attributes.hash(state);
    }
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
    pub attributes: Vec<Attribute>,
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
    /// A class field declared with no mutability keyword, as in `total int`.
    ///
    /// It binds exactly as `Mutable` does — the field is writable — and the
    /// variant exists only so the canonical rendering can write the field back
    /// the way it was written instead of inventing a `var` the author never
    /// typed. Every reader that asks whether a binding is mutable must answer
    /// the same for this as for [`VariableDeclarationType::Mutable`].
    Unmarked,
}

/// Trivia (comments and whitespace metadata) attached to a statement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StatementTrivia {
    /// Comments that precede the statement.
    pub leading_comments: Vec<BufferedComment>,
    /// An optional comment on the same line as the statement.
    pub trailing_comment: Option<BufferedComment>,
    /// Comments written on their own lines after the statement, which no
    /// later statement claimed because none follows it in its block.
    pub trailing_lines: Vec<BufferedComment>,
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

/// The target device for a forall construct.
/// Determines which backend executes the parallel loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcceleratorTarget {
    /// Device inferred from data residency in the loop body.
    Inferred,
    /// Explicit GPU target (guard against CPU-resident data).
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

    /// A parallel `forall` loop: `forall <vars> in <range>` or `gpu forall <vars> in <range>`.
    ///
    /// Lowered to a synthesized anonymous kernel plus the backend-appropriate
    /// terminator (GPU: `GpuLaunch`; CPU: nested sequential loops).
    /// The device field determines whether inference or explicit GPU is required.
    Forall {
        device: AcceleratorTarget,
        vars: Vec<VariableDeclaration>,
        iterable: Box<Expression>,
        body: Box<Statement>,
    },

    /// A GPU frame-step loop: `gpu frame <ident> in <range>`.
    ///
    /// Reads from one gpu buffer and writes to another, implementing a
    /// ping-pong pattern for animations/simulations. Lowered to a synthesized
    /// kernel marked with `is_frame_step=true`.
    GpuFrame(Vec<VariableDeclaration>, Box<Expression>, Box<Statement>),

    /// A GPU frame block: `gpu frame { forall ..., forall ... }`.
    ///
    /// Multiple ordered passes where each pass is a `forall` loop. Each pass
    /// is a separate kernel dispatched sequentially on the GPU. Enables
    /// ping-pong buffering and multi-stage animations.
    GpuFrameBlock(Box<Statement>),

    /// A function declaration. Boxed to reduce enum size.
    FunctionDeclaration(Box<FunctionDeclarationData>),

    /// A return statement.
    Return(Option<Box<Expression>>),

    /// A use statement (import).
    Use(Box<Expression>, Option<Box<Expression>>),

    /// A type alias declaration.
    Type(Vec<Expression>, MemberVisibility),

    /// An enum declaration.
    /// (name, generics, variants, methods, visibility, attributes)
    Enum(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        Vec<Statement>,
        MemberVisibility,
        Vec<Attribute>,
    ),

    /// A struct declaration.
    /// (name, generics, fields, methods, visibility, traits)
    ///
    /// `traits` holds the `implements Trait, ...` list. A struct is a data type,
    /// so its traits are capability markers (e.g. `Accelerable`) rather than a
    /// dispatch base; the list mirrors the class trait list.
    Struct(
        Box<Expression>,
        Option<Vec<Expression>>,
        Vec<Expression>,
        Vec<Statement>,
        MemberVisibility,
        Vec<Expression>,
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

/// Represents a statement with trivia (leading and trailing comments).
#[derive(Debug, Clone, Eq)]
pub struct Statement {
    pub id: usize,
    pub node: StatementKind,
    pub span: Span,
    pub trivia: StatementTrivia,
}

impl PartialEq for Statement {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl Hash for Statement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node.hash(state);
    }
}

impl Default for Statement {
    fn default() -> Self {
        Statement {
            id: 0,
            node: StatementKind::Empty,
            span: Span::new(0, 0),
            trivia: Default::default(),
        }
    }
}
