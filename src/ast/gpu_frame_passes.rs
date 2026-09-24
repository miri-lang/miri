// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The ordered passes a `gpu frame` block runs.
//!
//! The type checker validates a frame's passes and MIR lowering emits one
//! kernel per pass; both read the block through this one flattening, so they
//! agree on which children are passes and how often a repeat runs them.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::statement::{Statement, StatementKind};
use crate::error::syntax::Span;

/// Flattens a `gpu frame` block body into an ordered list of `gpu forall`
/// passes. A literal-count `for _ in a..b` repeat wrapping a group of passes
/// is expanded into `b - a` sequential copies (each inner pass appears once
/// per iteration), so an 18-iteration Jacobi pressure solve is written as a
/// loop yet lowers to 18 ordered passes. Ping-pong between passes is expressed
/// in the source (passes alternate their read/write buffers); no buffer
/// rewriting happens here.
///
/// Returns `(message, span)` on the first malformed child, so the type checker
/// and MIR lowering report the same diagnostic for it.
pub fn flatten_frame_passes(stmts: &[Statement]) -> Result<Vec<&Statement>, (String, Span)> {
    let mut passes: Vec<&Statement> = Vec::new();
    for stmt in stmts {
        match &stmt.node {
            // Both `gpu forall` and a bare `forall` are accepted here; residency
            // routing (a bare pass over host data belongs on the CPU, not in a
            // frame) is enforced by the type checker before lowering runs.
            StatementKind::Forall { .. } => passes.push(stmt),
            StatementKind::For(_, iterable, body) => {
                expand_frame_repeat(iterable, body, &mut passes)?;
            }
            _ => {
                return Err((
                    "'gpu frame' block may only contain 'gpu forall' passes or a literal-count 'for _ in 0..k' repeat around them".to_string(),
                    stmt.span,
                ));
            }
        }
    }
    Ok(passes)
}

/// Appends `count` copies of a repeat body's inner `gpu forall` passes to
/// `passes`, where `count` is the iteration count of `iterable`. Rejects a
/// non-block body or a body holding anything other than `gpu forall` passes.
fn expand_frame_repeat<'a>(
    iterable: &Expression,
    body: &'a Statement,
    passes: &mut Vec<&'a Statement>,
) -> Result<(), (String, Span)> {
    let count = frame_repeat_count(iterable)?;
    let StatementKind::Block(inner) = &body.node else {
        return Err((
            "'gpu frame' repeat body must be a block of 'gpu forall' passes".to_string(),
            body.span,
        ));
    };
    for s in inner {
        // A repeat body holds `gpu forall` or bare `forall` passes; the type
        // checker rejects any bare pass that resolves to the CPU by residency.
        if !matches!(&s.node, StatementKind::Forall { .. }) {
            return Err((
                "'gpu frame' repeat body may only contain 'gpu forall' passes".to_string(),
                s.span,
            ));
        }
    }
    for _ in 0..count {
        passes.extend(inner.iter());
    }
    Ok(())
}

/// Reads the iteration count of a `gpu frame` repeat's bounded literal range
/// (`a..b` → `b - a`). Rejects unbounded, descending, or negative ranges.
fn frame_repeat_count(iterable: &Expression) -> Result<usize, (String, Span)> {
    let ExpressionKind::Range(start, Some(end), _) = &iterable.node else {
        return Err((
            "'gpu frame' repeat must iterate a bounded literal range like '0..18'".to_string(),
            iterable.span,
        ));
    };
    let lit = |e: &Expression| match &e.node {
        ExpressionKind::Literal(Literal::Integer(value)) => Ok(value.to_i128()),
        _ => Err((
            "'gpu frame' repeat range bounds must be integer literals".to_string(),
            iterable.span,
        )),
    };
    let s = lit(start)?;
    let e = lit(end)?;
    if s < 0 || e < s {
        return Err((
            "'gpu frame' repeat range must be non-negative and ascending".to_string(),
            iterable.span,
        ));
    }
    usize::try_from(e - s).map_err(|_| {
        (
            "'gpu frame' repeat range is too long to expand".to_string(),
            iterable.span,
        )
    })
}
