// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::{ast::*, error::syntax::Span, lexer::RegexToken};
use std::sync::atomic::{AtomicUsize, Ordering};
use types::*;

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn next_id() -> usize {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn expr_with_span(kind: ExpressionKind, span: Span) -> Expression {
    Expression {
        id: next_id(),
        node: kind,
        span,
    }
}

fn expr(kind: ExpressionKind) -> Expression {
    expr_with_span(kind, 0..0)
}

pub fn stmt_with_span(kind: StatementKind, span: Span) -> Statement {
    Statement {
        id: next_id(),
        node: kind,
        span,
    }
}

pub fn stmt(kind: StatementKind) -> Statement {
    stmt_with_span(kind, 0..0)
}

pub fn identifier_with_span(name: &str, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Identifier(name.into(), None), span)
}

pub fn identifier_with_class_and_span(name: &str, class: Option<String>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Identifier(name.into(), class), span)
}

pub fn literal_with_span(value: Literal, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Literal(value), span)
}

pub fn binary_with_span(
    left: Expression,
    op: BinaryOp,
    right: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Binary(Box::new(left), op, Box::new(right)),
        span,
    )
}

pub fn unary_with_span(op: UnaryOp, expr_node: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Unary(op, Box::new(expr_node)), span)
}

pub fn logical_with_span(
    left: Expression,
    op: BinaryOp,
    right: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Logical(Box::new(left), op, Box::new(right)),
        span,
    )
}

pub fn call_with_span(callee: Expression, args: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Call(Box::new(callee), args), span)
}

pub fn member_with_span(object: Expression, property: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Member(Box::new(object), Box::new(property)),
        span,
    )
}

pub fn index_with_span(object: Expression, index: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Index(Box::new(object), Box::new(index)),
        span,
    )
}

pub fn assign_with_span(
    left: LeftHandSideExpression,
    op: AssignmentOp,
    right: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Assignment(Box::new(left), op, Box::new(right)),
        span,
    )
}

pub fn list_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::List(elements), span)
}

pub fn map_with_span(pairs: Vec<(Expression, Expression)>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Map(pairs), span)
}

pub fn tuple_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Tuple(elements), span)
}

pub fn set_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Set(elements), span)
}

pub fn match_expression_with_span(
    subject: Expression,
    branches: Vec<MatchBranch>,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::Match(Box::new(subject), branches), span)
}

pub fn f_string_with_span(parts: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::FormattedString(parts), span)
}

pub fn type_expression_with_span(inner: Type, is_nullable: bool, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Type(Box::new(inner), is_nullable), span)
}

pub fn generic_type_expression_with_span(
    name_expression: Expression,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::GenericType(Box::new(name_expression), constraint, kind),
        span,
    )
}

pub fn generic_type_expression(
    name_expression: Expression,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
) -> Expression {
    generic_type_expression_with_span(name_expression, constraint, kind, 0..0)
}

pub fn conditional_with_span(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
    if_type: IfStatementType,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Conditional(
            Box::new(then),
            Box::new(cond),
            else_b.map(Box::new),
            if_type,
        ),
        span,
    )
}

pub fn range_with_span(
    start: Expression,
    end: Option<Box<Expression>>,
    range_type: RangeExpressionType,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::Range(Box::new(start), end, range_type),
        span,
    )
}

pub fn guard_with_span(op: GuardOp, expr_node: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Guard(op, Box::new(expr_node)), span)
}

pub fn import_path_expression_with_span(
    segments: Vec<Expression>,
    kind: ImportPathKind,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::ImportPath(segments, kind), span)
}

pub fn type_declaration_expression_with_span(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::TypeDeclaration(Box::new(name), generic_types, kind, type_expr),
        span,
    )
}

pub fn enum_value_expression_with_span(
    name: Expression,
    types: Vec<Expression>,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::EnumValue(Box::new(name), types), span)
}

pub fn struct_member_expression_with_span(
    name: Expression,
    typ: Expression,
    span: Span,
) -> Expression {
    expr_with_span(
        ExpressionKind::StructMember(Box::new(name), Box::new(typ)),
        span,
    )
}

pub fn empty_statement() -> Statement {
    stmt(StatementKind::Empty)
}

pub fn program(statements: Vec<Statement>) -> Program {
    Program { body: statements }
}

pub fn empty_program() -> Vec<Statement> {
    vec![]
}

pub fn identifier_with_class(name: &str, class: Option<String>) -> Expression {
    expr(ExpressionKind::Identifier(name.into(), class))
}

pub fn identifier(name: &str) -> Expression {
    identifier_with_class(name, None)
}

pub fn literal(value: Literal) -> Expression {
    expr(ExpressionKind::Literal(value))
}

pub fn class_identifier(name: &str) -> Expression {
    let parts = name.split("::").collect::<Vec<&str>>();
    let class = parts[0].to_string();
    let id_name = parts[1].to_string();

    expr(ExpressionKind::Identifier(id_name, Some(class)))
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
    expr(ExpressionKind::Literal(int_literal(val)))
}

pub fn float32(val: f32) -> FloatLiteral {
    FloatLiteral::F32(val.to_bits())
}

pub fn float64(val: f64) -> FloatLiteral {
    FloatLiteral::F64(val.to_bits())
}

pub fn float32_literal(val: f32) -> Literal {
    let literal = float32(val);
    Literal::Float(literal)
}

pub fn float32_literal_expression(val: f32) -> Expression {
    literal(float32_literal(val))
}

pub fn float64_literal(val: f64) -> Literal {
    let literal = float64(val);
    Literal::Float(literal)
}

pub fn float64_literal_expression(val: f64) -> Expression {
    literal(float64_literal(val))
}

pub fn string_literal(val: &str) -> Literal {
    Literal::String(val.to_string())
}

pub fn string_literal_expression(val: &str) -> Expression {
    expr(ExpressionKind::Literal(string_literal(val)))
}

pub fn f_string(parts: Vec<Expression>) -> Expression {
    expr(ExpressionKind::FormattedString(parts))
}

pub fn boolean(val: bool) -> Literal {
    Literal::Boolean(val)
}

pub fn boolean_literal(val: bool) -> Expression {
    expr(ExpressionKind::Literal(boolean(val)))
}

pub fn symbol(val: &str) -> Literal {
    Literal::Symbol(val.to_string())
}

pub fn symbol_literal(val: &str) -> Expression {
    expr(ExpressionKind::Literal(symbol(val)))
}

pub fn regex_literal_from_token(value: RegexToken) -> Literal {
    Literal::Regex(value)
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
    expr(ExpressionKind::Literal(regex_literal_from_token(token)))
}

pub fn binary(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    expr(ExpressionKind::Binary(Box::new(left), op, Box::new(right)))
}

pub fn unary(op: UnaryOp, expr_node: Expression) -> Expression {
    expr(ExpressionKind::Unary(op, Box::new(expr_node)))
}

pub fn logical(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    expr(ExpressionKind::Logical(Box::new(left), op, Box::new(right)))
}

pub fn assign(left: LeftHandSideExpression, op: AssignmentOp, right: Expression) -> Expression {
    expr(ExpressionKind::Assignment(
        Box::new(left),
        op,
        Box::new(right),
    ))
}

pub fn let_variable(
    name: &str,
    typ: Option<Box<Expression>>,
    init: Option<Box<Expression>>,
) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Immutable,
    }
}

pub fn var(
    name: &str,
    typ: Option<Box<Expression>>,
    init: Option<Box<Expression>>,
) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Mutable,
    }
}

pub fn conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
    if_type: IfStatementType,
) -> Expression {
    expr(ExpressionKind::Conditional(
        Box::new(then),
        Box::new(cond),
        else_b.map(Box::new),
        if_type,
    ))
}

pub fn if_conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
) -> Expression {
    conditional(then, cond, else_b, IfStatementType::If)
}

pub fn unless_conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
) -> Expression {
    conditional(then, cond, else_b, IfStatementType::Unless)
}

pub fn range(
    start: Expression,
    end: Option<Box<Expression>>,
    range_type: RangeExpressionType,
) -> Expression {
    expr(ExpressionKind::Range(Box::new(start), end, range_type))
}

pub fn iter_obj(start: Expression) -> Expression {
    expr(ExpressionKind::Range(
        Box::new(start),
        None,
        RangeExpressionType::IterableObject,
    ))
}

pub fn member(object: Expression, property: Expression) -> Expression {
    expr(ExpressionKind::Member(Box::new(object), Box::new(property)))
}

pub fn index(object: Expression, index: Expression) -> Expression {
    expr(ExpressionKind::Index(Box::new(object), Box::new(index)))
}

pub fn lhs_identifier_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Identifier(Box::new(expr))
}

pub fn lhs_identifier(name: &str) -> LeftHandSideExpression {
    lhs_identifier_from_expr(identifier(name))
}

pub fn lhs_member_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Member(Box::new(expr))
}

pub fn lhs_member(object: Expression, property: Expression) -> LeftHandSideExpression {
    lhs_member_from_expr(member(object, property))
}

pub fn lhs_index_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Index(Box::new(expr))
}

pub fn lhs_index(object: Expression, idx: Expression) -> LeftHandSideExpression {
    lhs_index_from_expr(index(object, idx))
}

pub fn call(callee: Expression, args: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Call(Box::new(callee), args))
}

pub fn variable_statement(
    declarations: Vec<VariableDeclaration>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Variable(declarations, visibility))
}

pub fn expression_statement(expr: Expression) -> Statement {
    let span = expr.span.clone();
    stmt_with_span(StatementKind::Expression(expr), span)
}

pub fn block_statement(stmts: Vec<Statement>) -> Statement {
    stmt(StatementKind::Block(stmts))
}

pub fn block(stmts: Vec<Statement>) -> Statement {
    block_statement(stmts)
}

pub fn if_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    stmt(StatementKind::If(
        Box::new(cond),
        Box::new(then),
        else_b.map(Box::new),
        IfStatementType::If,
    ))
}

pub fn unless_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    stmt(StatementKind::If(
        Box::new(cond),
        Box::new(then),
        else_b.map(Box::new),
        IfStatementType::Unless,
    ))
}

pub fn while_statement_with_type(
    cond: Expression,
    body: Statement,
    while_statement_type: WhileStatementType,
) -> Statement {
    stmt(StatementKind::While(
        Box::new(cond),
        Box::new(body),
        while_statement_type,
    ))
}

pub fn while_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::While)
}

pub fn do_while_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::DoWhile)
}

pub fn until_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::Until)
}

pub fn forever_statement(body: Statement) -> Statement {
    while_statement_with_type(
        expr(ExpressionKind::Literal(Literal::Boolean(true))),
        body,
        WhileStatementType::Forever,
    )
}

pub fn for_statement(
    variable_declarations: Vec<VariableDeclaration>,
    iterable: Expression,
    body: Statement,
) -> Statement {
    stmt(StatementKind::For(
        variable_declarations,
        Box::new(iterable),
        Box::new(body),
    ))
}

pub fn return_statement(expr: Option<Box<Expression>>) -> Statement {
    stmt(StatementKind::Return(expr))
}

pub fn guard(op: GuardOp, expr_node: Expression) -> Expression {
    expr(ExpressionKind::Guard(op, Box::new(expr_node)))
}

pub fn parameter(
    name: String,
    typ: Expression,
    guard: Option<Box<Expression>>,
    default_value: Option<Box<Expression>>,
) -> Parameter {
    Parameter {
        name,
        typ: Box::new(typ),
        guard,
        default_value,
    }
}

pub fn import_path_expression(segments: Vec<Expression>, kind: ImportPathKind) -> Expression {
    expr(ExpressionKind::ImportPath(segments, kind))
}

pub fn import_path(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Simple)
}

pub fn import_path_wildcard(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Wildcard)
}

pub fn import_path_multi(
    path: &str,
    items: Vec<(Expression, Option<Box<Expression>>)>,
) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Multi(items))
}

pub fn use_statement(import_path: Expression, alias: Option<Box<Expression>>) -> Statement {
    stmt(StatementKind::Use(Box::new(import_path), alias))
}

pub fn generic_type(name: &str, constraint: Option<Box<Expression>>) -> Expression {
    generic_type_expression(identifier(name), constraint, TypeDeclarationKind::None)
}

pub fn generic_type_with_kind(
    name: &str,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
) -> Expression {
    generic_type_expression(identifier(name), constraint, kind)
}

pub fn type_expression(inner: Type, is_nullable: bool) -> Expression {
    expr(ExpressionKind::Type(Box::new(inner), is_nullable))
}

pub fn type_expr_non_null(t: Type) -> Expression {
    type_expression(t, false)
}

pub fn type_expr_null(t: Type) -> Expression {
    type_expression(t, true)
}

pub fn make_type(kind: TypeKind) -> Type {
    Type::new(kind, 0..0)
}

pub fn type_int() -> Type {
    make_type(TypeKind::Int)
}
pub fn type_float() -> Type {
    make_type(TypeKind::Float)
}
pub fn type_string() -> Type {
    make_type(TypeKind::String)
}
pub fn type_bool() -> Type {
    make_type(TypeKind::Boolean)
}
pub fn type_void() -> Type {
    make_type(TypeKind::Void)
}
pub fn type_f64() -> Type {
    make_type(TypeKind::F64)
}
pub fn type_f32() -> Type {
    make_type(TypeKind::F32)
}
pub fn type_i128() -> Type {
    make_type(TypeKind::I128)
}
pub fn type_i64() -> Type {
    make_type(TypeKind::I64)
}
pub fn type_i32() -> Type {
    make_type(TypeKind::I32)
}
pub fn type_i16() -> Type {
    make_type(TypeKind::I16)
}
pub fn type_i8() -> Type {
    make_type(TypeKind::I8)
}
pub fn type_u128() -> Type {
    make_type(TypeKind::U128)
}
pub fn type_u64() -> Type {
    make_type(TypeKind::U64)
}
pub fn type_u32() -> Type {
    make_type(TypeKind::U32)
}
pub fn type_u16() -> Type {
    make_type(TypeKind::U16)
}
pub fn type_u8() -> Type {
    make_type(TypeKind::U8)
}

pub fn type_list(inner: Type) -> Type {
    make_type(TypeKind::List(Box::new(type_expr_non_null(inner))))
}

pub fn type_map(k: Type, v: Type) -> Type {
    make_type(TypeKind::Map(
        Box::new(type_expr_non_null(k)),
        Box::new(type_expr_non_null(v)),
    ))
}

pub fn type_set(inner: Type) -> Type {
    make_type(TypeKind::Set(Box::new(type_expr_non_null(inner))))
}

pub fn type_tuple(elements: Vec<Type>) -> Type {
    make_type(TypeKind::Tuple(
        elements.into_iter().map(type_expr_non_null).collect(),
    ))
}

pub fn type_null(inner: Type) -> Type {
    make_type(TypeKind::Nullable(Box::new(inner)))
}

pub fn type_result(ok: Type, err: Type) -> Type {
    make_type(TypeKind::Result(
        Box::new(type_expr_non_null(ok)),
        Box::new(type_expr_non_null(err)),
    ))
}

pub fn type_custom(name: &str, args: Option<Vec<Expression>>) -> Type {
    make_type(TypeKind::Custom(name.to_string(), args))
}

pub fn type_future(inner: Type) -> Type {
    make_type(TypeKind::Future(Box::new(type_expr_non_null(inner))))
}

pub fn type_function(
    generics: Option<Vec<Expression>>,
    params: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
) -> Type {
    make_type(TypeKind::Function(generics, params, return_type))
}

pub fn type_symbol() -> Type {
    make_type(TypeKind::Symbol)
}

pub fn type_declaration_expression(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
) -> Expression {
    expr(ExpressionKind::TypeDeclaration(
        Box::new(name),
        generic_types,
        kind,
        type_expr,
    ))
}

pub fn type_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
) -> Expression {
    type_declaration_expression(identifier(name), generic_types, kind, type_expr)
}

pub fn type_statement(declarations: Vec<Expression>, visibility: MemberVisibility) -> Statement {
    stmt(StatementKind::Type(declarations, visibility))
}

pub fn break_statement() -> Statement {
    stmt(StatementKind::Break)
}

pub fn continue_statement() -> Statement {
    stmt(StatementKind::Continue)
}

pub fn enum_statement(
    name: Expression,
    values: Vec<Expression>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Enum(Box::new(name), values, visibility))
}

pub fn enum_value_expression(name: Expression, types: Vec<Expression>) -> Expression {
    expr(ExpressionKind::EnumValue(Box::new(name), types))
}

pub fn enum_value(name: &str, types: Vec<Expression>) -> Expression {
    enum_value_expression(identifier(name), types)
}

pub fn struct_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    members: Vec<Expression>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Struct(
        Box::new(name),
        generic_types,
        members,
        visibility,
    ))
}

pub fn struct_member_expression(name: Expression, typ: Expression) -> Expression {
    expr(ExpressionKind::StructMember(Box::new(name), Box::new(typ)))
}

pub fn struct_member(name: &str, typ: Expression) -> Expression {
    struct_member_expression(identifier(name), typ)
}

pub fn extends(base: Expression) -> Statement {
    stmt(StatementKind::Extends(Box::new(base)))
}

pub fn implements(traits: Vec<Expression>) -> Statement {
    stmt(StatementKind::Implements(traits))
}

pub fn includes(modules: Vec<Expression>) -> Statement {
    stmt(StatementKind::Includes(modules))
}

pub fn list(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::List(elements))
}

pub fn map(pairs: Vec<(Expression, Expression)>) -> Expression {
    expr(ExpressionKind::Map(pairs))
}

pub fn tuple(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Tuple(elements))
}

pub fn set(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Set(elements))
}

pub fn match_expression(subject: Expression, branches: Vec<MatchBranch>) -> Expression {
    expr(ExpressionKind::Match(Box::new(subject), branches))
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

    pub fn properties(mut self, properties: FunctionProperties) -> Self {
        self.properties = properties;
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
        stmt(StatementKind::FunctionDeclaration(
            self.name,
            self.generic_types,
            self.parameters,
            self.return_type,
            Box::new(body),
            self.properties,
        ))
    }

    pub fn build_empty_body(self) -> Statement {
        self.build(empty_statement())
    }

    pub fn build_lambda(self, body: Statement) -> Expression {
        expr(ExpressionKind::Lambda(
            self.generic_types,
            self.parameters,
            self.return_type,
            Box::new(body),
            self.properties,
        ))
    }

    pub fn build_lambda_empty_body(self) -> Expression {
        self.build_lambda(empty_statement())
    }
}

pub fn func(name: &str) -> FunctionBuilder {
    FunctionBuilder::new(name)
}

pub fn function_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    body: Statement,
    properties: FunctionProperties,
) -> Statement {
    stmt(StatementKind::FunctionDeclaration(
        name.into(),
        generic_types,
        parameters,
        return_type,
        Box::new(body),
        properties,
    ))
}

pub fn lambda() -> FunctionBuilder {
    FunctionBuilder::new("")
}

pub fn lambda_expression(
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    body: Statement,
    properties: FunctionProperties,
) -> Expression {
    expr(ExpressionKind::Lambda(
        generic_types,
        parameters,
        return_type,
        Box::new(body),
        properties,
    ))
}

pub fn named_argument(name: String, value: Expression) -> Expression {
    expr(ExpressionKind::NamedArgument(name, Box::new(value)))
}

pub fn named_argument_with_span(name: String, value: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::NamedArgument(name, Box::new(value)), span)
}
