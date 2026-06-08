// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! AST factory helpers, grouped by produced node category.
//!
//! Every constructor is named after the grammar non-terminal / AST node it
//! returns. Span defaults to `(0, 0)` for the non-`_with_span` variant.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::error::syntax::Span;

mod declaration;
mod expression;
mod lhs;
mod literal;
mod primitives;
mod statement;
mod type_factory;

pub use declaration::*;
pub use expression::*;
pub use lhs::*;
pub use literal::*;
pub use primitives::*;
pub use statement::*;
pub use type_factory::*;

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

pub(super) fn next_id() -> usize {
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

pub(super) fn expr(kind: ExpressionKind) -> Expression {
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
