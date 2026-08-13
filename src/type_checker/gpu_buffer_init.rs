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
use crate::ast::statement::{BindingResidency, Statement, StatementKind, VariableDeclarationType};
use crate::ast::types::{primitive_type_kind, wgsl_scalar_name, BuiltinCollectionKind, TypeKind};
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
        let consts = collect_const_values(program);
        for statement in &program.body {
            collect_from_statement(statement, &mut self.gpu_buffer_inits, &consts);
        }
    }
}

/// Resolves every top-level `const NAME = <expr>` whose initializer is a
/// compile-time integer (a literal, integer arithmetic, or another such const)
/// into a concrete value. A sized `Array<T, N>()` may name one of these consts
/// as its length (e.g. `const PAINT = 128 * 128 * 4`), so the size evaluator
/// needs the resolved table to avoid emitting a zero-length buffer.
///
/// Runs a fixpoint over the declarations so a const may reference an earlier or
/// later one regardless of source order; it converges once no new const
/// resolves in a full pass.
fn collect_const_values(program: &Program) -> HashMap<String, usize> {
    let mut consts: HashMap<String, usize> = HashMap::new();
    loop {
        let mut progressed = false;
        for statement in &program.body {
            let StatementKind::Variable(decls, _) = &statement.node else {
                continue;
            };
            for decl in decls {
                if decl.declaration_type != VariableDeclarationType::Constant
                    || consts.contains_key(&decl.name)
                {
                    continue;
                }
                if let Some(init) = &decl.initializer {
                    if let Some(value) = try_eval_const_size(init, &consts) {
                        consts.insert(decl.name.clone(), value);
                        progressed = true;
                    }
                }
            }
        }
        if !progressed {
            return consts;
        }
    }
}

fn collect_from_statement(
    stmt: &Statement,
    inits: &mut HashMap<String, GpuBufferInit>,
    consts: &HashMap<String, usize>,
) {
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
                            length: extract_array_size(init, consts),
                        },
                    );
                }
            }
        }
        StatementKind::Block(stmts) => {
            for s in stmts {
                collect_from_statement(s, inits, consts);
            }
        }
        StatementKind::If(_, then_branch, else_branch, _) => {
            collect_from_statement(then_branch, inits, consts);
            if let Some(e) = else_branch {
                collect_from_statement(e, inits, consts);
            }
        }
        StatementKind::While(_, body, _) | StatementKind::For(_, _, body) => {
            collect_from_statement(body, inits, consts);
        }
        StatementKind::Forall { body, .. } => {
            collect_from_statement(body, inits, consts);
        }
        StatementKind::FunctionDeclaration(decl) => {
            if let Some(body) = &decl.body {
                collect_from_statement(body, inits, consts);
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
    primitive_type_kind(type_name)
        .and_then(|k| wgsl_scalar_name(&k))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "i32".to_string())
}

fn infer_elem_type_from_type(kind: &TypeKind) -> String {
    wgsl_scalar_name(kind).unwrap_or("i32").to_string()
}

fn infer_elem_type_from_literal(elem: &Expression) -> String {
    match &elem.node {
        // A buffer element is read by a kernel, so a float literal takes the
        // device's float width rather than the host's — the shader has no f64
        // to widen it into.
        ExpressionKind::Literal(Literal::Float(_)) => {
            infer_elem_type_from_type(&crate::type_checker::float_literals::gpu_float_width())
        }
        // Integer literals are `int` (Miri default), which maps to i32 for
        // browser portability. The host keeps i64; marshalling narrows to i32
        // for the device and widens on readback.
        ExpressionKind::Literal(Literal::Integer(_)) => "i32".to_string(),
        _ => "i32".to_string(),
    }
}

fn extract_array_size(expr: &Expression, consts: &HashMap<String, usize>) -> Option<usize> {
    if let ExpressionKind::Call(func_expr, _) = &expr.node {
        if let ExpressionKind::TypeDeclaration(_, Some(generics), _, _) = &func_expr.node {
            if generics.len() >= 2 {
                // The size is the second generic argument.
                return try_eval_const_size(&generics[1], consts);
            }
        }
    }
    None
}

/// Evaluates a simple constant size expression: a non-negative integer literal,
/// integer arithmetic over such literals, or a named top-level `const` resolved
/// through `consts`. Uses checked arithmetic to detect and prevent overflow,
/// which would otherwise silently wrap to a small value and cause incorrect
/// buffer allocation sizes.
fn try_eval_const_size(expr: &Expression, consts: &HashMap<String, usize>) -> Option<usize> {
    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(int_lit)) => {
            let val = integer_literal_as_i128(int_lit);
            (val >= 0).then_some(val as usize)
        }
        // A named top-level `const` used as the size (e.g. `Array<f32, PAINT>()`
        // where `const PAINT = 128 * 128 * 4`). Unresolved names yield `None`.
        // In a resolved constructor's generic slot the name arrives as a
        // `Custom` type node; in a const's own initializer it is a plain
        // identifier expression. Resolve both against the const table.
        ExpressionKind::Identifier(name, _) => consts.get(name).copied(),
        ExpressionKind::Type(ty, _) => match &ty.kind {
            crate::ast::types::TypeKind::Custom(name, _) => consts.get(name).copied(),
            _ => None,
        },
        ExpressionKind::Binary(left, op, right) => {
            let l = try_eval_const_size(left, consts)?;
            let r = try_eval_const_size(right, consts)?;
            match op {
                BinaryOp::Add => l.checked_add(r),
                BinaryOp::Sub => l.checked_sub(r),
                BinaryOp::Mul => l.checked_mul(r),
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
