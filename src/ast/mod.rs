// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::error::syntax::Span;
use crate::lexer::RegexToken;

pub mod factory;

/// Represents a fully parsed Miri program
#[derive(Debug, PartialEq)]
pub struct Program {
    pub body: Vec<Statement>,
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

/// Represents the type of a range expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RangeExpressionType {
    Exclusive, // Represents a range like `1..10`
    Inclusive, // Represents a range like `1..=10`
    // TODO: Step,      // Represents a range with a step, e.g., `1..10:2`
    IterableObject, // Represents an iterable object, e.g. a string, or a collection
}

/// Represents a parameter in a function declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Parameter {
    pub name: String,
    pub typ: Box<Expression>,
    pub guard: Option<Box<Expression>>, // Optional guard expression
    pub default_value: Option<Box<Expression>>, // Optional default value
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MemberVisibility {
    Public,
    Protected,
    Private,
}

/// Represents the properties of a function declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionProperties {
    pub is_async: bool,
    pub is_gpu: bool,
    pub visibility: MemberVisibility,
}

use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Eq)]
pub struct IdNode<T> {
    pub id: usize,
    pub node: T,
    pub span: Span,
}

impl<T: PartialEq> PartialEq for IdNode<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl<T: Hash> Hash for IdNode<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node.hash(state);
    }
}

impl<T> IdNode<T> {
    pub fn new(id: usize, node: T, span: Span) -> Self {
        Self { id, node, span }
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImportPathKind {
    Simple,
    Wildcard,
    Multi(Vec<(Expression, Option<Box<Expression>>)>),
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

/// Represents a binary operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitwiseOr,
    BitwiseAnd,
    BitwiseXor,
    Equal,
    NotEqual,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Not,
    And,
    Or,
    Range, // Represents a range operator (e.g., `1..10`)
    In,    // Represents the `in` operator for membership tests
}

/// Represents a guard operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum GuardOp {
    NotEqual,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
    Not,
    NotIn,
    In,
}

/// Represents a unary operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum UnaryOp {
    Negate, // - operator
    Not,
    Plus,       // + operator (unary plus)
    BitwiseNot, // ~ operator
    Decrement,  // -- operator
    Increment,  // ++ operator
    Await,
}

/// Represents an assignment operator
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum AssignmentOp {
    Assign,
    AssignAdd,
    AssignSub,
    AssignMul,
    AssignDiv,
    AssignMod,
}

/// Represents a literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    String(String),
    Boolean(bool),
    Symbol(String),
    Regex(RegexToken),
    None,
}

/// Represents an integer literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IntegerLiteral {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    I128(i128),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
}

/// Represents a floating-point literal value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FloatLiteral {
    F32(u32), // Store as u32 to be hashable
    F64(u64),
}

/// Represents a type expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    Float,
    F32,
    F64,
    String,
    Boolean,
    Symbol,
    List(Box<Expression>),                    // [i32]
    Map(Box<Expression>, Box<Expression>),    // {string: i32}
    Tuple(Vec<Expression>),                   // (i32, String)
    Set(Box<Expression>),                     // {i32}
    Result(Box<Expression>, Box<Expression>), // result<i32, String>
    Future(Box<Expression>),                  // future<i32>
    Function(
        Option<Vec<Expression>>,
        Vec<Parameter>,
        Option<Box<Expression>>,
    ), // fn<T>(x int) float

    Generic(String, Option<Box<Type>>, TypeDeclarationKind), // T extends Number

    Custom(String, Option<Vec<Expression>>), // a custom type, e.g., MyStruct<T, U>
    Meta(Box<Type>), // Represents the type of a type itself, e.g. the type of the identifier `Point` is `Meta(Custom("Point"))`
    Nullable(Box<Type>), // Represents a nullable type, e.g., `int?`
    Void,            // Represents void type
    Error,           // Represents a type error
}

/// Represents a type declaration kind
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDeclarationKind {
    None,
    Is,
    Extends,
    Implements,
    Includes,
}

/// Represents a branch in a match expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchBranch {
    pub patterns: Vec<Pattern>,
    pub guard: Option<Box<Expression>>,
    pub body: Box<Statement>,
}

/// Represents a pattern in a match expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pattern {
    Literal(Literal),
    Identifier(String),
    Tuple(Vec<Pattern>),
    Regex(RegexToken),
    Default,
    Member(Box<Pattern>, String),
}

pub fn opt_expr(expr: Expression) -> Option<Box<Expression>> {
    Some(Box::new(expr))
}
