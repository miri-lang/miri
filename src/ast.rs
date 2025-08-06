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
}

#[derive(Debug, PartialEq, Clone)]
pub enum Statement {
    Empty, // Represents an empty statement, e.g., when a block is empty
    Expression(Expression),
    Block(Vec<Statement>),
    Variable(Vec<VariableDeclaration>),
    If(Box<Expression>, Box<Statement>, Option<Box<Statement>>, IfStatementType), // condition, then_block, else_block, type
    While(Box<Expression>, Box<Statement>, WhileStatementType), // condition, then_block, type
}

/// Represents an expression
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    // Literals
    Literal(Literal),
    
    // Variables, fields and indexing
    Identifier(String),

    Binary(Box<Expression>, BinaryOp, Box<Expression>),

    Logical(Box<Expression>, BinaryOp, Box<Expression>),

    Unary(UnaryOp, Box<Expression>),

    Assignment(Box<LeftHandSideExpression>, AssignmentOp, Box<Expression>),

    Conditional(Box<Expression>, Box<Expression>, Option<Box<Expression>>), // condition, then_expr, else_expr

    // FieldAccess(Box<Expr>, String), // expr.field
    // Index(Box<Expr>, Box<Expr>),    // expr[index]
    
    // // Function calls
    // Call(Box<Expr>, Vec<Expr>), // function, args
    // MethodCall(Box<Expr>, String, Vec<Expr>), // object, method, args
    
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
    pub typ: Option<String>, // Type can be specified, e.g., "i32", "String"
    pub initializer: Option<Expression>, // Optional initializer expression
    pub declaration_type: VariableDeclarationType, // Whether the variable is mutable
}

/// Represents a left-hand side expression, which can be an identifier or a more complex expression
#[derive(Debug, Clone, PartialEq)]
pub enum LeftHandSideExpression {
    Identifier(String),
    // Other possible left-hand side expression types (e.g., fields, array elements) can be added here
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

// /// Represents a fully parsed Miri program
// #[derive(Debug, PartialEq)]
// pub struct Program {
//     pub statements: Vec<Stmt>,
// }

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

// /// Represents a function parameter
// #[derive(Debug, PartialEq)]
// pub struct Parameter {
//     pub name: String,
//     pub typ: TypeExpr,
//     pub guard: Option<Box<Expr>>,
// }

// /// Represents a type expression
// #[derive(Debug, PartialEq)]
// pub enum TypeExpr {
//     Simple(String),
//     Array(Box<TypeExpr>),
//     Dict(Box<TypeExpr>, Box<TypeExpr>),
//     Generic(String, Vec<TypeExpr>),
//     Result(Box<TypeExpr>, Box<TypeExpr>),
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

    pub fn create_left_hand_side_expression(&self, expression: Expression) -> LeftHandSideExpression {
        match expression {
            Expression::Identifier(name) => LeftHandSideExpression::Identifier(name),
            // Other left-hand side expression types can be added here in the future
            _ => panic!("Unsupported left-hand side expression type"),
        }
    }

    pub fn create_identifier_expression(&self, name: String) -> Expression {
        Expression::Identifier(name)
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

    pub fn create_conditional_expression(&self, then: Expression, condition: Expression, else_branch: Option<Expression>) -> Expression {
        Expression::Conditional(
            Box::new(then),
            Box::new(condition),
            else_branch.map(Box::new)
        )
    }
}