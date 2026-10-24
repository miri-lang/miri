// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The receiver of a write, lowered in two steps around the write's other
//! operands.
//!
//! A write — a writing method, a runtime entry point that writes an argument
//! in place, an indexed store — first lowers the code its receiver's path
//! runs (the value the path starts at, its indexes and keys), then every
//! other operand in the order the source writes them, and only then makes
//! the receiver the program's own: copies it on write, checks the method's
//! own receiver unique, or unshares each value on its path. Nothing runs
//! between that check and the write, so an argument that hands the receiver's
//! value to another name — a method storing the list it is given, a closure
//! capturing it — does so before the check, which then finds the value shared
//! and copies it, and the write lands in the copy alone.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::Type;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::Operand;

use super::expression::unshare::{lower_written_path, LoweredPath, TemporaryRoot};
use super::method_dispatch::emit_cow_check;
use super::{lower_expression, LoweringContext};

/// A write's receiver whose path is lowered and which is not yet made the
/// write's own.
#[must_use]
pub(crate) struct PendingReceiver<'e> {
    obj: &'e Expression,
    form: PendingForm,
    span: Span,
}

enum PendingForm {
    /// A path of fields, elements and entries, unshared along its length.
    Path(LoweredPath),
    /// A binding or any other value, copied on write through `cow` when it
    /// names the collection's type and its copy-on-write entry point.
    Value(Operand, Option<(Type, &'static str)>),
}

impl PendingReceiver<'_> {
    /// An operand reading the receiver's value as it stands, before it is
    /// made the write's own: a read needs no value of its own, so a
    /// read-modify-write reads first and checks only right before it stores.
    pub(crate) fn read(&self, ctx: &mut LoweringContext) -> Operand {
        match &self.form {
            PendingForm::Path(path) => Operand::Copy(path.read_place(ctx, self.obj)),
            PendingForm::Value(operand, _) => match operand {
                Operand::Copy(place) | Operand::Move(place) => Operand::Copy(place.clone()),
                Operand::Constant(_) | Operand::Function(_) => operand.clone(),
            },
        }
    }

    /// Makes the receiver the write's own and answers the operand the write
    /// goes through. Called right before the write, after every other
    /// operand is lowered.
    pub(crate) fn make_own(self, ctx: &mut LoweringContext) -> Result<Operand, LoweringError> {
        match self.form {
            PendingForm::Path(path) => {
                let written = path.make_own(ctx, self.obj)?;
                Ok(Operand::Copy(written.place))
            }
            PendingForm::Value(operand, Some((ty, cow))) => {
                emit_cow_check(ctx, operand, &ty, cow, self.span)
            }
            PendingForm::Value(operand, None) => Ok(operand),
        }
    }
}

/// The receiver a shared emitter writes through: one already the write's
/// own, like a collection a literal has just built, or one made its own once
/// the emitter has lowered its other operands.
#[must_use]
pub(crate) enum Receiver<'e> {
    Ready(Operand),
    Pending(PendingReceiver<'e>),
}

impl Receiver<'_> {
    /// The operand the write goes through, made the write's own now.
    pub(crate) fn make_own(self, ctx: &mut LoweringContext) -> Result<Operand, LoweringError> {
        match self {
            Receiver::Ready(operand) => Ok(operand),
            Receiver::Pending(pending) => pending.make_own(ctx),
        }
    }
}

/// Lowers the code the receiver `obj` of a write runs and defers making it
/// the write's own.
///
/// A bare binding is lowered as it is and, when `cow` names the collection's
/// type and its copy-on-write entry point, will be checked through it. Any
/// other receiver will be made the program's own along its whole path: the
/// struct or tuple holding it may be shared with another name, and so may the
/// collection itself, and a call's result shares whatever it was read from.
///
/// Returned beside the receiver is the temp holding the value its path starts
/// at when no binding holds it; the caller releases it once the write is done.
pub(crate) fn lower_pending_receiver<'e>(
    ctx: &mut LoweringContext,
    obj: &'e Expression,
    cow: Option<(&Type, &'static str)>,
    span: Span,
) -> Result<(PendingReceiver<'e>, Option<TemporaryRoot>), LoweringError> {
    let is_binding = matches!(obj.node, ExpressionKind::Identifier(..));
    if !is_binding {
        if let Some(mut path) = lower_written_path(ctx, obj)? {
            let temporary = path.take_temporary();
            let pending = PendingReceiver {
                obj,
                form: PendingForm::Path(path),
                span,
            };
            return Ok((pending, temporary));
        }
    }
    let watermark = ctx.body.local_decls.len();
    let obj_op = lower_expression(ctx, obj, None)?;
    let temporary = match &obj_op {
        Operand::Copy(place) | Operand::Move(place) if !is_binding => {
            Some(TemporaryRoot::new(place.local, watermark))
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => None,
    };
    let cow = cow.map(|(ty, cow)| (ty.clone(), cow));
    let pending = PendingReceiver {
        obj,
        form: PendingForm::Value(obj_op, cow),
        span,
    };
    Ok((pending, temporary))
}
