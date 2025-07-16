use crate::lexer::Span;


/// Represents a fully parsed Miri program
#[derive(Debug, PartialEq)]
pub struct Program {
    pub body: Literal,
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

/// Represents an expression
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    // Literals
    Literal(Literal),
    
    // Variables, fields and indexing
    Identifier(String),

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

// /// Represents a block of statements
// #[derive(Debug, PartialEq)]
// pub struct Block {
//     pub statements: Vec<Stmt>,
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

// /// Represents a binary operator
// #[derive(Debug, PartialEq, Clone, Copy)]
// pub enum BinaryOp {
//     Add, Sub, Mul, Div, Mod,
//     Eq, Neq, Lt, Lte, Gt, Gte,
//     And, Or,
//     Range,
//     In,
// }

// /// Represents a unary operator
// #[derive(Debug, PartialEq, Clone, Copy)]
// pub enum UnaryOp {
//     Neg, Not,
// }

// // Display implementations for error reporting
// impl fmt::Display for BinaryOp {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             BinaryOp::Add => write!(f, "+"),
//             BinaryOp::Sub => write!(f, "-"),
//             BinaryOp::Mul => write!(f, "*"),
//             BinaryOp::Div => write!(f, "/"),
//             BinaryOp::Mod => write!(f, "%"),
//             BinaryOp::Eq => write!(f, "=="),
//             BinaryOp::Neq => write!(f, "!="),
//             BinaryOp::Lt => write!(f, "<"),
//             BinaryOp::Lte => write!(f, "<="),
//             BinaryOp::Gt => write!(f, ">"),
//             BinaryOp::Gte => write!(f, ">="),
//             BinaryOp::And => write!(f, "and"),
//             BinaryOp::Or => write!(f, "or"),
//             BinaryOp::Range => write!(f, ".."),
//             BinaryOp::In => write!(f, "in"),
//         }
//     }
// }

// impl fmt::Display for UnaryOp {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             UnaryOp::Neg => write!(f, "-"),
//             UnaryOp::Not => write!(f, "not"),
//         }
//     }
// }