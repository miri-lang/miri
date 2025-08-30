// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

#![allow(dead_code)] // Allow unused functions, as not all helpers may be used in every test file.

use miri::{ast::*, lexer::RegexToken};

// === Expression Builders ===

pub fn empty_statement() -> Statement {
    Statement::Empty
}

pub fn empty_program() -> Vec<Statement> {
    vec![]
}

pub fn identifier(name: &str) -> Expression {
    Expression::Identifier(name.into(), None)
}

pub fn class_identifier(name: &str) -> Expression {
    let parts = name.split("::").collect::<Vec<&str>>();
    let class = parts[0].to_string();
    let id_name = parts[1].to_string();

    Expression::Identifier(id_name, Some(class))
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

pub fn int_literal(val: i128) -> Literal {
    Literal::Integer(int(val))
}

pub fn int_literal_expression(val: i128) -> Expression {
    Expression::Literal(int_literal(val))
}

pub fn float32(val: f32) -> FloatLiteral {
    FloatLiteral::F32(val.to_bits())
}

pub fn float64(val: f64) -> FloatLiteral {
    FloatLiteral::F64(val.to_bits())
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

pub fn f_string(parts: Vec<Expression>) -> Expression {
    Expression::FormattedString(parts)
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

pub fn symbol_literal(val: &str) -> Expression {
    Expression::Literal(symbol(val))
}

pub fn regex_literal(body: &str, flags: &str) -> Expression {
    let token = RegexToken {
        body: body.to_string(),
        ignore_case: flags.contains('i'),
        global: flags.contains('g'),
        multiline: flags.contains('m'),
        dot_all: flags.contains('s'),
        unicode: flags.contains('u'),
    };
    Expression::Literal(Literal::Regex(token))
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

pub fn let_variable(name: &str, typ: Option<Box<Expression>>, init: Option<Box<Expression>>) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Immutable,
    }
}

pub fn var(name: &str, typ: Option<Box<Expression>>, init: Option<Box<Expression>>) -> VariableDeclaration {
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

pub fn iter_obj(start: Expression) -> Expression {
    Expression::Range(Box::new(start), None, RangeExpressionType::IterableObject)
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

pub fn variable_statement(declarations: Vec<VariableDeclaration>, visibility: MemberVisibility) -> Statement {
    Statement::Variable(declarations, visibility)
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

pub fn do_while_statement(cond: Expression, body: Statement) -> Statement {
    Statement::While(Box::new(cond), Box::new(body), WhileStatementType::DoWhile)
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

pub fn parameter(name: String, typ: Option<Box<Expression>>, guard: Option<Box<Expression>>) -> Parameter {
    Parameter { name, typ, guard }
}

pub fn import_path(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    Expression::ImportPath(segments)
}

pub fn use_statement(import_path: Expression, alias: Option<Box<Expression>>) -> Statement {
    Statement::Use(Box::new(import_path), alias)
}

pub fn generic_type(name: &str, constraint: Option<Box<Expression>>) -> Expression {
    Expression::GenericType(Box::new(identifier(name)), constraint)
}

pub fn typ(t: Type) -> Expression {
    Expression::Type(Box::new(t), false)
}

pub fn null_typ(t: Type) -> Expression {
    Expression::Type(Box::new(t), true)
}

pub fn type_declaration(name: &str, kind: TypeDeclarationKind, type_expr: Option<Box<Expression>>) -> Expression {
    Expression::TypeDeclaration(Box::new(identifier(name)), kind, type_expr)
}

pub fn type_statement(declarations: Vec<Expression>, visibility: MemberVisibility) -> Statement {
    Statement::Type(declarations, visibility)
}

pub fn break_statement() -> Statement {
    Statement::Break
}

pub fn continue_statement() -> Statement {
    Statement::Continue
}

pub fn enum_statement(name: Expression, values: Vec<Expression>, visibility: MemberVisibility) -> Statement {
    Statement::Enum(Box::new(name), values, visibility)
}

pub fn enum_value(name: &str, types: Vec<Expression>) -> Expression {
    Expression::EnumValue(Box::new(identifier(name)), types)
}

pub fn struct_statement(name: Expression, members: Vec<Expression>, visibility: MemberVisibility) -> Statement {
    Statement::Struct(Box::new(name), members, visibility)
}

pub fn struct_member(name: &str, typ: Expression) -> Expression {
    Expression::StructMember(Box::new(identifier(name)), Box::new(typ))
}

pub fn extends(base: Expression) -> Statement {
    Statement::Extends(Box::new(base))
}

pub fn implements(traits: Vec<Expression>) -> Statement {
    Statement::Implements(traits)
}

pub fn includes(modules: Vec<Expression>) -> Statement {
    Statement::Includes(modules)
}

pub fn list(elements: Vec<Expression>) -> Expression {
    Expression::List(elements)
}

pub fn map(pairs: Vec<(Expression, Expression)>) -> Expression {
    Expression::Map(pairs)
}

pub fn tuple(elements: Vec<Expression>) -> Expression {
    Expression::Tuple(elements)
}

pub fn set(elements: Vec<Expression>) -> Expression {
    Expression::Set(elements)
}

pub fn match_expression(subject: Expression, branches: Vec<MatchBranch>) -> Expression {
    Expression::Match(Box::new(subject), branches)
}

pub struct FunctionBuilder {
    name: String,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    properties: FunctionProperties,
}

impl FunctionBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            generic_types: None,
            parameters: vec![],
            return_type: None,
            properties: FunctionProperties {
                is_async: false,
                is_gpu: false,
                visibility: MemberVisibility::Public,
            },
        }
    }

    pub fn generics(mut self, generics: Vec<Expression>) -> Self {
        self.generic_types = Some(generics);
        self
    }

    pub fn params(mut self, params: Vec<Parameter>) -> Self {
        self.parameters = params;
        self
    }

    pub fn return_type(mut self, ret_type: Expression) -> Self {
        self.return_type = Some(Box::new(ret_type));
        self
    }

    pub fn set_async(mut self) -> Self {
        self.properties.is_async = true;
        self
    }

    pub fn set_gpu(mut self) -> Self {
        self.properties.is_gpu = true;
        self
    }

    pub fn set_private(mut self) -> Self {
        self.properties.visibility = MemberVisibility::Private;
        self
    }

    pub fn set_protected(mut self) -> Self {
        self.properties.visibility = MemberVisibility::Protected;
        self
    }

    pub fn build(self, body: Statement) -> Statement {
        Statement::FunctionDeclaration(
            self.name,
            self.generic_types,
            self.parameters,
            self.return_type,
            Box::new(body),
            self.properties,
        )
    }

    pub fn build_empty_body(self) -> Statement {
        self.build(empty_statement())
    }

    pub fn build_lambda(self, body: Statement) -> Expression {
        Expression::Lambda(
            self.generic_types,
            self.parameters,
            self.return_type,
            Box::new(body),
            self.properties,
        )
    }

    pub fn build_lambda_empty_body(self) -> Expression {
        self.build_lambda(empty_statement())
    }
}

pub fn func(name: &str) -> FunctionBuilder {
    FunctionBuilder::new(name)
}

pub fn lambda() -> FunctionBuilder {
    FunctionBuilder::new("")
}