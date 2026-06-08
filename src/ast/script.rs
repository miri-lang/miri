// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Script-mode main synthesis: wrapping top-level statements in a synthetic main function.

use crate::ast::factory::{
    block, func, int_literal_expression, return_statement, stmt_with_span, type_expr_non_null,
    type_int,
};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::Program;
use crate::error::syntax::Span;

/// Returns true if the program contains a function named "main".
fn has_main_function(program: &Program) -> bool {
    program.body.iter().any(|s| {
        if let StatementKind::FunctionDeclaration(decl) = &s.node {
            decl.name == "main"
        } else {
            false
        }
    })
}

/// Returns true if the statement can be wrapped in a synthetic main function.
fn is_wrappable_stmt(stmt: &Statement) -> bool {
    matches!(
        &stmt.node,
        StatementKind::Expression(_)
            | StatementKind::Variable(..)
            | StatementKind::If(..)
            | StatementKind::While(..)
            | StatementKind::For(..)
            | StatementKind::GpuFor(..)
            | StatementKind::GpuFrame(..)
            | StatementKind::Block(..)
            | StatementKind::Return(..)
            | StatementKind::Break
            | StatementKind::Continue
    )
}

/// Returns true if the statement should stay at the top level (not wrapped in main).
fn is_top_level_stmt(stmt: &Statement) -> bool {
    matches!(
        &stmt.node,
        StatementKind::Use(..)
            | StatementKind::Class(..)
            | StatementKind::Struct(..)
            | StatementKind::Enum(..)
            | StatementKind::Trait(..)
            | StatementKind::Type(..)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..)
            | StatementKind::FunctionDeclaration(..)
    )
}

/// Wraps a script-style program (with or without function declarations) in a synthetic `main` function.
/// Skips programs that already contain a `main` function or non-wrappable type definitions.
///
/// The synthetic main has return type `Int` and appends `return 0` so the process
/// exits cleanly. Without this, the exit code leaks from whatever value the last
/// expression leaves in the return register.
pub fn wrap_script_in_main(program: &mut Program) {
    if has_main_function(program) {
        return;
    }

    let return_zero = return_statement(Some(Box::new(int_literal_expression(0))));
    let int_ret = type_expr_non_null(type_int());

    if program.body.is_empty() {
        let body = block(vec![return_zero]);
        let main_fn = func("main").return_type(int_ret).build(body);
        program.body = vec![main_fn];
        return;
    }

    // Separate top-level declarations (use, class, struct, etc.) from executable statements.
    let mut top_level = Vec::new();
    let mut body_stmts = Vec::new();

    // First, verify we can wrap the program. If not, return early.
    for stmt in &program.body {
        if !is_top_level_stmt(stmt) && !is_wrappable_stmt(stmt) {
            return;
        }
    }

    let old_body = std::mem::take(&mut program.body);
    for stmt in old_body {
        if is_top_level_stmt(&stmt) {
            top_level.push(stmt);
        } else if is_wrappable_stmt(&stmt) {
            body_stmts.push(stmt);
        }
    }

    body_stmts.push(return_zero);
    let body = block(body_stmts);
    let main_fn = func("main").return_type(int_ret).build(body);
    top_level.push(main_fn);
    program.body = top_level;
}

/// Ensures that a user-defined `main()` function returns `Int` with an
/// implicit `return 0` at the end, so the process exits cleanly.
pub fn patch_main_return(program: &mut Program) {
    let return_zero = return_statement(Some(Box::new(int_literal_expression(0))));
    let int_ret = type_expr_non_null(type_int());

    for stmt in &mut program.body {
        if let StatementKind::FunctionDeclaration(decl) = &mut stmt.node {
            if decl.name == "main" && decl.return_type.is_none() {
                // Set return type to Int
                decl.return_type = Some(Box::new(int_ret));

                // Append `return 0` to the body
                if let Some(body_stmt) = &mut decl.body {
                    match &mut body_stmt.node {
                        StatementKind::Block(stmts) => {
                            stmts.push(return_zero);
                        }
                        _ => {
                            let span = body_stmt.span;
                            let existing = std::mem::replace(
                                body_stmt.as_mut(),
                                stmt_with_span(StatementKind::Empty, Span::new(0, 0)),
                            );
                            **body_stmt = stmt_with_span(
                                StatementKind::Block(vec![existing, return_zero]),
                                span,
                            );
                        }
                    }
                }
                return;
            }
        }
    }
}
