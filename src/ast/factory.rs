// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::{ast::*, error::syntax::Span, lexer::RegexToken};
use std::sync::atomic::{AtomicUsize, Ordering};
use types::*;

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn next_id() -> usize {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Creates an expression with a specific span.
pub fn expr_with_span(kind: ExpressionKind, span: Span) -> Expression {
    Expression {
        id: next_id(),
        node: kind,
        span,
    }
}

fn expr(kind: ExpressionKind) -> Expression {
    expr_with_span(kind, Span::new(0, 0))
}

/// Creates a statement with a specific span.
pub fn stmt_with_span(kind: StatementKind, span: Span) -> Statement {
    Statement {
        id: next_id(),
        node: kind,
        span,
    }
}

/// Creates a statement with a default (empty) span.
pub fn stmt(kind: StatementKind) -> Statement {
    stmt_with_span(kind, Span::new(0, 0))
}

/// Creates an identifier expression with a specific span.
pub fn identifier_with_span(name: &str, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Identifier(name.into(), None), span)
}

/// Creates an identifier expression with an optional class qualifier and span.
pub fn identifier_with_class_and_span(name: &str, class: Option<String>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Identifier(name.into(), class), span)
}

/// Creates a literal expression with a specific span.
pub fn literal_with_span(value: Literal, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Literal(value), span)
}

/// Creates a binary expression with a specific span.
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

/// Creates a unary expression with a specific span.
pub fn unary_with_span(op: UnaryOp, expr_node: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Unary(op, Box::new(expr_node)), span)
}

/// Creates a logical binary expression with a specific span.
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

/// Creates a function call expression with a specific span.
pub fn call_with_span(callee: Expression, args: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Call(Box::new(callee), args), span)
}

/// Creates a member access expression with a specific span.
pub fn member_with_span(object: Expression, property: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Member(Box::new(object), Box::new(property)),
        span,
    )
}

/// Creates an index access expression with a specific span.
pub fn index_with_span(object: Expression, index: Expression, span: Span) -> Expression {
    expr_with_span(
        ExpressionKind::Index(Box::new(object), Box::new(index)),
        span,
    )
}

/// Creates an assignment expression with a specific span.
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

/// Creates a list literal expression with a specific span.
pub fn list_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::List(elements), span)
}

/// Creates a map literal expression with a specific span.
pub fn map_with_span(pairs: Vec<(Expression, Expression)>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Map(pairs), span)
}

/// Creates a tuple literal expression with a specific span.
pub fn tuple_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Tuple(elements), span)
}

/// Creates a set literal expression with a specific span.
pub fn set_with_span(elements: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Set(elements), span)
}

/// Creates a match expression with a specific span.
pub fn match_expression_with_span(
    subject: Expression,
    branches: Vec<MatchBranch>,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::Match(Box::new(subject), branches), span)
}

/// Creates a formatted string expression with a specific span.
pub fn f_string_with_span(parts: Vec<Expression>, span: Span) -> Expression {
    expr_with_span(ExpressionKind::FormattedString(parts), span)
}

/// Creates a type expression with a specific span.
pub fn type_expression_with_span(inner: Type, is_nullable: bool, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Type(Box::new(inner), is_nullable), span)
}

/// Creates a generic type expression with a specific span.
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

/// Creates a generic type expression with a default span.
pub fn generic_type_expression(
    name_expression: Expression,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
) -> Expression {
    generic_type_expression_with_span(name_expression, constraint, kind, Span::new(0, 0))
}

/// Creates a conditional expression with a specific span.
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

/// Creates a range expression with a specific span.
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

/// Creates a guard expression with a specific span.
pub fn guard_with_span(op: GuardOp, expr_node: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::Guard(op, Box::new(expr_node)), span)
}

/// Creates an import path expression with a specific span.
pub fn import_path_expression_with_span(
    segments: Vec<Expression>,
    kind: ImportPathKind,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::ImportPath(segments, kind), span)
}

/// Creates a type declaration expression with a specific span.
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

/// Creates an enum value expression with a specific span.
pub fn enum_value_expression_with_span(
    name: Expression,
    types: Vec<Expression>,
    span: Span,
) -> Expression {
    expr_with_span(ExpressionKind::EnumValue(Box::new(name), types), span)
}

/// Creates a struct member expression with a specific span.
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

/// Creates an empty statement.
pub fn empty_statement() -> Statement {
    stmt(StatementKind::Empty)
}

/// Creates a program from a list of statements.
pub fn program(statements: Vec<Statement>) -> Program {
    Program { body: statements }
}

/// Creates an initially empty list of statements.
pub fn empty_program() -> Vec<Statement> {
    vec![]
}

/// Creates an identifier expression with an optional class qualifier.
pub fn identifier_with_class(name: &str, class: Option<String>) -> Expression {
    expr(ExpressionKind::Identifier(name.into(), class))
}

/// Creates a simple identifier expression.
pub fn identifier(name: &str) -> Expression {
    identifier_with_class(name, None)
}

/// Creates a literal expression.
pub fn literal(value: Literal) -> Expression {
    expr(ExpressionKind::Literal(value))
}

/// Creates a class identifier (e.g., `Class::StaticMember`).
pub fn class_identifier(name: &str) -> Expression {
    let parts = name.split("::").collect::<Vec<&str>>();
    let class = parts[0].to_string();
    let id_name = parts[1].to_string();

    expr(ExpressionKind::Identifier(id_name, Some(class)))
}

/// Creates the smallest possible integer literal from an i128 value.
pub fn int(val: i128) -> IntegerLiteral {
    match val {
        v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => IntegerLiteral::I8(v as i8),
        v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => IntegerLiteral::I16(v as i16),
        v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => IntegerLiteral::I32(v as i32),
        v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => IntegerLiteral::I64(v as i64),
        _ => IntegerLiteral::I128(val),
    }
}

/// Creates an integer literal.
pub fn int_literal(val: i128) -> Literal {
    Literal::Integer(int(val))
}

/// Creates an integer literal expression.
pub fn int_literal_expression(val: i128) -> Expression {
    expr(ExpressionKind::Literal(int_literal(val)))
}

/// Creates a 32-bit float literal.
pub fn float32(val: f32) -> FloatLiteral {
    FloatLiteral::F32(val.to_bits())
}

/// Creates a 64-bit float literal.
pub fn float64(val: f64) -> FloatLiteral {
    FloatLiteral::F64(val.to_bits())
}

/// Creates a 32-bit float literal value.
pub fn float32_literal(val: f32) -> Literal {
    let literal = float32(val);
    Literal::Float(literal)
}

/// Creates a 32-bit float literal expression.
pub fn float32_literal_expression(val: f32) -> Expression {
    literal(float32_literal(val))
}

/// Creates a 64-bit float literal value.
pub fn float64_literal(val: f64) -> Literal {
    let literal = float64(val);
    Literal::Float(literal)
}

/// Creates a 64-bit float literal expression.
pub fn float64_literal_expression(val: f64) -> Expression {
    literal(float64_literal(val))
}

/// Creates a string literal.
pub fn string_literal(val: &str) -> Literal {
    Literal::String(val.to_string())
}

/// Creates a string literal expression.
pub fn string_literal_expression(val: &str) -> Expression {
    expr(ExpressionKind::Literal(string_literal(val)))
}

/// Creates an f-string expression (interpolated string).
pub fn f_string(parts: Vec<Expression>) -> Expression {
    expr(ExpressionKind::FormattedString(parts))
}

/// Creates a boolean literal.
pub fn boolean(val: bool) -> Literal {
    Literal::Boolean(val)
}

/// Creates a boolean literal expression.
pub fn boolean_literal(val: bool) -> Expression {
    expr(ExpressionKind::Literal(boolean(val)))
}

/// Creates a symbol literal.
pub fn symbol(val: &str) -> Literal {
    Literal::Symbol(val.to_string())
}

/// Creates a symbol literal expression.
pub fn symbol_literal(val: &str) -> Expression {
    expr(ExpressionKind::Literal(symbol(val)))
}

/// Creates a regex literal from a token.
pub fn regex_literal_from_token(value: RegexToken) -> Literal {
    Literal::Regex(value)
}

/// Creates a regex literal expression from pattern and flags strings.
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

/// Creates a binary expression.
pub fn binary(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    expr(ExpressionKind::Binary(Box::new(left), op, Box::new(right)))
}

/// Creates a unary expression.
pub fn unary(op: UnaryOp, expr_node: Expression) -> Expression {
    expr(ExpressionKind::Unary(op, Box::new(expr_node)))
}

/// Creates a logical binary expression.
pub fn logical(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    expr(ExpressionKind::Logical(Box::new(left), op, Box::new(right)))
}

/// Creates an assignment expression.
pub fn assign(left: LeftHandSideExpression, op: AssignmentOp, right: Expression) -> Expression {
    expr(ExpressionKind::Assignment(
        Box::new(left),
        op,
        Box::new(right),
    ))
}

/// Creates an immutable variable declaration structure.
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
        is_shared: false,
    }
}

/// Creates a mutable variable declaration structure.
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
        is_shared: false,
    }
}

/// Creates a compile-time constant declaration structure.
pub fn const_variable(
    name: &str,
    typ: Option<Box<Expression>>,
    init: Option<Box<Expression>>,
) -> VariableDeclaration {
    VariableDeclaration {
        name: name.into(),
        typ,
        initializer: init,
        declaration_type: VariableDeclarationType::Constant,
        is_shared: false,
    }
}

/// Creates a conditional expression (ternary or if-else expr).
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

/// Creates an `if` expression.
pub fn if_conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
) -> Expression {
    conditional(then, cond, else_b, IfStatementType::If)
}

/// Creates an `unless` expression.
pub fn unless_conditional(
    then: Expression,
    cond: Expression,
    else_b: Option<Expression>,
) -> Expression {
    conditional(then, cond, else_b, IfStatementType::Unless)
}

/// Creates a range expression.
pub fn range(
    start: Expression,
    end: Option<Box<Expression>>,
    range_type: RangeExpressionType,
) -> Expression {
    expr(ExpressionKind::Range(Box::new(start), end, range_type))
}

/// Creates an iterable object expression (from a range).
pub fn iter_obj(start: Expression) -> Expression {
    expr(ExpressionKind::Range(
        Box::new(start),
        None,
        RangeExpressionType::IterableObject,
    ))
}

/// Creates a member access expression.
pub fn member(object: Expression, property: Expression) -> Expression {
    expr(ExpressionKind::Member(Box::new(object), Box::new(property)))
}

/// Creates an index access expression.
pub fn index(object: Expression, index: Expression) -> Expression {
    expr(ExpressionKind::Index(Box::new(object), Box::new(index)))
}

/// Wraps an expression as a left-hand side identifier.
pub fn lhs_identifier_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Identifier(Box::new(expr))
}

/// Creates a left-hand side identifier from a string name.
pub fn lhs_identifier(name: &str) -> LeftHandSideExpression {
    lhs_identifier_from_expr(identifier(name))
}

/// Wraps an expression as a left-hand side member access.
pub fn lhs_member_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Member(Box::new(expr))
}

/// Creates a left-hand side member access.
pub fn lhs_member(object: Expression, property: Expression) -> LeftHandSideExpression {
    lhs_member_from_expr(member(object, property))
}

/// Wraps an expression as a left-hand side index access.
pub fn lhs_index_from_expr(expr: Expression) -> LeftHandSideExpression {
    LeftHandSideExpression::Index(Box::new(expr))
}

/// Creates a left-hand side index access.
pub fn lhs_index(object: Expression, idx: Expression) -> LeftHandSideExpression {
    lhs_index_from_expr(index(object, idx))
}

/// Creates a function call expression.
pub fn call(callee: Expression, args: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Call(Box::new(callee), args))
}

/// Creates a variable declaration statement.
pub fn variable_statement(
    declarations: Vec<VariableDeclaration>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Variable(declarations, visibility))
}

/// Creates an expression statement (expression used as a statement).
pub fn expression_statement(expr: Expression) -> Statement {
    let span = expr.span;
    stmt_with_span(StatementKind::Expression(expr), span)
}

/// Creates a block statement.
pub fn block_statement(stmts: Vec<Statement>) -> Statement {
    stmt(StatementKind::Block(stmts))
}

/// Alias for `block_statement`.
pub fn block(stmts: Vec<Statement>) -> Statement {
    block_statement(stmts)
}

/// Creates an `if` statement.
pub fn if_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    stmt(StatementKind::If(
        Box::new(cond),
        Box::new(then),
        else_b.map(Box::new),
        IfStatementType::If,
    ))
}

/// Creates an `unless` statement.
pub fn unless_statement(cond: Expression, then: Statement, else_b: Option<Statement>) -> Statement {
    stmt(StatementKind::If(
        Box::new(cond),
        Box::new(then),
        else_b.map(Box::new),
        IfStatementType::Unless,
    ))
}

/// Creates a loop statement of a specific type (while, do-while, etc).
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

/// Creates a `while` loop statement.
pub fn while_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::While)
}

/// Creates a `do-while` loop statement.
pub fn do_while_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::DoWhile)
}

/// Creates an `until` loop statement.
pub fn until_statement(cond: Expression, body: Statement) -> Statement {
    while_statement_with_type(cond, body, WhileStatementType::Until)
}

/// Creates an infinite loop statement.
pub fn forever_statement(body: Statement) -> Statement {
    while_statement_with_type(
        expr(ExpressionKind::Literal(Literal::Boolean(true))),
        body,
        WhileStatementType::Forever,
    )
}

/// Creates a `for` loop statement.
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

/// Creates a return statement.
pub fn return_statement(expr: Option<Box<Expression>>) -> Statement {
    stmt(StatementKind::Return(expr))
}

/// Creates a guard expression.
pub fn guard(op: GuardOp, expr_node: Expression) -> Expression {
    expr(ExpressionKind::Guard(op, Box::new(expr_node)))
}

/// Creates a function parameter.
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

/// Creates an import path expression.
pub fn import_path_expression(segments: Vec<Expression>, kind: ImportPathKind) -> Expression {
    expr(ExpressionKind::ImportPath(segments, kind))
}

/// Creates a simple import path from a dotted string.
pub fn import_path(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Simple)
}

/// Creates a wildcard import path.
pub fn import_path_wildcard(path: &str) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Wildcard)
}

/// Creates a multi-import path.
pub fn import_path_multi(
    path: &str,
    items: Vec<(Expression, Option<Box<Expression>>)>,
) -> Expression {
    let segments: Vec<Expression> = path.split(".").map(|s| identifier(s.trim())).collect();
    import_path_expression(segments, ImportPathKind::Multi(items))
}

/// Creates a use statement.
pub fn use_statement(import_path: Expression, alias: Option<Box<Expression>>) -> Statement {
    stmt(StatementKind::Use(Box::new(import_path), alias))
}

/// Creates a generic type expression bound to a name.
pub fn generic_type(name: &str, constraint: Option<Box<Expression>>) -> Expression {
    generic_type_expression(identifier(name), constraint, TypeDeclarationKind::None)
}

/// Creates a generic type with a specific declaration kind.
pub fn generic_type_with_kind(
    name: &str,
    constraint: Option<Box<Expression>>,
    kind: TypeDeclarationKind,
) -> Expression {
    generic_type_expression(identifier(name), constraint, kind)
}

/// Creates a type expression wrapping a Type.
pub fn type_expression(inner: Type, is_nullable: bool) -> Expression {
    expr(ExpressionKind::Type(Box::new(inner), is_nullable))
}

/// Creates a non-nullable type expression.
pub fn type_expr_non_null(t: Type) -> Expression {
    type_expression(t, false)
}

/// Creates an optional type expression.
pub fn type_expr_option(t: Type) -> Expression {
    type_expression(t, true)
}

/// Creates an optional type expression (legacy alias).
pub fn type_expr_null(t: Type) -> Expression {
    type_expr_option(t)
}

/// Creates a Type of a specific kind with a default span.
pub fn make_type(kind: TypeKind) -> Type {
    Type::new(kind, Span::new(0, 0))
}

/// Creates an `Int` (arbitrary precision) type.
pub fn type_int() -> Type {
    make_type(TypeKind::Int)
}
/// Creates a `Float` (arbitrary precision) type.
pub fn type_float() -> Type {
    make_type(TypeKind::Float)
}
/// Creates a `String` type.
pub fn type_string() -> Type {
    make_type(TypeKind::String)
}
/// Creates a `Boolean` type.
pub fn type_bool() -> Type {
    make_type(TypeKind::Boolean)
}
/// Creates a `Void` type.
pub fn type_void() -> Type {
    make_type(TypeKind::Void)
}
/// Creates an `F64` type.
pub fn type_f64() -> Type {
    make_type(TypeKind::F64)
}
/// Creates an `F32` type.
pub fn type_f32() -> Type {
    make_type(TypeKind::F32)
}
/// Creates an `I128` type.
pub fn type_i128() -> Type {
    make_type(TypeKind::I128)
}
/// Creates an `I64` type.
pub fn type_i64() -> Type {
    make_type(TypeKind::I64)
}
/// Creates an `I32` type.
pub fn type_i32() -> Type {
    make_type(TypeKind::I32)
}
/// Creates an `I16` type.
pub fn type_i16() -> Type {
    make_type(TypeKind::I16)
}
/// Creates an `I8` type.
pub fn type_i8() -> Type {
    make_type(TypeKind::I8)
}
/// Creates a `U128` type.
pub fn type_u128() -> Type {
    make_type(TypeKind::U128)
}
/// Creates a `U64` type.
pub fn type_u64() -> Type {
    make_type(TypeKind::U64)
}
/// Creates a `U32` type.
pub fn type_u32() -> Type {
    make_type(TypeKind::U32)
}
/// Creates a `U16` type.
pub fn type_u16() -> Type {
    make_type(TypeKind::U16)
}
/// Creates a `U8` type.
pub fn type_u8() -> Type {
    make_type(TypeKind::U8)
}

/// Creates a `List` type.
pub fn type_list(inner: Type) -> Type {
    make_type(TypeKind::List(Box::new(type_expr_non_null(inner))))
}

/// Creates an `Array` type.
pub fn type_array(inner: Type, size: i128) -> Type {
    make_type(TypeKind::Array(
        Box::new(type_expr_non_null(inner)),
        Box::new(int_literal_expression(size)),
    ))
}

/// Creates a `Map` type.
pub fn type_map(k: Type, v: Type) -> Type {
    make_type(TypeKind::Map(
        Box::new(type_expr_non_null(k)),
        Box::new(type_expr_non_null(v)),
    ))
}

/// Creates a `Set` type.
pub fn type_set(inner: Type) -> Type {
    make_type(TypeKind::Set(Box::new(type_expr_non_null(inner))))
}

/// Creates a `Tuple` type.
pub fn type_tuple(elements: Vec<Type>) -> Type {
    make_type(TypeKind::Tuple(
        elements.into_iter().map(type_expr_non_null).collect(),
    ))
}

/// Creates an optional wrapped type.
pub fn type_option(inner: Type) -> Type {
    make_type(TypeKind::Option(Box::new(inner)))
}

/// Creates an optional wrapped type (legacy alias).
pub fn type_null(inner: Type) -> Type {
    type_option(inner)
}

/// Creates a `Result` type.
pub fn type_result(ok: Type, err: Type) -> Type {
    make_type(TypeKind::Result(
        Box::new(type_expr_non_null(ok)),
        Box::new(type_expr_non_null(err)),
    ))
}

/// Creates a custom type (e.g., struct or class instance).
pub fn type_custom(name: &str, args: Option<Vec<Expression>>) -> Type {
    make_type(TypeKind::Custom(name.to_string(), args))
}

/// Creates a `Future` type.
pub fn type_future(inner: Type) -> Type {
    make_type(TypeKind::Future(Box::new(type_expr_non_null(inner))))
}

/// Creates a function signature type.
pub fn type_function(
    generics: Option<Vec<Expression>>,
    params: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
) -> Type {
    make_type(TypeKind::Function(Box::new(FunctionTypeData {
        generics,
        params,
        return_type,
    })))
}

/// Creates a `Symbol` type.
pub fn type_symbol() -> Type {
    make_type(TypeKind::Symbol)
}

/// Creates a `RawPtr` type (platform-width opaque pointer).
pub fn type_rawptr() -> Type {
    make_type(TypeKind::RawPtr)
}

/// Creates a type declaration expression (e.g., `T extends Number`).
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

/// Creates a type declaration from a string name.
pub fn type_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    kind: TypeDeclarationKind,
    type_expr: Option<Box<Expression>>,
) -> Expression {
    type_declaration_expression(identifier(name), generic_types, kind, type_expr)
}

/// Creates a type alias statement.
pub fn type_statement(declarations: Vec<Expression>, visibility: MemberVisibility) -> Statement {
    stmt(StatementKind::Type(declarations, visibility))
}

/// Creates a break statement.
pub fn break_statement() -> Statement {
    stmt(StatementKind::Break)
}

/// Creates a continue statement.
pub fn continue_statement() -> Statement {
    stmt(StatementKind::Continue)
}

/// Creates an enum declaration statement.
pub fn enum_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    values: Vec<Expression>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Enum(
        Box::new(name),
        generic_types,
        values,
        visibility,
    ))
}

/// Creates an enum value expression (variant).
pub fn enum_value_expression(name: Expression, types: Vec<Expression>) -> Expression {
    expr(ExpressionKind::EnumValue(Box::new(name), types))
}

/// Creates an enum value (variant) from a string name.
pub fn enum_value(name: &str, types: Vec<Expression>) -> Expression {
    enum_value_expression(identifier(name), types)
}

/// Creates a struct declaration statement.
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

/// Creates a struct member expression (name and type pair).
pub fn struct_member_expression(name: Expression, typ: Expression) -> Expression {
    expr(ExpressionKind::StructMember(Box::new(name), Box::new(typ)))
}

/// Creates a struct member from a string name and type expression.
pub fn struct_member(name: &str, typ: Expression) -> Expression {
    struct_member_expression(identifier(name), typ)
}

/// Creates a class declaration statement.
pub fn class_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    class_statement_with_abstract(
        name,
        generic_types,
        base_class,
        traits,
        body,
        visibility,
        false,
    )
}

/// Creates a class declaration statement with abstract flag.
pub fn class_statement_with_abstract(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
    is_abstract: bool,
) -> Statement {
    stmt(StatementKind::Class(Box::new(ClassData {
        name: Box::new(name),
        generics: generic_types,
        base_class,
        traits,
        body,
        visibility,
        is_abstract,
    })))
}

/// Creates an abstract class declaration statement.
pub fn abstract_class_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    class_statement_with_abstract(
        name,
        generic_types,
        base_class,
        traits,
        body,
        visibility,
        true,
    )
}

/// Creates a class declaration from string name.
pub fn class_decl(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<&str>,
    traits: Vec<&str>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    class_statement(
        identifier(name),
        generic_types,
        base_class.map(|s| Box::new(identifier(s))),
        traits.into_iter().map(identifier).collect(),
        body,
        visibility,
    )
}

/// Creates a trait declaration statement.
pub fn trait_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    parent_traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Trait(
        Box::new(name),
        generic_types,
        parent_traits,
        body,
        visibility,
    ))
}

/// Creates a trait declaration from string name.
pub fn trait_decl(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parent_traits: Vec<&str>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    trait_statement(
        identifier(name),
        generic_types,
        parent_traits.into_iter().map(identifier).collect(),
        body,
        visibility,
    )
}

/// Creates a super expression for calling parent class methods.
pub fn super_expression() -> Expression {
    expr(ExpressionKind::Super)
}

/// Creates a super expression with a specific span.
pub fn super_expression_with_span(span: Span) -> Expression {
    expr_with_span(ExpressionKind::Super, span)
}

/// Creates a list literal expression.
pub fn list(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::List(elements))
}

/// Creates a fixed-size array literal expression.
pub fn array(elements: Vec<Expression>, size: Box<Expression>) -> Expression {
    expr(ExpressionKind::Array(elements, size))
}

/// Creates a map literal expression.
pub fn map(pairs: Vec<(Expression, Expression)>) -> Expression {
    expr(ExpressionKind::Map(pairs))
}

/// Creates a tuple literal expression.
pub fn tuple(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Tuple(elements))
}

/// Creates a set literal expression.
pub fn set(elements: Vec<Expression>) -> Expression {
    expr(ExpressionKind::Set(elements))
}

/// Creates a match expression.
pub fn match_expression(subject: Expression, branches: Vec<MatchBranch>) -> Expression {
    expr(ExpressionKind::Match(Box::new(subject), branches))
}

/// Represents a function builder, used to create functions with a more readable syntax.
pub struct FunctionBuilder {
    name: String,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    properties: FunctionProperties,
}

impl FunctionBuilder {
    /// Creates a new function builder with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            generic_types: None,
            parameters: vec![],
            return_type: None,
            properties: FunctionProperties {
                is_async: false,
                is_parallel: false,
                is_gpu: false,

                visibility: MemberVisibility::Public,
            },
        }
    }

    /// Sets the generic type parameters.
    pub fn generics(mut self, generics: Vec<Expression>) -> Self {
        self.generic_types = Some(generics);
        self
    }

    /// Sets the function parameters.
    pub fn params(mut self, params: Vec<Parameter>) -> Self {
        self.parameters = params;
        self
    }

    /// Sets the function properties (async, parallel, gpu, visibility).
    pub fn properties(mut self, properties: FunctionProperties) -> Self {
        self.properties = properties;
        self
    }

    /// Sets the return type.
    pub fn return_type(mut self, ret_type: Expression) -> Self {
        self.return_type = Some(Box::new(ret_type));
        self
    }

    /// Marks the function as async.
    pub fn set_async(mut self) -> Self {
        self.properties.is_async = true;
        self
    }

    /// Marks the function as parallel.
    pub fn set_parallel(mut self) -> Self {
        self.properties.is_parallel = true;
        self
    }

    /// Marks the function as a GPU kernel.
    pub fn set_gpu(mut self) -> Self {
        self.properties.is_gpu = true;
        self
    }

    /// Sets visibility to private.
    pub fn set_private(mut self) -> Self {
        self.properties.visibility = MemberVisibility::Private;
        self
    }

    /// Sets visibility to protected.
    pub fn set_protected(mut self) -> Self {
        self.properties.visibility = MemberVisibility::Protected;
        self
    }

    /// Builds a function declaration statement with the given body.
    pub fn build(self, body: Statement) -> Statement {
        stmt(StatementKind::FunctionDeclaration(Box::new(
            FunctionDeclarationData {
                name: self.name,
                generics: self.generic_types,
                params: self.parameters,
                return_type: self.return_type,
                body: Some(Box::new(body)),
                properties: self.properties,
            },
        )))
    }

    /// Builds an abstract function declaration (no body).
    pub fn build_abstract(self) -> Statement {
        stmt(StatementKind::FunctionDeclaration(Box::new(
            FunctionDeclarationData {
                name: self.name,
                generics: self.generic_types,
                params: self.parameters,
                return_type: self.return_type,
                body: None,
                properties: self.properties,
            },
        )))
    }

    /// Builds a function declaration with an empty body.
    pub fn build_empty_body(self) -> Statement {
        self.build(empty_statement())
    }

    /// Builds a lambda expression with the given body.
    pub fn build_lambda(self, body: Statement) -> Expression {
        expr(ExpressionKind::Lambda(Box::new(LambdaData {
            generics: self.generic_types,
            params: self.parameters,
            return_type: self.return_type,
            body: Box::new(body),
            properties: self.properties,
        })))
    }

    /// Builds a lambda expression with an empty body.
    pub fn build_lambda_empty_body(self) -> Expression {
        self.build_lambda(empty_statement())
    }
}

/// Creates a function builder.
pub fn func(name: &str) -> FunctionBuilder {
    FunctionBuilder::new(name)
}

/// Creates a function declaration statement.
pub fn function_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    body: Statement,
    properties: FunctionProperties,
) -> Statement {
    stmt(StatementKind::FunctionDeclaration(Box::new(
        FunctionDeclarationData {
            name: name.into(),
            generics: generic_types,
            params: parameters,
            return_type,
            body: Some(Box::new(body)),
            properties,
        },
    )))
}

/// Creates an abstract function declaration (no body).
pub fn abstract_function_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    properties: FunctionProperties,
) -> Statement {
    stmt(StatementKind::FunctionDeclaration(Box::new(
        FunctionDeclarationData {
            name: name.into(),
            generics: generic_types,
            params: parameters,
            return_type,
            body: None,
            properties,
        },
    )))
}

/// Creates a runtime function declaration (extern binding to a runtime library).
pub fn runtime_function_declaration(
    runtime: common::RuntimeKind,
    name: &str,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
) -> Statement {
    stmt(StatementKind::RuntimeFunctionDeclaration(
        runtime,
        name.into(),
        parameters,
        return_type,
    ))
}

/// Creates a lambda function builder.
pub fn lambda() -> FunctionBuilder {
    FunctionBuilder::new("")
}

/// Creates a lambda function expression.
pub fn lambda_expression(
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    body: Statement,
    properties: FunctionProperties,
) -> Expression {
    expr(ExpressionKind::Lambda(Box::new(LambdaData {
        generics: generic_types,
        params: parameters,
        return_type,
        body: Box::new(body),
        properties,
    })))
}

/// Creates a named argument expression.
pub fn named_argument(name: String, value: Expression) -> Expression {
    expr(ExpressionKind::NamedArgument(name, Box::new(value)))
}

/// Creates a named argument expression with a span.
pub fn named_argument_with_span(name: String, value: Expression, span: Span) -> Expression {
    expr_with_span(ExpressionKind::NamedArgument(name, Box::new(value)), span)
}

/// Creates a type from an expression.
pub fn type_from_expr(expr: Expression) -> Type {
    match expr.node {
        ExpressionKind::Type(t, _) => *t,
        _ => make_type(TypeKind::Error),
    }
}
