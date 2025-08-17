#![allow(dead_code)] // Allow unused functions, as not all helpers may be used in every test file.

use miri::ast::*;

// === Expression Builders ===

pub fn empty_statement() -> Statement {
    Statement::Empty
}

pub fn empty_program() -> Vec<Statement> {
    vec![Statement::Empty]
}

pub fn identifier(name: &str) -> Expression {
    Expression::Identifier(name.into())
}

pub fn int(val: i128) -> IntegerLiteral {
    match val {
        v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => IntegerLiteral::I8(v as i8),
        v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => IntegerLiteral::I16(v as i16),
        v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => IntegerLiteral::I32(v as i32),
        v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => IntegerLiteral::I64(v as i64),
        _ => IntegerLiteral::I128(val),
    }
}

pub fn int_literal(val: i128) -> Expression {
    let literal = int(val);
    Expression::Literal(Literal::Integer(literal))
}

pub fn float32(val: f32) -> FloatLiteral {
    FloatLiteral::F32(val)
}

pub fn float64(val: f64) -> FloatLiteral {
    FloatLiteral::F64(val)
}

pub fn float32_literal(val: f32) -> Expression {
    let literal = float32(val);
    Expression::Literal(Literal::Float(literal))
}

pub fn float64_literal(val: f64) -> Expression {
    let literal = float64(val);
    Expression::Literal(Literal::Float(literal))
}

pub fn string(val: &str) -> Literal {
    Literal::String(val.to_string())
}

pub fn string_literal(val: &str) -> Expression {
    Expression::Literal(string(val))
}

pub fn boolean(val: bool) -> Literal {
    Literal::Boolean(val)
}

pub fn boolean_literal(val: bool) -> Expression {
    Expression::Literal(boolean(val))
}

pub fn symbol(val: &str) -> Literal {
    Literal::Symbol(val.to_string())
}

pub fn binary(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    Expression::Binary(Box::new(left), op, Box::new(right))
}

pub fn unary(op: UnaryOp, expr: Expression) -> Expression {
    Expression::Unary(op, Box::new(expr))
}

pub fn logical(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    Expression::Logical(Box::new(left), op, Box::new(right))
}

pub fn assign(left: LeftHandSideExpression, op: AssignmentOp, right: Expression) -> Expression {
    Expression::Assignment(Box::new(left), op, Box::new(right))
}

pub fn let_variable(name: &str, typ: Option<String>, init: Option<Box<Expression>>) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Immutable,
    }
}

pub fn var(name: &str, typ: Option<String>, init: Option<Box<Expression>>) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Mutable,
    }
}

pub fn conditional(cond: Expression, then: Expression, else_b: Option<Expression>, if_type: IfStatementType) -> Expression {
    Expression::Conditional(Box::new(cond), Box::new(then), else_b.map(Box::new), if_type)
}

pub fn if_conditional(cond: Expression, then: Expression, else_b: Option<Expression>) -> Expression {
    conditional(cond, then, else_b, IfStatementType::If)
}

pub fn unless_conditional(cond: Expression, then: Expression, else_b: Option<Expression>) -> Expression {
    conditional(cond, then, else_b, IfStatementType::Unless)
}

pub fn range(start: Expression, end: Option<Box<Expression>>, range_type: RangeExpressionType) -> Expression {
    Expression::Range(Box::new(start), end, range_type)
}

pub fn member(object: Expression, property: Expression) -> Expression {
    Expression::Member(Box::new(object), Box::new(property))
}

pub fn index(object: Expression, index: Expression) -> Expression {
    Expression::Index(Box::new(object), Box::new(index))
}

pub fn lhs_identifier(name: &str) -> LeftHandSideExpression {
    LeftHandSideExpression::Identifier(Box::new(identifier(name)))
}

pub fn lhs_member(object: Expression, property: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Member(Box::new(member(object, property)))
}

pub fn lhs_index(object: Expression, idx: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Index(Box::new(index(object, idx)))
}

pub fn call(callee: Expression, args: Vec<Expression>) -> Expression {
    Expression::Call(Box::new(callee), args)
}

// === Statement Builders ===

pub fn variable_statement(declarations: Vec<VariableDeclaration>) -> Statement {
    Statement::Variable(declarations)
}

pub fn expression_statement(expr: Expression) -> Statement {
    Statement::Expression(expr)
}

pub fn block(stmts: Vec<Statement>) -> Statement {
    Statement::Block(stmts)
}

pub fn if_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    Statement::If(Box::new(cond), Box::new(then), else_b.map(Box::new), IfStatementType::If)
}

pub fn while_statement(cond: Expression, body: Statement) -> Statement {
    Statement::While(Box::new(cond), Box::new(body), WhileStatementType::While)
}

pub fn until_statement(cond: Expression, body: Statement) -> Statement {
    Statement::While(Box::new(cond), Box::new(body), WhileStatementType::Until)
}

pub fn forever_statement(body: Statement) -> Statement {
    Statement::While(Box::new(Expression::Literal(Literal::Boolean(true))), Box::new(body), WhileStatementType::Forever)
}

pub fn for_statement(
    variable_declarations: Vec<VariableDeclaration>,
    iterable: Expression,
    body: Statement,
) -> Statement {
    Statement::For(variable_declarations, Box::new(iterable), Box::new(body))
}

pub fn return_statement(expr: Option<Box<Expression>>) -> Statement {
    Statement::Return(expr)
}

pub fn guard(op: GuardOp, expr: Expression) -> Expression {
    Expression::Guard(op, Box::new(expr))
}

pub fn def(
    name: String,
    parameters: Vec<Parameter>,
    return_type: Option<String>,
    body: Statement,
) -> Statement {
    Statement::FunctionDeclaration(
        name,
        parameters,
        return_type,
        Box::new(body)
    )
}

pub fn parameter(name: String, typ: Option<String>, guard: Option<Box<Expression>>) -> Parameter {
    Parameter { name, typ, guard }
}

pub fn import_path(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    Expression::ImportPath(segments)
}

pub fn use_statement(import_path: Expression, alias: Option<Box<Expression>>) -> Statement {
    Statement::Use(Box::new(import_path), alias)
}
