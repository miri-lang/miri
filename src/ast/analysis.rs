// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Pure-AST analysis: GPU usage detection and other structural queries.

use crate::ast::statement::{AcceleratorTarget, Statement, StatementKind};

/// Returns true if any statement in the iterable uses GPU constructs.
pub fn program_uses_gpu<'a, I: IntoIterator<Item = &'a Statement>>(stmts: I) -> bool {
    for stmt in stmts {
        if stmt_uses_gpu(stmt) {
            return true;
        }
    }
    false
}

/// Returns true if a statement or any of its nested children use GPU constructs.
fn stmt_uses_gpu(stmt: &Statement) -> bool {
    match &stmt.node {
        StatementKind::Forall { device, .. } => {
            matches!(device, AcceleratorTarget::Gpu)
        }
        StatementKind::GpuFrame(_, _, _) => true,
        StatementKind::GpuFrameBlock(block) => stmt_uses_gpu(block),
        StatementKind::FunctionDeclaration(decl) => {
            decl.properties.is_gpu || decl.body.as_ref().is_some_and(|b| stmt_uses_gpu(b))
        }
        StatementKind::Block(stmts) => stmts.iter().any(stmt_uses_gpu),
        StatementKind::If(_, then_branch, else_branch, _) => {
            stmt_uses_gpu(then_branch) || else_branch.as_ref().is_some_and(|s| stmt_uses_gpu(s))
        }
        StatementKind::While(_, body, _) | StatementKind::For(_, _, body) => stmt_uses_gpu(body),
        StatementKind::Class(class_data) => class_data.body.iter().any(stmt_uses_gpu),
        StatementKind::Struct(_, _, _, methods, _)
        | StatementKind::Enum(_, _, _, methods, _, _)
        | StatementKind::Trait(_, _, _, methods, _) => methods.iter().any(stmt_uses_gpu),
        // A `gpu let` / `gpu var` binding may trigger a cross-residency
        // readback that calls into the GPU runtime even when the program has
        // no `gpu for` / `gpu fn`, so a gpu-resident declaration alone
        // requires linking it.
        StatementKind::Variable(decls, _) => decls
            .iter()
            .any(|d| d.residency == crate::ast::statement::BindingResidency::Gpu),
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::Return(_)
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => false,
    }
}
