// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A field or an element a call takes `out`.
//!
//! `step(grid[i])` with `step(c out Cell)` hands `step` the slot `grid[i]`
//! itself: the callee writes the place, and the caller reads what it left
//! there once the call returns. The place is lowered in two steps around the
//! call's other operands, the way a writing method's receiver is
//! (`written_receiver.rs`): first the code its path runs — each index, copied
//! into a temp of its own when a binding holds it, so a call writing that
//! binding through another `out` argument does not move the place — then
//! every other operand, and only then, right before the call, every
//! copy-on-write block above the slot is made the caller's own. The slot
//! itself is left as it is, for the callee to write. [`TakenArguments`]
//! keeps that order for every call that takes a place.
//!
//! How the slot is handed over is the representation's to say
//! ([`Representation::is_passed_out_by_address`]): a counted word, an
//! optional and a value held as its bytes are handed as the place, which a
//! backend passes by the slot's address, so the callee writes in place and
//! every exit of the callee leaves its value there. A word nothing counts may
//! sit in a slot narrower than the word a callee reads, so it is copied into
//! a temp of its own, the temp is handed over, and the temp is copied back
//! into the slot as the first thing after the call ([`Restore`]).
//!
//! A slot inside a class instance that is no resource is copied in and out
//! the same way, whatever its representation
//! ([`is_shared_on_assignment`]): class instances are shared on assignment,
//! so another binding may reach the instance, and through it the slot, while
//! the call runs, and a callee replacing the slot's value in place would
//! release a value that binding still reads. The copy holds a reference of
//! its own for the call, and the path is made the caller's own again before
//! the value the call left is stored back. Once a class instance is copied
//! on write as every other value is, such a slot is handed over by its
//! address too.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::Type;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::place::PlaceElem;
use crate::mir::representation::Representation;
use crate::mir::type_facts::unsharing::is_shared_on_assignment;
use crate::mir::{Local, Operand, Place, Rvalue, Statement, StatementKind};

use super::dispatch::place_at_parameter;
use super::expression::unshare::{lower_taken_path, TakenPath};
use super::LoweringContext;

/// The fields and elements one call takes `out`, each lowered up to the call
/// at the parameter it binds and not yet made the caller's own. Their slots
/// stay empty until [`TakenArguments::make_own`] fills them, after every
/// other operand of the call.
#[derive(Default)]
#[must_use]
pub(crate) struct TakenArguments {
    pending: Vec<(usize, PendingOut)>,
    /// Each temp holding the value an `out` argument's place held before
    /// the call, for a default reading that parameter.
    pre_call_reads: Vec<Local>,
}

impl TakenArguments {
    /// Lowers `value`, bound to the parameter at `position`, up to the call
    /// when that parameter is `out` (`is_out`) and `value` is a field or an
    /// element. False when it is anything else, for the caller to lower as
    /// any argument: a whole binding is handed over as its local.
    pub(crate) fn take(
        &mut self,
        ctx: &mut LoweringContext,
        (position, is_out): (usize, bool),
        value: &Expression,
    ) -> Result<bool, LoweringError> {
        let is_path = matches!(
            value.node,
            ExpressionKind::Member(..) | ExpressionKind::Index(..)
        );
        if !is_out || !is_path {
            return Ok(false);
        }
        let Some(path) = lower_taken_path(ctx, value)? else {
            return Err(LoweringError::internal(
                DiagnosticCode::MirValidationFailed,
                "an `out` argument is a field or an element of no binding a call can take",
                value.span,
            ));
        };
        let span = value.span;
        self.pending.push((position, PendingOut { path, span }));
        Ok(true)
    }

    /// Whether the parameter at `position` binds a place this call takes,
    /// whose slot [`TakenArguments::make_own`] fills.
    pub(crate) fn holds(&self, position: usize) -> bool {
        self.pending.iter().any(|(taken, _)| *taken == position)
    }

    /// Reads into a temp of its own the value the `out` parameter at
    /// `position` holds before the call, for a default of a later parameter
    /// that reads it. A default is a sibling operand: it sees the value the
    /// place held before the call, whatever the callee writes there. The
    /// temp holds a reference of its own, so a callee replacing the place
    /// releases nothing the default still reads; it is released once the
    /// default is lowered unless the default's value holds it
    /// ([`TakenArguments::release_reads_since`]), else with the call's other
    /// temps ([`Restores::pre_call_reads`]).
    ///
    /// `handed` is the operand already in the parameter's slot, a whole
    /// variable; a field or an element this call takes is read through its
    /// path, nothing above the slot made the caller's own yet.
    pub(crate) fn read_before_call(
        &mut self,
        ctx: &mut LoweringContext,
        (position, handed): (usize, Option<&Operand>),
        span: Span,
    ) -> Result<Local, LoweringError> {
        let pending = self.pending.iter().find(|(taken, _)| *taken == position);
        let (place, ty) = match (pending, handed) {
            (Some((_, pending)), _) => match pending.path.slot() {
                Some((place, ty)) => (place, ty.clone()),
                None => return Err(unread_out_argument(position, span)),
            },
            (None, Some(Operand::Copy(place) | Operand::Move(place))) => {
                let ty = (ctx.body.local_decls.get(place.local.0))
                    .filter(|_| place.projection.is_empty())
                    .map(|decl| decl.ty.clone())
                    .ok_or_else(|| unread_out_argument(position, span))?;
                (place.clone(), ty)
            }
            (None, Some(Operand::Constant(_) | Operand::Function(_)) | None) => {
                return Err(unread_out_argument(position, span));
            }
        };
        let temp = ctx.push_temp(ty, span);
        ctx.push_statement(Statement {
            kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(Operand::Copy(place))),
            span,
        });
        self.pre_call_reads.push(temp);
        Ok(temp)
    }

    /// How many pre-call reads are held so far: the mark a default's own
    /// reads start at, for [`TakenArguments::release_reads_since`].
    pub(crate) fn read_count(&self) -> usize {
        self.pre_call_reads.len()
    }

    /// Releases, before the call, each pre-call read made since `mark` that
    /// the default's value `lowered` does not hold. A default is evaluated
    /// as its own expression before the place is taken: one reading a field
    /// of the value (`id int = r.id`) keeps nothing of it, so the callee
    /// replacing the place releases the old value there, its hook included.
    ///
    /// A value read out of a read (`r.id`) is first copied into a temp of
    /// its own at the parameter's type `ty`, which the call's other temps are
    /// released with. A read the default's value is whole (`keep = r`) lives
    /// until the call returns, released with the call's other temps.
    pub(crate) fn release_reads_since(
        &mut self,
        ctx: &mut LoweringContext,
        (mark, ty): (usize, Option<&Type>),
        (lowered, span): (&mut Operand, Span),
    ) {
        let reads = self
            .pre_call_reads
            .split_off(mark.min(self.pre_call_reads.len()));
        let held = match &*lowered {
            Operand::Copy(place) | Operand::Move(place) => {
                Some((place.local, place.projection.is_empty()))
            }
            Operand::Constant(_) | Operand::Function(_) => None,
        };
        let part_of_a_read = held.filter(|(local, is_whole)| !is_whole && reads.contains(local));
        if let (Some(_), Some(ty)) = (part_of_a_read, ty) {
            let temp = ctx.push_temp(ty.clone(), span);
            let read = std::mem::replace(lowered, Operand::Copy(Place::new(temp)));
            ctx.push_statement(Statement {
                kind: StatementKind::Assign(Place::new(temp), Rvalue::Use(read)),
                span,
            });
        }
        let held = match &*lowered {
            Operand::Copy(place) | Operand::Move(place) => Some(place.local),
            Operand::Constant(_) | Operand::Function(_) => None,
        };
        for temp in reads {
            if Some(temp) == held {
                self.pre_call_reads.push(temp);
            } else {
                ctx.emit_temp_drop(temp, temp.0, span);
            }
        }
    }

    /// Makes every place taken the caller's own and puts the operand handing
    /// it over in its slot among `slots`. Called right before the call, after
    /// every other operand is lowered; answers what to store back once the
    /// call returns.
    pub(crate) fn make_own(
        self,
        ctx: &mut LoweringContext,
        slots: &mut Vec<Option<Operand>>,
    ) -> Result<Restores, LoweringError> {
        let mut restores = Vec::new();
        for (position, pending) in self.pending {
            let (operand, restore) = pending.make_own(ctx)?;
            place_at_parameter(slots, position, operand);
            restores.extend(restore);
        }
        Ok(Restores {
            restores,
            pre_call_reads: self.pre_call_reads,
        })
    }
}

/// The internal error for a default reading the `out` parameter at
/// `position`, which the call hands neither a whole variable nor a place it
/// takes.
fn unread_out_argument(position: usize, span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirCustomLoweringError,
        format!("a default reads the `out` parameter at {position}, which the call hands no place"),
        span,
    )
}

/// A field or an element a call takes `out`, its path lowered.
struct PendingOut {
    path: TakenPath,
    span: Span,
}

impl PendingOut {
    /// Makes every copy-on-write block above the slot the caller's own and
    /// answers the operand the call takes, with what to store back into the
    /// slot once the call returns when the slot's value is copied in.
    fn make_own(
        self,
        ctx: &mut LoweringContext,
    ) -> Result<(Operand, Option<Restore>), LoweringError> {
        let PendingOut { path, span } = self;
        let resources = ctx.type_checker.resource_types();
        let is_shared =
            (path.enclosing_types()).any(|ty| is_shared_on_assignment(&ctx.body, ty, resources));
        let (place, ty) = path.make_own_enclosing(ctx, span)?;
        let representation = Representation::of(&ty.kind, ctx.body.layout_scope());
        if !is_shared && representation.is_passed_out_by_address() {
            return Ok((Operand::Copy(place), None));
        }
        let is_counted_field = matches!(place.projection.last(), Some(PlaceElem::Field(_)))
            && ctx.is_counted(&ty.kind);
        let temp = ctx.push_temp(ty, span);
        ctx.push_statement(Statement {
            kind: StatementKind::Assign(
                Place::new(temp),
                Rvalue::Use(Operand::Copy(place.clone())),
            ),
            span,
        });
        let restore = Restore {
            place,
            temp,
            span,
            retaken: is_shared.then_some(path),
            is_counted_field,
        };
        Ok((Operand::Copy(Place::new(temp)), Some(restore)))
    }
}

/// A value copied out of the place a call takes, to be stored back once the
/// call returns.
struct Restore {
    place: Place,
    temp: Local,
    span: Span,
    /// The path to make the caller's own again before the store, when the
    /// place lies inside a value another binding may reach.
    retaken: Option<TakenPath>,
    /// Whether the place is a field holding a counted value, which a store
    /// replaces by releasing the value it held, as an assignment to the field
    /// does; an element store releases the element it replaces itself.
    is_counted_field: bool,
}

/// What a call leaves to store back into the places it took once it
/// returns, and the temps its defaults read the places' values from before
/// the call.
#[must_use]
pub(crate) struct Restores {
    restores: Vec<Restore>,
    pre_call_reads: Vec<Local>,
}

impl Restores {
    /// Nothing to store back: a call that takes no field or element.
    pub(crate) fn none() -> Self {
        Self {
            restores: Vec::new(),
            pre_call_reads: Vec::new(),
        }
    }

    /// The temps holding what an `out` argument's place held before the
    /// call, read by a default ([`TakenArguments::read_before_call`]): the
    /// call's own, released with its other temps once it returns.
    pub(crate) fn pre_call_reads(&self) -> &[Local] {
        &self.pre_call_reads
    }

    /// Stores each value a call was handed in place of a field or an element
    /// back into that place. Emitted first in the block the call continues
    /// in.
    pub(crate) fn store_back(self, ctx: &mut LoweringContext) -> Result<(), LoweringError> {
        for restore in self.restores {
            let place = match &restore.retaken {
                Some(path) => path.make_own_enclosing(ctx, restore.span)?.0,
                None => restore.place,
            };
            // The temp is the call's operand, released with the call's
            // other temps, so the store copies it and takes a reference of
            // its own.
            let stored = Rvalue::Use(Operand::Copy(Place::new(restore.temp)));
            let kind = if restore.is_counted_field {
                StatementKind::Reassign(place, stored)
            } else {
                StatementKind::Assign(place, stored)
            };
            ctx.push_statement(Statement {
                kind,
                span: restore.span,
            });
        }
        Ok(())
    }
}
