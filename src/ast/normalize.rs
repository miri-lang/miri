// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Post-parse normalisation: collection TypeKind variants → TypeKind::Custom.
//!
//! The parser produces `TypeKind::List(T)`, `TypeKind::Array(T, N)`,
//! `TypeKind::Map(K, V)`, and `TypeKind::Set(T)` for conciseness.  This
//! one-shot pass, run immediately after parsing and before type checking,
//! converts all four into `TypeKind::Custom("List", [T])` etc.
//!
//! After this pass the compiler sees only `TypeKind::Custom` for builtin
//! collections everywhere; no downstream phase needs to match on the four
//! canonical variants.

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind, LambdaData, LeftHandSideExpression};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{ClassData, FunctionDeclarationData, Statement, StatementKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::Program;

/// Normalise all collection `TypeKind` variants inside `program` to
/// `TypeKind::Custom`.  Call this once, immediately after parsing.
pub fn normalize(program: &mut Program) {
    for stmt in &mut program.body {
        normalize_stmt(stmt);
    }
}

fn normalize_stmt(stmt: &mut Statement) {
    match &mut stmt.node {
        StatementKind::Empty | StatementKind::Break | StatementKind::Continue => {}
        StatementKind::Block(stmts) => normalize_block(stmts),
        StatementKind::Expression(expr) => normalize_expr(expr),
        StatementKind::Return(expr) => normalize_optional_expr(expr.as_deref_mut()),
        StatementKind::Variable(decls, _) => normalize_variable_decls(decls),
        StatementKind::If(cond, then_branch, else_branch, _) => {
            normalize_if_stmt(cond, then_branch, else_branch.as_deref_mut())
        }
        StatementKind::While(cond, body, _) => {
            normalize_expr(cond);
            normalize_stmt(body);
        }
        StatementKind::For(decls, iterable, body)
        | StatementKind::GpuFrame(decls, iterable, body) => {
            normalize_for_stmt(decls, iterable, body)
        }
        StatementKind::Forall {
            vars,
            iterable,
            body,
            ..
        } => normalize_for_stmt(vars, iterable, body),
        StatementKind::GpuFrameBlock(block) => normalize_stmt(block),
        StatementKind::FunctionDeclaration(decl) => normalize_function_decl(decl),
        StatementKind::RuntimeFunctionDeclaration(_, _, params, ret) => {
            normalize_params(params);
            normalize_optional_expr(ret.as_deref_mut());
        }
        StatementKind::IntrinsicFunctionDeclaration(_, generics, params, ret, _) => {
            normalize_intrinsic_function_decl(generics, params, ret.as_deref_mut())
        }
        StatementKind::Use(path, alias) => {
            normalize_expr(path);
            normalize_optional_expr(alias.as_deref_mut());
        }
        StatementKind::Type(exprs, _) => normalize_expr_list(exprs),
        StatementKind::Enum(name, generics, variants, methods, _, _) => {
            normalize_nominal_type_decl(name, generics, variants, methods)
        }
        StatementKind::Struct(name, generics, fields, methods, _) => {
            normalize_nominal_type_decl(name, generics, fields, methods)
        }
        StatementKind::Class(class_data) => normalize_class_stmt(class_data),
        StatementKind::Trait(name, generics, parent_traits, body, _) => {
            normalize_nominal_type_decl(name, generics, parent_traits, body)
        }
    }
}

fn normalize_if_stmt(
    cond: &mut Expression,
    then_branch: &mut Statement,
    else_branch: Option<&mut Statement>,
) {
    normalize_expr(cond);
    normalize_stmt(then_branch);
    if let Some(e) = else_branch {
        normalize_stmt(e);
    }
}

fn normalize_intrinsic_function_decl(
    generics: &mut Option<Vec<Expression>>,
    params: &mut [Parameter],
    ret: Option<&mut Expression>,
) {
    normalize_optional_generics(generics);
    normalize_params(params);
    normalize_optional_expr(ret);
}

fn normalize_nominal_type_decl(
    name: &mut Expression,
    generics: &mut Option<Vec<Expression>>,
    exprs: &mut [Expression],
    body: &mut [Statement],
) {
    normalize_expr(name);
    normalize_optional_generics(generics);
    normalize_expr_list(exprs);
    normalize_stmt_list(body);
}

fn normalize_block(stmts: &mut [Statement]) {
    for s in stmts.iter_mut() {
        normalize_stmt(s);
    }
}

fn normalize_stmt_list(stmts: &mut [Statement]) {
    for s in stmts.iter_mut() {
        normalize_stmt(s);
    }
}

fn normalize_expr_list(exprs: &mut [Expression]) {
    for e in exprs.iter_mut() {
        normalize_expr(e);
    }
}

fn normalize_optional_expr(expr: Option<&mut Expression>) {
    if let Some(e) = expr {
        normalize_expr(e);
    }
}

fn normalize_optional_generics(generics: &mut Option<Vec<Expression>>) {
    if let Some(gens) = generics {
        normalize_expr_list(gens);
    }
}

fn normalize_variable_decls(decls: &mut [crate::ast::statement::VariableDeclaration]) {
    for decl in decls.iter_mut() {
        normalize_optional_expr(decl.typ.as_deref_mut());
        normalize_optional_expr(decl.initializer.as_deref_mut());
    }
}

fn normalize_for_stmt(
    decls: &mut [crate::ast::statement::VariableDeclaration],
    iterable: &mut Expression,
    body: &mut Statement,
) {
    for decl in decls.iter_mut() {
        normalize_optional_expr(decl.typ.as_deref_mut());
    }
    normalize_expr(iterable);
    normalize_stmt(body);
}

fn normalize_function_decl(decl: &mut FunctionDeclarationData) {
    normalize_params(&mut decl.params);
    normalize_optional_expr(decl.return_type.as_deref_mut());
    if let Some(body) = &mut decl.body {
        normalize_stmt(body);
    }
}

fn normalize_class_stmt(class_data: &mut ClassData) {
    normalize_expr(&mut class_data.name);
    normalize_optional_generics(&mut class_data.generics);
    if let Some(base) = &mut class_data.base_class {
        normalize_expr(base);
    }
    normalize_expr_list(&mut class_data.traits);
    normalize_stmt_list(&mut class_data.body);
}

fn normalize_params(params: &mut [Parameter]) {
    for param in params.iter_mut() {
        normalize_expr(&mut param.typ);
        if let Some(guard) = &mut param.guard {
            normalize_expr(guard);
        }
        if let Some(default) = &mut param.default_value {
            normalize_expr(default);
        }
    }
}

fn normalize_expr(expr: &mut Expression) {
    match &mut expr.node {
        ExpressionKind::Type(ty, _) => normalize_type(ty),
        ExpressionKind::Binary(lhs, _, rhs) | ExpressionKind::Logical(lhs, _, rhs) => {
            normalize_expr_pair(lhs, rhs)
        }
        ExpressionKind::Unary(_, operand) | ExpressionKind::Guard(_, operand) => {
            normalize_expr(operand)
        }
        ExpressionKind::Assignment(lhs, _, rhs) => {
            normalize_lhs(lhs);
            normalize_expr(rhs);
        }
        ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
            normalize_conditional_expr(cond, then_expr, else_expr.as_deref_mut())
        }
        ExpressionKind::Range(start, end, _) => {
            normalize_expr(start);
            normalize_optional_expr(end.as_deref_mut());
        }
        ExpressionKind::Member(obj, prop) | ExpressionKind::Index(obj, prop) => {
            normalize_expr_pair(obj, prop)
        }
        ExpressionKind::Call(callee, args) | ExpressionKind::EnumValue(callee, args) => {
            normalize_expr(callee);
            normalize_expr_list(args);
        }
        ExpressionKind::ImportPath(parts, _) => normalize_expr_list(parts),
        ExpressionKind::GenericType(name, constraint, _) => {
            normalize_expr(name);
            normalize_optional_expr(constraint.as_deref_mut());
        }
        ExpressionKind::TypeDeclaration(name, generics, _, constraint) => {
            normalize_type_declaration_expr(name, generics, constraint.as_deref_mut())
        }
        ExpressionKind::StructMember(name, ty) => normalize_expr_pair(name, ty),
        ExpressionKind::Lambda(lambda_data) => normalize_lambda(lambda_data),
        ExpressionKind::List(elems)
        | ExpressionKind::Tuple(elems)
        | ExpressionKind::Set(elems)
        | ExpressionKind::FormattedString(elems) => normalize_expr_list(elems),
        ExpressionKind::Array(elems, size) => {
            normalize_expr_list(elems);
            normalize_expr(size);
        }
        ExpressionKind::Map(entries) => normalize_map_entries(entries),
        ExpressionKind::Match(scrutinee, branches) => normalize_match(scrutinee, branches),
        ExpressionKind::NamedArgument(_, value) => normalize_expr(value),
        ExpressionKind::Block(stmts, final_expr) => {
            normalize_stmt_list(stmts);
            normalize_expr(final_expr);
        }
        ExpressionKind::Cast(value_expr, target_type_expr) => {
            normalize_expr_pair(value_expr, target_type_expr)
        }
        ExpressionKind::Literal(_) | ExpressionKind::Identifier(_, _) | ExpressionKind::Super => {}
    }
}

fn normalize_expr_pair(a: &mut Expression, b: &mut Expression) {
    normalize_expr(a);
    normalize_expr(b);
}

fn normalize_conditional_expr(
    cond: &mut Expression,
    then_expr: &mut Expression,
    else_expr: Option<&mut Expression>,
) {
    normalize_expr(cond);
    normalize_expr(then_expr);
    normalize_optional_expr(else_expr);
}

fn normalize_type_declaration_expr(
    name: &mut Expression,
    generics: &mut Option<Vec<Expression>>,
    constraint: Option<&mut Expression>,
) {
    normalize_expr(name);
    normalize_optional_generics(generics);
    normalize_optional_expr(constraint);
}

fn normalize_map_entries(entries: &mut [(Expression, Expression)]) {
    for (k, v) in entries.iter_mut() {
        normalize_expr_pair(k, v);
    }
}

fn normalize_lhs(lhs: &mut LeftHandSideExpression) {
    match lhs {
        LeftHandSideExpression::Identifier(e)
        | LeftHandSideExpression::Member(e)
        | LeftHandSideExpression::Index(e) => normalize_expr(e),
    }
}

fn normalize_lambda(lambda_data: &mut LambdaData) {
    normalize_params(&mut lambda_data.params);
    normalize_optional_expr(lambda_data.return_type.as_deref_mut());
    normalize_stmt(&mut lambda_data.body);
}

fn normalize_match(scrutinee: &mut Expression, branches: &mut [MatchBranch]) {
    normalize_expr(scrutinee);
    for branch in branches.iter_mut() {
        normalize_stmt(&mut branch.body);
        if let Some(guard) = &mut branch.guard {
            normalize_expr(guard);
        }
    }
}

/// Normalize a `Type` in-place: convert `List/Array/Map/Set` to `Custom`.
///
/// The conversion is recursive so that nested types like `List<List<int>>` are
/// fully normalized: `Custom("List", [Custom("List", [int])])`.
pub fn normalize_type(ty: &mut Type) {
    normalize_type_children(&mut ty.kind);
    ty.kind = canonicalize_collection(std::mem::replace(&mut ty.kind, TypeKind::Error));
}

fn normalize_type_children(kind: &mut TypeKind) {
    match kind {
        TypeKind::List(inner) => normalize_expr(inner),
        TypeKind::Array(inner, size) => {
            normalize_expr(inner);
            normalize_expr(size);
        }
        TypeKind::Map(k, v) => {
            normalize_expr(k);
            normalize_expr(v);
        }
        TypeKind::Set(inner) => normalize_expr(inner),
        TypeKind::Custom(_, Some(args)) => normalize_expr_list(args),
        TypeKind::Custom(_, None) => {}
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            normalize_type(inner);
        }
        TypeKind::Result(ok, err) => {
            normalize_expr(ok);
            normalize_expr(err);
        }
        TypeKind::Future(inner) => normalize_expr(inner),
        TypeKind::Tuple(elems) => normalize_expr_list(elems),
        TypeKind::Generic(_, Some(c), _) => normalize_type(c),
        TypeKind::Generic(_, None, _) => {}
        TypeKind::Function(func_data) => {
            normalize_params(&mut func_data.params);
            normalize_optional_expr(func_data.return_type.as_deref_mut());
        }
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => {}
    }
}

fn canonicalize_collection(kind: TypeKind) -> TypeKind {
    match kind {
        TypeKind::List(inner) => TypeKind::Custom(
            BuiltinCollectionKind::List.name().to_string(),
            Some(vec![*inner]),
        ),
        TypeKind::Array(inner, size) => TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![*inner, *size]),
        ),
        TypeKind::Map(k, v) => TypeKind::Custom(
            BuiltinCollectionKind::Map.name().to_string(),
            Some(vec![*k, *v]),
        ),
        TypeKind::Set(inner) => TypeKind::Custom(
            BuiltinCollectionKind::Set.name().to_string(),
            Some(vec![*inner]),
        ),
        kind @ (TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_)
        | TypeKind::Void
        | TypeKind::Error) => kind,
    }
}
