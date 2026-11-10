// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The copies a body binds on entry of the parameters it writes.
//!
//! A collection, a struct or a tuple passed to a function is a value: a write
//! to it inside the function is not seen by the caller. The caller still holds
//! the value it passed, often under more than one name, so its count says
//! nothing about whether the body may write it in place. A body that writes a
//! parameter, or writes through a path starting at one — a field, a tuple
//! element, a collection element, a map entry — whether by a store, a method
//! that writes its receiver or a runtime entry point that writes what it is
//! handed in place, is therefore lowered as if it began with
//! `var <name> = <name>`: the copy takes a reference of its own, and
//! every write goes through it, unsharing it first like any binding's value.
//! The caller's value is left alone and the copy is released when the body
//! ends, unless it is returned.
//!
//! Which parameters a body writes is the type checker's verdict
//! ([`ParameterWrites`]), made before any body is lowered, so a read lowered
//! before the write — a loop header — reads the copy as well. This module
//! only binds the copies, for the written parameters whose value a write
//! copies.
//!
//! A method's receiver is never copied: a call to a method that writes it
//! hands it a value only the caller holds, so the body writes it in place.
//! Neither is an `out` parameter, which is the caller's binding itself.
//!
//! [`ParameterWrites`]: crate::type_checker::body_writes::ParameterWrites

use crate::ast::common::Parameter;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::type_facts::unsharing::is_copied_on_write;
use crate::mir::{
    Local, Operand, Place, Rvalue, Statement as MirStatement, StatementKind as MirStatementKind,
};

/// The scope the entry copies of a body's written parameters are bound in.
#[must_use]
pub(crate) struct WrittenParameterCopies {
    is_scoped: bool,
}

impl WrittenParameterCopies {
    /// Ends the scope of the copies once the body is lowered, releasing them
    /// when the body falls off its end; a `return` released them already.
    pub(crate) fn close(self, ctx: &mut LoweringContext, span: Span) {
        if self.is_scoped {
            ctx.pop_scope(span);
        }
    }
}

/// Binds, at the body's entry, a copy of each of `params` its body writes,
/// so every use of the parameter's name inside the body reads the copy.
pub(crate) fn copy_written_parameters(
    ctx: &mut LoweringContext,
    params: &[Parameter],
) -> WrittenParameterCopies {
    let written = written_parameters(ctx, params);
    if written.is_empty() {
        return WrittenParameterCopies { is_scoped: false };
    }
    ctx.push_scope();
    for (param, local) in written {
        bind_parameter_copy(ctx, param, local);
    }
    WrittenParameterCopies { is_scoped: true }
}

/// Each of `params` its body writes and that holds a value its caller still
/// holds, with its local.
fn written_parameters<'p>(
    ctx: &LoweringContext,
    params: &'p [Parameter],
) -> Vec<(&'p Parameter, Local)> {
    if ctx.body.is_gpu() {
        return Vec::new();
    }
    params
        .iter()
        .filter(|param| ctx.type_checker.parameter_writes.writes(param))
        .filter_map(|param| {
            let local = *ctx.variable_map.get(param.name.as_str())?;
            is_copied_parameter(ctx, local).then_some((param, local))
        })
        .collect()
}

/// Whether the parameter `local` holds a value its caller still holds and a
/// write copies: not the receiver, not `out`, not GPU-resident.
fn is_copied_parameter(ctx: &LoweringContext, local: Local) -> bool {
    let decl = &ctx.body.local_decls[local.0];
    ctx.body.is_lent_param(local)
        && decl.device_handle.is_none()
        && is_copied_on_write(&ctx.body, &decl.ty)
}

/// Binds `param`'s name to a new local holding a copy of the parameter
/// `local`, the way `var <name> = <name>` would.
///
/// The copy is a local of the body's own, numbered past `arg_count`, so the
/// rules that tell a parameter from a local by that number treat it as the
/// `var` it stands for, on purpose: consuming a resource held in it ends its
/// storage (`variable::release_consumed_binding`), and calling its drop hook
/// releases it (`drop_hook_call::release_owned_local`). The caller's
/// parameter is never touched by either.
fn bind_parameter_copy(ctx: &mut LoweringContext, param: &Parameter, local: Local) {
    let span = param.typ.span;
    let ty = ctx.body.local_decls[local.0].ty.clone();
    let copy = ctx.alloc_local(param.name.clone(), ty, span);
    ctx.push_statement(MirStatement {
        kind: MirStatementKind::Assign(
            Place::new(copy),
            Rvalue::Use(Operand::Copy(Place::new(local))),
        ),
        span,
    });
    ctx.bind_local_name(param.name.clone(), copy);
}
