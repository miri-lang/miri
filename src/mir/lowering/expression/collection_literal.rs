// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The collection a list, set or map literal fills.
//!
//! A literal is lowered as the empty collection its type names, followed by one
//! store per element through the same call a method stores with. The empty
//! collection registers how its elements are matched, released, cloned and
//! ordered from its declared type; the stores then hand each element over by
//! the element ABI, so a literal and `push`/`add`/`set` cannot disagree on how
//! an element is passed, how wide it is, or who owns it afterwards.

use crate::ast::expression::Expression;
use crate::ast::types::Type;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::helpers::resolve_type;
use crate::mir::{Operand, Place, Rvalue, Statement, StatementKind};

/// Where a literal builds its collection and the type its elements are stored
/// at.
pub(crate) struct LiteralTarget {
    pub(crate) place: Place,
    pub(crate) ty: Type,
}

/// The place the literal `expr` builds its collection in: `dest` when it names
/// a whole local, and a fresh temp otherwise.
///
/// A whole-local destination answers the element type with its own declared
/// type, which is the slot every element is stored at — `let s Set<i128> =
/// {1, 2}` stores two `i128`s, whatever width the literal's own elements were
/// recorded at. A projected destination is assigned from the temp by
/// [`deliver`] once the literal is filled.
pub(crate) fn literal_target(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<&Place>,
) -> LiteralTarget {
    match dest.filter(|d| d.projection.is_empty()) {
        Some(d) => LiteralTarget {
            place: d.clone(),
            ty: ctx.body.local_decls[d.local.0].ty.clone(),
        },
        None => {
            let ty = ctx
                .recorded_type(expr.id)
                .unwrap_or_else(|| resolve_type(ctx.type_checker, expr));
            LiteralTarget {
                place: Place::new(ctx.push_temp(ty.clone(), expr.span)),
                ty,
            }
        }
    }
}

/// Hand the filled collection at `built` to `dest`, when the literal was built
/// in a temp of its own because `dest` is reached through a projection.
pub(crate) fn deliver(
    ctx: &mut LoweringContext,
    built: Place,
    dest: Option<Place>,
    expr: &Expression,
) -> Operand {
    match dest {
        Some(d) if d != built => {
            ctx.push_statement(Statement {
                kind: StatementKind::Assign(d.clone(), Rvalue::Use(Operand::Move(built))),
                span: expr.span,
            });
            Operand::Copy(d)
        }
        Some(_) | None => Operand::Copy(built),
    }
}
