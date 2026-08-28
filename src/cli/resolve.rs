// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared symbol resolution for CLI commands (view, patch).
//!
//! Both `miri view` and `miri patch` need to resolve function and method names
//! to their declarations in a parsed program. This module centralizes that logic
//! to avoid duplication.

use crate::ast::formatter;
use crate::ast::statement::StatementKind;
use crate::ast::{Program, Statement};
use crate::cli::sanitize_for_terminal;
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{Diagnostic, DiagnosticBuilder};

/// One function a name resolved to, and how to name it unambiguously.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    /// The declaration itself.
    pub statement: &'a Statement,
    /// The name that reaches this one declaration and no other.
    pub qualified: String,
}

/// Find the one function `name` refers to.
///
/// `Container.method` names a method directly. A bare name reaches a top-level
/// function, and also any method of that name, so a caller who knows only the
/// method name still gets there — and is told to qualify it when more than one
/// container declares it.
pub fn resolve<'a>(program: &'a Program, name: &str) -> Result<&'a Statement, Box<Diagnostic>> {
    let matches = match name.rsplit_once('.') {
        Some((container, method)) => methods_of(program, container, method),
        None => unqualified(program, name),
    };

    match matches.len() {
        0 => Err(function_not_found(name)),
        1 => Ok(matches[0].statement),
        _ => Err(ambiguous_function(name, &matches)),
    }
}

/// Everything a bare name reaches: top-level functions and any method of that name.
pub fn unqualified<'a>(program: &'a Program, name: &str) -> Vec<Candidate<'a>> {
    let mut found: Vec<Candidate<'a>> = program
        .body
        .iter()
        .filter(|statement| declared_function_name(statement).is_some_and(|found| found == name))
        .map(|statement| Candidate {
            statement,
            qualified: name.to_string(),
        })
        .collect();

    for container in &program.body {
        let Some(container_name) = container_name(container) else {
            continue;
        };
        for member in children(container) {
            if declared_function_name(member).is_some_and(|found| found == name) {
                found.push(Candidate {
                    statement: member,
                    qualified: format!("{}.{}", container_name, name),
                });
            }
        }
    }
    found
}

/// Methods with this name declared by a container with that name.
pub fn methods_of<'a>(program: &'a Program, container: &str, method: &str) -> Vec<Candidate<'a>> {
    program
        .body
        .iter()
        .filter(|statement| container_name(statement).is_some_and(|found| found == container))
        .flat_map(children)
        .filter(|member| declared_function_name(member).is_some_and(|found| found == method))
        .map(|statement| Candidate {
            statement,
            qualified: format!("{}.{}", container, method),
        })
        .collect()
}

/// The name a statement declares a function under, if it declares one.
pub fn declared_function_name(node: &Statement) -> Option<&str> {
    if let StatementKind::FunctionDeclaration(declaration) = &node.node {
        return Some(&declaration.name);
    }
    if let StatementKind::RuntimeFunctionDeclaration(_, name, _, _) = &node.node {
        return Some(name);
    }
    if let StatementKind::IntrinsicFunctionDeclaration(name, _, _, _, _) = &node.node {
        return Some(name);
    }
    None
}

/// The name a statement declares a method container under, if it declares one.
pub fn container_name(node: &Statement) -> Option<String> {
    container_name_expression(node).map(formatter::expression_text)
}

/// The name expression of a statement that can declare methods.
pub fn container_name_expression(node: &Statement) -> Option<&crate::ast::expression::Expression> {
    if let StatementKind::Class(data) = &node.node {
        return Some(&data.name);
    }
    if let StatementKind::Enum(name, ..) = &node.node {
        return Some(name);
    }
    if let StatementKind::Struct(name, ..) = &node.node {
        return Some(name);
    }
    if let StatementKind::Trait(name, ..) = &node.node {
        return Some(name);
    }
    None
}

/// The statements one statement holds.
pub fn children(node: &Statement) -> Vec<&Statement> {
    match &node.node {
        StatementKind::Block(statements) => statements.iter().collect(),
        StatementKind::If(_, then_branch, else_branch, _) => {
            let mut found = vec![then_branch.as_ref()];
            found.extend(else_branch.as_deref());
            found
        }
        StatementKind::While(_, body, _) => vec![body.as_ref()],
        StatementKind::For(_, _, body) => vec![body.as_ref()],
        StatementKind::Forall { body, .. } => vec![body.as_ref()],
        StatementKind::GpuFrame(_, _, body) => vec![body.as_ref()],
        StatementKind::GpuFrameBlock(body) => vec![body.as_ref()],
        StatementKind::FunctionDeclaration(declaration) => {
            declaration.body.as_deref().into_iter().collect()
        }
        StatementKind::Class(data) => data.body.iter().collect(),
        StatementKind::Enum(_, _, _, methods, _, _) => methods.iter().collect(),
        StatementKind::Struct(_, _, _, methods, _, _) => methods.iter().collect(),
        StatementKind::Trait(_, _, _, members, _) => members.iter().collect(),
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::Variable(..)
        | StatementKind::Return(_)
        | StatementKind::Use(..)
        | StatementKind::Type(..)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => Vec::new(),
    }
}

/// Report a name that no declaration answers to.
fn function_not_found(name: &str) -> Box<Diagnostic> {
    Box::new(
        DiagnosticBuilder::error(DiagnosticCode::BldFunctionNotFound.title().to_string())
            .code(DiagnosticCode::BldFunctionNotFound.as_str())
            .message(format!(
                "no function named `{}` in this file",
                sanitize_for_terminal(name)
            ))
            .help("a method is reached as `Class.method`; run `miri view --outline` to list what the file declares".to_string())
            .build(),
    )
}

/// Report a name that more than one declaration answers to.
fn ambiguous_function(name: &str, matches: &[Candidate<'_>]) -> Box<Diagnostic> {
    let candidates = matches
        .iter()
        .map(|candidate| sanitize_for_terminal(&candidate.qualified))
        .collect::<Vec<_>>()
        .join(", ");
    Box::new(
        DiagnosticBuilder::error(DiagnosticCode::BldAmbiguousFunctionName.title().to_string())
            .code(DiagnosticCode::BldAmbiguousFunctionName.as_str())
            .message(format!(
                "`{}` matches {} declarations: {}",
                sanitize_for_terminal(name),
                matches.len(),
                candidates
            ))
            .help(
                "qualify the name with the container that declares it, as `Class.method`"
                    .to_string(),
            )
            .build(),
    )
}
