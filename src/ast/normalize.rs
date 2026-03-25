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
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
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

        StatementKind::Block(stmts) => {
            for s in stmts.iter_mut() {
                normalize_stmt(s);
            }
        }

        StatementKind::Expression(expr) => normalize_expr(expr),

        StatementKind::Return(expr) => {
            if let Some(e) = expr {
                normalize_expr(e);
            }
        }

        StatementKind::Variable(decls, _) => {
            for decl in decls.iter_mut() {
                if let Some(ty) = &mut decl.typ {
                    normalize_expr(ty);
                }
                if let Some(init) = &mut decl.initializer {
                    normalize_expr(init);
                }
            }
        }

        StatementKind::If(cond, then_branch, else_branch, _) => {
            normalize_expr(cond);
            normalize_stmt(then_branch);
            if let Some(e) = else_branch {
                normalize_stmt(e);
            }
        }

        StatementKind::While(cond, body, _) => {
            normalize_expr(cond);
            normalize_stmt(body);
        }

        StatementKind::For(decls, iterable, body) => {
            for decl in decls.iter_mut() {
                if let Some(ty) = &mut decl.typ {
                    normalize_expr(ty);
                }
            }
            normalize_expr(iterable);
            normalize_stmt(body);
        }

        StatementKind::FunctionDeclaration(decl) => {
            normalize_params(&mut decl.params);
            if let Some(ret) = &mut decl.return_type {
                normalize_expr(ret);
            }
            if let Some(body) = &mut decl.body {
                normalize_stmt(body);
            }
        }

        StatementKind::RuntimeFunctionDeclaration(_, _, params, ret) => {
            normalize_params(params);
            if let Some(r) = ret {
                normalize_expr(r);
            }
        }

        StatementKind::Use(path, alias) => {
            normalize_expr(path);
            if let Some(a) = alias {
                normalize_expr(a);
            }
        }

        StatementKind::Type(exprs, _) => {
            for e in exprs.iter_mut() {
                normalize_expr(e);
            }
        }

        StatementKind::Enum(name, generics, variants, _) => {
            normalize_expr(name);
            if let Some(gens) = generics {
                for g in gens.iter_mut() {
                    normalize_expr(g);
                }
            }
            for v in variants.iter_mut() {
                normalize_expr(v);
            }
        }

        StatementKind::Struct(name, generics, fields, _) => {
            normalize_expr(name);
            if let Some(gens) = generics {
                for g in gens.iter_mut() {
                    normalize_expr(g);
                }
            }
            for f in fields.iter_mut() {
                normalize_expr(f);
            }
        }

        StatementKind::Class(class_data) => {
            normalize_expr(&mut class_data.name);
            if let Some(gens) = &mut class_data.generics {
                for g in gens.iter_mut() {
                    normalize_expr(g);
                }
            }
            if let Some(base) = &mut class_data.base_class {
                normalize_expr(base);
            }
            for t in class_data.traits.iter_mut() {
                normalize_expr(t);
            }
            for body_stmt in class_data.body.iter_mut() {
                normalize_stmt(body_stmt);
            }
        }

        StatementKind::Trait(name, generics, parent_traits, body, _) => {
            normalize_expr(name);
            if let Some(gens) = generics {
                for g in gens.iter_mut() {
                    normalize_expr(g);
                }
            }
            for pt in parent_traits.iter_mut() {
                normalize_expr(pt);
            }
            for body_stmt in body.iter_mut() {
                normalize_stmt(body_stmt);
            }
        }
    }
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
            normalize_expr(lhs);
            normalize_expr(rhs);
        }

        ExpressionKind::Unary(_, operand) => normalize_expr(operand),

        ExpressionKind::Assignment(lhs, _, rhs) => {
            match lhs.as_mut() {
                crate::ast::expression::LeftHandSideExpression::Identifier(e)
                | crate::ast::expression::LeftHandSideExpression::Member(e)
                | crate::ast::expression::LeftHandSideExpression::Index(e) => normalize_expr(e),
            }
            normalize_expr(rhs);
        }

        ExpressionKind::Conditional(cond, then_expr, else_expr, _) => {
            normalize_expr(cond);
            normalize_expr(then_expr);
            if let Some(e) = else_expr {
                normalize_expr(e);
            }
        }

        ExpressionKind::Range(start, end, _) => {
            normalize_expr(start);
            if let Some(e) = end {
                normalize_expr(e);
            }
        }

        ExpressionKind::Guard(_, operand) => normalize_expr(operand),

        ExpressionKind::Member(obj, prop) => {
            normalize_expr(obj);
            normalize_expr(prop);
        }

        ExpressionKind::Index(obj, idx) => {
            normalize_expr(obj);
            normalize_expr(idx);
        }

        ExpressionKind::Call(callee, args) => {
            normalize_expr(callee);
            for arg in args.iter_mut() {
                normalize_expr(arg);
            }
        }

        ExpressionKind::ImportPath(parts, _) => {
            for p in parts.iter_mut() {
                normalize_expr(p);
            }
        }

        ExpressionKind::GenericType(name, constraint, _) => {
            normalize_expr(name);
            if let Some(c) = constraint {
                normalize_expr(c);
            }
        }

        ExpressionKind::TypeDeclaration(name, generics, _, constraint) => {
            normalize_expr(name);
            if let Some(gens) = generics {
                for g in gens.iter_mut() {
                    normalize_expr(g);
                }
            }
            if let Some(c) = constraint {
                normalize_expr(c);
            }
        }

        ExpressionKind::EnumValue(variant, args) => {
            normalize_expr(variant);
            for a in args.iter_mut() {
                normalize_expr(a);
            }
        }

        ExpressionKind::StructMember(name, ty) => {
            normalize_expr(name);
            normalize_expr(ty);
        }

        ExpressionKind::Lambda(lambda_data) => {
            normalize_params(&mut lambda_data.params);
            if let Some(ret) = &mut lambda_data.return_type {
                normalize_expr(ret);
            }
            normalize_stmt(&mut lambda_data.body);
        }

        ExpressionKind::List(elems) => {
            for e in elems.iter_mut() {
                normalize_expr(e);
            }
        }

        ExpressionKind::Array(elems, size) => {
            for e in elems.iter_mut() {
                normalize_expr(e);
            }
            normalize_expr(size);
        }

        ExpressionKind::Map(entries) => {
            for (k, v) in entries.iter_mut() {
                normalize_expr(k);
                normalize_expr(v);
            }
        }

        ExpressionKind::Tuple(elems) | ExpressionKind::Set(elems) => {
            for e in elems.iter_mut() {
                normalize_expr(e);
            }
        }

        ExpressionKind::Match(scrutinee, branches) => {
            normalize_expr(scrutinee);
            for branch in branches.iter_mut() {
                normalize_stmt(&mut branch.body);
                if let Some(guard) = &mut branch.guard {
                    normalize_expr(guard);
                }
            }
        }

        ExpressionKind::FormattedString(parts) => {
            for p in parts.iter_mut() {
                normalize_expr(p);
            }
        }

        ExpressionKind::NamedArgument(_, value) => normalize_expr(value),

        ExpressionKind::Block(stmts, final_expr) => {
            for s in stmts.iter_mut() {
                normalize_stmt(s);
            }
            normalize_expr(final_expr);
        }

        // Leaves — nothing to normalize
        ExpressionKind::Literal(_) | ExpressionKind::Identifier(_, _) | ExpressionKind::Super => {}
    }
}

/// Normalize a `Type` in-place: convert `List/Array/Map/Set` to `Custom`.
///
/// The conversion is recursive so that nested types like `List<List<int>>` are
/// fully normalized: `Custom("List", [Custom("List", [int])])`.
fn normalize_type(ty: &mut Type) {
    // Recurse first, then replace the outer kind.
    match &mut ty.kind {
        TypeKind::List(inner) => {
            normalize_expr(inner);
        }
        TypeKind::Array(inner, size) => {
            normalize_expr(inner);
            normalize_expr(size);
        }
        TypeKind::Map(k, v) => {
            normalize_expr(k);
            normalize_expr(v);
        }
        TypeKind::Set(inner) => {
            normalize_expr(inner);
        }
        TypeKind::Custom(_, Some(args)) => {
            for a in args.iter_mut() {
                normalize_expr(a);
            }
        }
        TypeKind::Custom(_, None) => {}
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            normalize_type(inner);
        }
        TypeKind::Result(ok, err) => {
            normalize_expr(ok);
            normalize_expr(err);
        }
        TypeKind::Future(inner) => {
            normalize_expr(inner);
        }
        TypeKind::Tuple(elems) => {
            for e in elems.iter_mut() {
                normalize_expr(e);
            }
        }
        TypeKind::Generic(_, Some(c), _) => {
            normalize_type(c);
        }
        TypeKind::Generic(_, None, _) => {}
        TypeKind::Function(func_data) => {
            normalize_params(&mut func_data.params);
            if let Some(ret) = &mut func_data.return_type {
                normalize_expr(ret);
            }
        }
        // Primitives and leaf types — nothing inside
        _ => {}
    }

    // Now replace the kind itself if it is a collection canonical variant.
    let new_kind = match std::mem::replace(&mut ty.kind, TypeKind::Error) {
        TypeKind::List(inner) => TypeKind::Custom("List".to_string(), Some(vec![*inner])),
        TypeKind::Array(inner, size) => {
            TypeKind::Custom("Array".to_string(), Some(vec![*inner, *size]))
        }
        TypeKind::Map(k, v) => TypeKind::Custom("Map".to_string(), Some(vec![*k, *v])),
        TypeKind::Set(inner) => TypeKind::Custom("Set".to_string(), Some(vec![*inner])),
        other => other,
    };
    ty.kind = new_kind;
}
