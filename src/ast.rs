// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

/// Represents a fully parsed Miri program
#[derive(Debug, PartialEq)]
pub struct Program {
    pub body: Vec<Statement>,
}

/// Represents the type of an if statement
#[derive(Debug, Clone, PartialEq)]
pub enum IfStatementType {
    If,
    Unless,
}

/// Represents the type of a while statement
#[derive(Debug, Clone, PartialEq)]
pub enum WhileStatementType {
    While,
    Until,
    Forever, // Endless loop
}

/// Represents the type of a range expression
#[derive(Debug, Clone, PartialEq)]
pub enum RangeExpressionType {
    Exclusive, // Represents a range like `1..10`
    Inclusive, // Represents a range like `1..=10`
    // TODO: Step,      // Represents a range with a step, e.g., `1..10:2`
    IterableObject, // Represents an iterable object, e.g. a string, or a collection
}

/// Represents a parameter in a function declaration
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub typ: Option<Box<Expression>>, // Type can be specified, e.g., "i32", "String"
    pub guard: Option<Box<Expression>>, // Optional guard expression
}

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Empty, // Represents an empty statement, e.g., when a block is empty

    Break,

    Continue,

    Expression(Expression),

    Block(Vec<Statement>),

    Variable(Vec<VariableDeclaration>),

    If(Box<Expression>, Box<Statement>, Option<Box<Statement>>, IfStatementType), // condition, then_block, else_block, type

    While(Box<Expression>, Box<Statement>, WhileStatementType), // condition, then_block, type

    For(Vec<VariableDeclaration>, Box<Expression>, Box<Statement>), // variable_declarations, iterable, body

    FunctionDeclaration(String, Option<Vec<Expression>>, Vec<Parameter>, Option<Box<Expression>>, Box<Statement>), // name, generic_types, parameters, return type, body

    Return(Option<Box<Expression>>), // Optional return expression

    Use(Box<Expression>, Option<Box<Expression>>),

    Type(Vec<Expression>) // type X, Y, Z extends A
}

/// Represents an expression
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Literal(Literal),
    
    Identifier(String),

    Binary(Box<Expression>, BinaryOp, Box<Expression>),

    Logical(Box<Expression>, BinaryOp, Box<Expression>),

    Unary(UnaryOp, Box<Expression>),

    Assignment(Box<LeftHandSideExpression>, AssignmentOp, Box<Expression>),

    Conditional(Box<Expression>, Box<Expression>, Option<Box<Expression>>, IfStatementType), // condition, then_expr, else_expr

    Range(Box<Expression>, Option<Box<Expression>>, RangeExpressionType), // start, end, range_type

    Guard(GuardOp, Box<Expression>), // guard operator and expression

    Member(Box<Expression>, Box<Expression>), // object.property

    Index(Box<Expression>, Box<Expression>), // object[index]

    Call(Box<Expression>, Vec<Expression>), // function, args

    ImportPath(Vec<Expression>), // Represents an import path, e.g., `use a.b.c`

    Type(Box<Type>, bool), // Represents a type expression, e.g., `i32`, `string`, etc.

    GenericType(Box<Expression>, Option<Box<Expression>>), // Represents a generic type, e.g., <T is MyClass>

    TypeDeclaration(Box<Expression>, TypeDeclarationKind, Option<Box<Expression>>), // T extends SomeClass

    // // Operators
    // Binary(Box<Expr>, BinaryOp, Box<Expr>),
    // Unary(UnaryOp, Box<Expr>),
    
    // // Collections
    // Array(Vec<Expr>),
    // Dict(Vec<(Expr, Expr)>),
    
    // // Control flow expressions
    // Lambda(Vec<Parameter>, Option<TypeExpr>, Box<Expr>),
    // Block(Block),
    // IfExpr(Box<Expr>, Box<Expr>, Option<Box<Expr>>), // condition, then_expr, else_expr
    
    // // Concurrency and parallelism
    // Async(Box<Expr>),
    // Await(Box<Expr>),
    // Spawn(Box<Expr>),
    // Parallel(Box<Expr>),
    // Send(Box<Expr>, String, Vec<Expr>), // actor, method, args
    
    // // Other
    // Symbol(String),
    // Try(Box<Expr>), // expr?
}

/// Represents the type of a variable
#[derive(Debug, Clone, PartialEq)]
pub enum VariableDeclarationType {
    Mutable,
    Immutable,
}

/// Represents a variable declaration
#[derive(Debug, Clone, PartialEq)]
pub struct VariableDeclaration {
    pub name: String,
    pub typ: Option<Box<Expression>>, // Type can be specified, e.g., "i32", "String"
    pub initializer: Option<Box<Expression>>, // Optional initializer expression
    pub declaration_type: VariableDeclarationType, // Whether the variable is mutable
}

/// Represents a left-hand side expression, which can be an identifier or a more complex expression
#[derive(Debug, Clone, PartialEq)]
pub enum LeftHandSideExpression {
    Identifier(Box<Expression>),

    Member(Box<Expression>), // object.property

    Index(Box<Expression>), // object[index]
}

/// Represents a binary operator
#[derive(Debug, PartialEq, Clone, Copy)]
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
    In,   // Represents the `in` operator for membership tests
}

/// Represents a guard operator
#[derive(Debug, PartialEq, Clone, Copy)]
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
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum UnaryOp {
    Negate, // - operator
    Not,
    Plus, // + operator (unary plus)
    BitwiseNot, // ~ operator
    Decrement, // -- operator
    Increment, // ++ operator
}

/// Represents an assignment operator
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum AssignmentOp {
    Assign,
    AssignAdd,
    AssignSub,
    AssignMul,
    AssignDiv,
    AssignMod,
}

/// Represents a literal value
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(IntegerLiteral),
    Float(FloatLiteral),
    String(String),
    Boolean(bool),
    Symbol(String),
}

/// Represents an integer literal value
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub enum FloatLiteral {
    F32(f32),
    F64(f64),
}

/// Represents a type expression
#[derive(Debug, Clone, PartialEq)]
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
    List(Box<Expression>),                      // [i32]
    Map(Box<Expression>, Box<Expression>),      // {string: i32}
    Tuple(Vec<Expression>),                     // (i32, String)
    Set(Box<Expression>),                       // {i32}
    Result(Box<Expression>, Box<Expression>),   // result<i32, String>
    Future(Box<Expression>),                    // future<i32>

    Custom(String, Option<Vec<Expression>>),    // a custom type, e.g., MyStruct<T, U>
}


/// Represents a type declaration kind
#[derive(Debug, Clone, PartialEq)]
pub enum TypeDeclarationKind {
    None,
    Is,
    Extends,
    Implements,
    Includes,
}

// /// Represents a statement in the Miri language
// #[derive(Debug, PartialEq)]
// pub enum Stmt {
//     // Variable declarations and assignments
//     VarDecl(String, Option<TypeExpr>, Option<Box<Expr>>, bool), // name, type, initializer, mutable
//     Assign(Box<Expr>, Box<Expr>), // target, value
    
//     // Function-related
//     FuncDecl(String, Vec<Parameter>, Option<TypeExpr>, Block), // name, parameters, return type, body
//     Return(Option<Box<Expr>>),
    
//     // Control flow
//     If(Box<Expr>, Block, Option<Block>), // condition, then_block, else_block
//     While(Box<Expr>, Block),
//     DoWhile(Block, Box<Expr>),
//     For(String, Box<Expr>, Block), // variable, iterable, body
//     Match(Box<Expr>, Vec<MatchArm>), // value, arms
    
//     // Module system
//     Use(Vec<String>, Option<String>, Option<Vec<String>>), // path, alias, selective imports
    
//     // Other statements
//     Block(Block),
//     Expr(Box<Expr>),
// }

// /// Represents a match arm in a match expression
// #[derive(Debug, PartialEq)]
// pub struct MatchArm {
//     pub pattern: Pattern,
//     pub guard: Option<Box<Expr>>,
//     pub body: Block,
// }

// /// Represents a pattern in a match expression
// #[derive(Debug, PartialEq)]
// pub enum Pattern {
//     Literal(Literal),
//     Identifier(String),
//     Multiple(Vec<Pattern>), // For patterns like "1 | 2 | 3"
//     Default,
// }


pub struct AstFactory;

impl AstFactory {
    pub fn new() -> Self {
        AstFactory {}
    }

    pub fn create_program(&self, statements: Vec<Statement>) -> Program {
        Program { body: statements }
    }

    pub fn create_expression_statement(&self, expression: Expression) -> Statement {
        Statement::Expression(expression)
    }

    pub fn create_block(&self, statements: Vec<Statement>) -> Statement {
        Statement::Block(statements)
    }

    pub fn create_literal_expression(&self, literal: Literal) -> Expression {
        Expression::Literal(literal)
    }

    pub fn create_i8_literal(&self, value: i8) -> Literal {
        Literal::Integer(IntegerLiteral::I8(value))
    }

    pub fn create_i16_literal(&self, value: i16) -> Literal {
        Literal::Integer(IntegerLiteral::I16(value))
    }

    pub fn create_i32_literal(&self, value: i32) -> Literal {
        Literal::Integer(IntegerLiteral::I32(value))
    }

    pub fn create_i64_literal(&self, value: i64) -> Literal {
        Literal::Integer(IntegerLiteral::I64(value))
    }

    pub fn create_i128_literal(&self, value: i128) -> Literal {
        Literal::Integer(IntegerLiteral::I128(value))
    }

    pub fn create_f32_literal(&self, value: f32) -> Literal {
        Literal::Float(FloatLiteral::F32(value))
    }

    pub fn create_f64_literal(&self, value: f64) -> Literal {
        Literal::Float(FloatLiteral::F64(value))
    }

    pub fn create_string_literal(&self, value: String) -> Literal {
        Literal::String(value)
    }

    pub fn create_boolean_literal(&self, value: bool) -> Literal {
        Literal::Boolean(value)
    }

    pub fn create_symbol_literal(&self, value: String) -> Literal {
        Literal::Symbol(value)
    }

    pub fn create_binary_expression(&self, left: Expression, op: BinaryOp, right: Expression) -> Expression {
        Expression::Binary(Box::new(left), op, Box::new(right))
    }

    pub fn create_logical_expression(&self, left: Expression, op: BinaryOp, right: Expression) -> Expression {
        Expression::Logical(Box::new(left), op, Box::new(right))
    }

    pub fn create_assignment_expression(&self, left: LeftHandSideExpression, op: AssignmentOp, right: Expression) -> Expression {
        Expression::Assignment(Box::new(left), op, Box::new(right))
    }

    pub fn create_identifier_expression(&self, name: String) -> Expression {
        Expression::Identifier(name)
    }

    pub fn create_range_expression(&self, start: Expression, end: Option<Box<Expression>>, range_type: RangeExpressionType) -> Expression {
        Expression::Range(Box::new(start), end, range_type)
    }

    pub fn create_variable_statement(&self, declarations: Vec<VariableDeclaration>) -> Statement {
        Statement::Variable(declarations)
    }

    pub fn create_if_statement(&self, condition: Expression, then_block: Statement, else_block: Option<Statement>, if_statement_type: IfStatementType) -> Statement {
        Statement::If(
            Box::new(condition),
            Box::new(then_block),
            else_block.map(Box::new),
            if_statement_type
        )
    }

    pub fn create_unary_expression(&self, op: UnaryOp, operand: Expression) -> Expression {
        Expression::Unary(op, Box::new(operand))
    }

    pub fn create_while_statement(&self, condition: Expression, body: Statement, while_statement_type: WhileStatementType) -> Statement {
        Statement::While(
            Box::new(condition),
            Box::new(body),
            while_statement_type
        )
    }

    pub fn create_conditional_expression(&self, then: Expression, condition: Expression, else_branch: Option<Expression>, if_statement_type: IfStatementType) -> Expression {
        Expression::Conditional(
            Box::new(then),
            Box::new(condition),
            else_branch.map(Box::new),
            if_statement_type
        )
    }

    pub fn create_for_statement(&self, variable_declarations: Vec<VariableDeclaration>, iterable: Expression, body: Statement) -> Statement {
        Statement::For(variable_declarations, Box::new(iterable), Box::new(body))
    }

    pub fn create_function_declaration(&self, name: String, generic_types: Option<Vec<Expression>>, parameters: Vec<Parameter>, return_type: Option<Box<Expression>>, body: Statement) -> Statement {
        Statement::FunctionDeclaration(name, generic_types, parameters, return_type, Box::new(body))
    }

    pub fn create_return_statement(&self, optional_expression: Option<Box<Expression>>) -> Statement {
        Statement::Return(optional_expression)
    }

    pub fn create_guard_expression(&self, op: GuardOp, expr: Expression) -> Expression {
        Expression::Guard(op, Box::new(expr))
    }

    pub fn create_member_expression(&self, object: Expression, property: Expression) -> Expression {
        Expression::Member(Box::new(object), Box::new(property))
    }

    pub fn create_index_expression(&self, object: Expression, index: Expression) -> Expression {
        Expression::Index(Box::new(object), Box::new(index))
    }

    pub fn create_left_hand_side_identifier(&self, identifier: Expression) -> LeftHandSideExpression {
        LeftHandSideExpression::Identifier(Box::new(identifier))
    }

    pub fn create_left_hand_side_member(&self, member: Expression) -> LeftHandSideExpression {
        LeftHandSideExpression::Member(Box::new(member))
    }

    pub fn create_left_hand_side_index(&self, index: Expression) -> LeftHandSideExpression {
        LeftHandSideExpression::Index(Box::new(index))
    }

    pub fn create_call_expression(&self, callee: Expression, args: Vec<Expression>) -> Expression {
        Expression::Call(Box::new(callee), args)
    }

    pub fn create_import_path_expression(&self, segments: Vec<Expression>) -> Expression {
        Expression::ImportPath(segments)
    }

    pub fn create_use_statement(&self, path: Expression, alias: Option<Box<Expression>>) -> Statement {
        Statement::Use(Box::new(path), alias)
    }

    pub fn create_type_expression(&self, inner: Type, is_nullable: bool) -> Expression {
        Expression::Type(Box::new(inner), is_nullable)
    }

    pub fn create_generic_type_expression(&self, name: Expression, constraint: Option<Box<Expression>>) -> Expression {
        Expression::GenericType(Box::new(name), constraint)
    }

    pub fn create_type_declaration(&self, name: Expression, kind: TypeDeclarationKind, type_expr: Option<Box<Expression>>) -> Expression {
        Expression::TypeDeclaration(Box::new(name), kind, type_expr)
    }

    pub fn create_type_statement(&self, declarations: Vec<Expression>) -> Statement {
        Statement::Type(declarations)
    }

    pub fn create_break_statement(&self) -> Statement {
        Statement::Break
    }

    pub fn create_continue_statement(&self) -> Statement {
        Statement::Continue
    }
}

pub fn opt_expr(expr: Expression) -> Option<Box<Expression>> {
    Some(Box::new(expr))
}