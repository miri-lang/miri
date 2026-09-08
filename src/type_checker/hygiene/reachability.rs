// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statements written where nothing runs.

use crate::ast::statement::{Statement, StatementKind};
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::hygiene::walk::Contents;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reports the first statement of each scope that follows one leaving it.
    ///
    /// Only the first is reported: every statement after it is unreachable for
    /// the same reason, and repeating that once per line buries the one thing
    /// the author has to move.
    pub(super) fn report_unreachable_statements(&mut self, contents: &Contents<'_>) {
        for scope in &contents.scopes {
            let Some((keyword, unreachable)) = first_unreachable(scope.statements) else {
                continue;
            };
            self.report_warning(
                DiagnosticCode::TypUnreachableStatement,
                DiagnosticCode::TypUnreachableStatement.title().to_string(),
                format!(
                    "Unreachable statement: the '{}' above leaves this block, so nothing after \
                     it runs",
                    keyword
                ),
                unreachable.span,
                Some(format!(
                    "move the statement above the '{}', or remove it.",
                    keyword
                )),
            );
        }
    }
}

/// The statement a scope can never reach, with the keyword that made it so.
///
/// A statement carrying no span is one the compiler put there — the `return 0`
/// appended to every `main`, for one — and it follows the author's own `return`
/// by construction. Reporting on code nobody wrote would be wrong wherever it
/// landed, and it has no location to land on.
fn first_unreachable(scope: &[Statement]) -> Option<(&'static str, &Statement)> {
    let mut leaves = None;
    for statement in scope {
        if let Some(keyword) = leaves {
            if is_written(statement) {
                return Some((keyword, statement));
            }
            continue;
        }
        leaves = leaving_keyword(statement);
    }
    None
}

/// True when the statement is one the author wrote, rather than one synthesis
/// added.
fn is_written(statement: &Statement) -> bool {
    !statement.span.is_empty() && !matches!(statement.node, StatementKind::Empty)
}

/// The keyword a statement uses to leave its block, when it leaves at all.
fn leaving_keyword(statement: &Statement) -> Option<&'static str> {
    match &statement.node {
        StatementKind::Return(_) => Some("return"),
        StatementKind::Break => Some("break"),
        StatementKind::Continue => Some("continue"),
        StatementKind::Empty
        | StatementKind::Expression(_)
        | StatementKind::Block(_)
        | StatementKind::Variable(_, _)
        | StatementKind::If(..)
        | StatementKind::While(..)
        | StatementKind::For(..)
        | StatementKind::Forall { .. }
        | StatementKind::GpuFrame(..)
        | StatementKind::GpuFrameBlock(_)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::Enum(..)
        | StatementKind::Struct(..)
        | StatementKind::Class(_)
        | StatementKind::Trait(..)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::syntax::Span;

    fn written(node: StatementKind) -> Statement {
        Statement {
            id: 0,
            node,
            span: Span::new(1, 2),
            trivia: Default::default(),
        }
    }

    fn synthesized(node: StatementKind) -> Statement {
        Statement {
            id: 0,
            node,
            span: Span::new(0, 0),
            trivia: Default::default(),
        }
    }

    #[test]
    fn test_a_statement_after_a_return_is_unreachable() {
        let scope = vec![
            written(StatementKind::Return(None)),
            written(StatementKind::Empty),
            written(StatementKind::Break),
        ];
        let Some((keyword, unreachable)) = first_unreachable(&scope) else {
            panic!("the break follows a return");
        };
        assert_eq!(keyword, "return");
        assert!(matches!(unreachable.node, StatementKind::Break));
    }

    #[test]
    fn test_a_return_that_ends_its_block_leaves_nothing_unreachable() {
        let scope = vec![
            written(StatementKind::Empty),
            written(StatementKind::Return(None)),
        ];
        assert!(first_unreachable(&scope).is_none());
    }

    #[test]
    fn test_a_statement_the_compiler_added_is_not_reported() {
        let scope = vec![
            written(StatementKind::Return(None)),
            synthesized(StatementKind::Return(None)),
        ];
        assert!(first_unreachable(&scope).is_none());
    }
}
