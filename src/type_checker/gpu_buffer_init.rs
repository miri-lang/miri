// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Collection of GPU buffer-initializer metadata during semantic analysis.
//!
//! A `gpu let`/`gpu var` bound to a compile-time constant array/list literal
//! (or a sized `Array<T, N>()` constructor) carries the buffer's initial host
//! data. The type checker records that metadata as it finishes checking a
//! program so the web-gpu bundle emitter consumes a resolved table instead of
//! re-walking the AST in the pipeline orchestrator.

use std::collections::HashMap;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::operator::BinaryOp;
use crate::ast::statement::{BindingResidency, Statement, StatementKind};
use crate::ast::types::{wgsl_scalar_name, BuiltinCollectionKind, TypeKind};
use crate::ast::Program;

use super::TypeChecker;

/// Initial data for a GPU buffer from a compile-time constant initializer.
#[derive(Debug, Clone)]
pub struct GpuBufferInit {
    /// WGSL scalar element name (e.g. `i32`, `f32`).
    pub elem_type: String,
    /// Constant element values; empty for sized zero-initialized buffers.
    pub values: Vec<f64>,
    /// Explicit length for sized allocations; `None` infers from `values.len()`.
    pub length: Option<usize>,
}

impl TypeChecker {
    /// Records buffer-init metadata for every `gpu` binding whose initializer is
    /// a compile-time constant array/list literal or a sized `Array<T, N>()`
    /// constructor. Called at the end of [`TypeChecker::check`].
    pub(crate) fn collect_gpu_buffer_initializers(&mut self, program: &Program) {
        for statement in &program.body {
            collect_from_statement(statement, &mut self.gpu_buffer_inits);
        }
    }
}

fn collect_from_statement(stmt: &Statement, inits: &mut HashMap<String, GpuBufferInit>) {
    match &stmt.node {
        StatementKind::Variable(decls, _) => {
            for decl in decls {
                if decl.residency != BindingResidency::Gpu {
                    continue;
                }
                let Some(init) = &decl.initializer else {
                    continue;
                };
                if let Some(values) = extract_const_array_values(init) {
                    inits.insert(
                        decl.name.clone(),
                        GpuBufferInit {
                            elem_type: infer_elem_type(init),
                            values,
                            length: extract_array_size(init),
                        },
                    );
                }
            }
        }
        StatementKind::Block(stmts) => {
            for s in stmts {
                collect_from_statement(s, inits);
            }
        }
        StatementKind::If(_, then_branch, else_branch, _) => {
            collect_from_statement(then_branch, inits);
            if let Some(e) = else_branch {
                collect_from_statement(e, inits);
            }
        }
        StatementKind::While(_, body, _) | StatementKind::For(_, _, body) => {
            collect_from_statement(body, inits);
        }
        StatementKind::Forall { body, .. } => {
            collect_from_statement(body, inits);
        }
        StatementKind::FunctionDeclaration(decl) => {
            if let Some(body) = &decl.body {
                collect_from_statement(body, inits);
            }
        }
        _ => {}
    }
}

fn extract_const_array_values(expr: &Expression) -> Option<Vec<f64>> {
    match &expr.node {
        ExpressionKind::Array(elements, _) | ExpressionKind::List(elements) => {
            elements.iter().map(extract_numeric_literal).collect()
        }
        // A sized `Array<T, N>()` constructor zero-fills: no element values, the
        // length comes from the type generic `N` (see `extract_array_size`).
        ExpressionKind::Call(func_expr, args)
            if args.is_empty() && is_array_constructor(func_expr) =>
        {
            Some(Vec::new())
        }
        _ => None,
    }
}

fn is_array_constructor(expr: &Expression) -> bool {
    if let ExpressionKind::TypeDeclaration(name_expr, Some(generics), _, _) = &expr.node {
        if let ExpressionKind::Identifier(name, _) = &name_expr.node {
            // `Array<T, N>` carries exactly two generic arguments.
            return BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
                && generics.len() == 2;
        }
    }
    false
}

fn extract_numeric_literal(expr: &Expression) -> Option<f64> {
    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(int_lit)) => Some(integer_literal_as_f64(int_lit)),
        ExpressionKind::Literal(Literal::Float(float_lit)) => Some(match float_lit {
            FloatLiteral::F32(v) => f32::from_bits(*v) as f64,
            FloatLiteral::F64(v) => f64::from_bits(*v),
        }),
        _ => None,
    }
}

fn integer_literal_as_f64(int_lit: &IntegerLiteral) -> f64 {
    match int_lit {
        IntegerLiteral::I8(v) => *v as f64,
        IntegerLiteral::I16(v) => *v as f64,
        IntegerLiteral::I32(v) => *v as f64,
        IntegerLiteral::I64(v) => *v as f64,
        IntegerLiteral::I128(v) => *v as f64,
        IntegerLiteral::U8(v) => *v as f64,
        IntegerLiteral::U16(v) => *v as f64,
        IntegerLiteral::U32(v) => *v as f64,
        IntegerLiteral::U64(v) => *v as f64,
        IntegerLiteral::U128(v) => *v as f64,
    }
}

fn infer_elem_type(expr: &Expression) -> String {
    match &expr.node {
        ExpressionKind::Array(elements, _) | ExpressionKind::List(elements) => elements
            .first()
            .map(infer_elem_type_from_literal)
            .unwrap_or_else(|| "i32".to_string()),
        ExpressionKind::Call(func_expr, _) if is_array_constructor(func_expr) => {
            infer_sized_array_elem_type(func_expr)
        }
        _ => "i32".to_string(),
    }
}

/// Extracts the WGSL element type from the first generic of a sized
/// `Array<T, N>()` constructor's type declaration.
fn infer_sized_array_elem_type(func_expr: &Expression) -> String {
    let ExpressionKind::TypeDeclaration(_base, Some(generics), _, _) = &func_expr.node else {
        return "i32".to_string();
    };
    let Some(elem_type_expr) = generics.first() else {
        return "i32".to_string();
    };
    match &elem_type_expr.node {
        ExpressionKind::Identifier(type_name, _) => scalar_name_from_identifier(type_name),
        // The type checker rewrites a resolved generic into a `Type` node.
        ExpressionKind::Type(inner_ty, _) => infer_elem_type_from_type(&inner_ty.kind),
        _ => "i32".to_string(),
    }
}

fn scalar_name_from_identifier(type_name: &str) -> String {
    let kind = match type_name {
        "int" => Some(TypeKind::Int),
        "i8" => Some(TypeKind::I8),
        "i16" => Some(TypeKind::I16),
        "i32" => Some(TypeKind::I32),
        "i64" => Some(TypeKind::I64),
        "u8" => Some(TypeKind::U8),
        "u16" => Some(TypeKind::U16),
        "u32" => Some(TypeKind::U32),
        "u64" => Some(TypeKind::U64),
        "f16" => Some(TypeKind::F16),
        "f32" => Some(TypeKind::F32),
        "float" => Some(TypeKind::Float),
        "f64" => Some(TypeKind::F64),
        _ => None,
    };
    kind.and_then(|k| wgsl_scalar_name(&k))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "i32".to_string())
}

fn infer_elem_type_from_type(kind: &TypeKind) -> String {
    wgsl_scalar_name(kind).unwrap_or("i32").to_string()
}

fn infer_elem_type_from_literal(elem: &Expression) -> String {
    match &elem.node {
        ExpressionKind::Literal(Literal::Float(float_lit)) => match float_lit {
            FloatLiteral::F32(_) => "f32".to_string(),
            FloatLiteral::F64(_) => "f64".to_string(),
        },
        // Integer literals are `int` (Miri default), which maps to i32 for
        // browser portability. The host keeps i64; marshalling narrows to i32
        // for the device and widens on readback.
        ExpressionKind::Literal(Literal::Integer(_)) => "i32".to_string(),
        _ => "i32".to_string(),
    }
}

fn extract_array_size(expr: &Expression) -> Option<usize> {
    if let ExpressionKind::Call(func_expr, _) = &expr.node {
        if let ExpressionKind::TypeDeclaration(_, Some(generics), _, _) = &func_expr.node {
            if generics.len() >= 2 {
                // The size is the second generic argument.
                return try_eval_const_size(&generics[1]);
            }
        }
    }
    None
}

/// Evaluates a simple constant size expression (a non-negative integer literal
/// or integer arithmetic over such literals).
fn try_eval_const_size(expr: &Expression) -> Option<usize> {
    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(int_lit)) => {
            let val = integer_literal_as_i128(int_lit);
            (val >= 0).then_some(val as usize)
        }
        ExpressionKind::Binary(left, op, right) => {
            let l = try_eval_const_size(left)?;
            let r = try_eval_const_size(right)?;
            match op {
                BinaryOp::Add => Some(l + r),
                BinaryOp::Sub => Some(l.saturating_sub(r)),
                BinaryOp::Mul => Some(l * r),
                BinaryOp::Div if r > 0 => Some(l / r),
                _ => None,
            }
        }
        _ => None,
    }
}

fn integer_literal_as_i128(int_lit: &IntegerLiteral) -> i128 {
    match int_lit {
        IntegerLiteral::I8(v) => *v as i128,
        IntegerLiteral::I16(v) => *v as i128,
        IntegerLiteral::I32(v) => *v as i128,
        IntegerLiteral::I64(v) => *v as i128,
        IntegerLiteral::I128(v) => *v,
        IntegerLiteral::U8(v) => *v as i128,
        IntegerLiteral::U16(v) => *v as i128,
        IntegerLiteral::U32(v) => *v as i128,
        IntegerLiteral::U64(v) => *v as i128,
        IntegerLiteral::U128(v) => *v as i128,
    }
}
