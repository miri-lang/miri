// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Keeping a `gpu`-resident binding's host array in step with its device buffer.
//!
//! A gpu binding holds its value twice: in the device buffer kernels write, and
//! in the host array every host-side read sees. A launch changes the first and
//! not the second, so a host read that follows a launch has to be preceded by a
//! readback copying the device buffer over the host array — otherwise it hands
//! back the values the binding was declared with, and exits 0.
//!
//! [`insert_readbacks`] places those readbacks once lowering is done, from a
//! dataflow over the whole body rather than at hand-picked expressions, so every
//! position a binding is read into a host value is covered and a binding the
//! device has not changed since its last readback is not read back again. The
//! verifier's cross-residency check proves each host read it leaves is fenced.
//!
//! This module holds the vocabulary both share: which runtime entries bring a
//! handle's two copies into agreement, which leave the device ahead, and which
//! operands read a gpu binding's whole host value.

mod emit;
mod mirrors;
mod readback;
mod staleness;

pub(crate) use emit::append_readback;
pub(crate) use mirrors::{HandleCarriers, Mirror};
pub use readback::insert_readbacks;

use crate::ast::literal::Literal;
use crate::mir::body::{BindingResidency, DeviceHandleId};
use crate::mir::{Body, Local, Operand, Rvalue, Statement, StatementKind, TerminatorKind};
use std::collections::HashSet;

// These GPU intrinsics are synthesized by the compiler, never written in Miri
// source, so they are not declared as `runtime "gpu" fn` in any `.mi` (their
// device-handle / array-header arguments are not expressible Miri types). Like
// `miri_gpu_launch_inline`, codegen declares the import on demand from the
// emitted call's operands.

/// Runtime entry that fences outstanding device writes and copies a
/// `gpu`-resident buffer back to its host array.
pub(crate) const READBACK_FN: &str = "miri_gpu_readback";

/// Runtime entry that copies a host array into a `gpu`-resident binding's
/// device buffer when a host value is assigned into the binding.
pub(crate) const UPLOAD_FN: &str = "miri_gpu_upload";

/// Runtime entry that opens a fresh activation of a `gpu`-resident binding's
/// handle, so each execution of its declaration owns a device buffer of its
/// own. Codegen closes the activation with `miri_gpu_release` at scope exit.
pub(crate) const ACQUIRE_FN: &str = "miri_gpu_acquire";

/// Runtime entry that hands one handle's live activation — and the device
/// buffer it owns — to another handle, and reopens the first with no buffer:
/// what a gpu-to-gpu move (`gpu var b = a`) does to the two bindings.
pub(crate) const TRANSFER_FN: &str = "miri_gpu_transfer";

/// What a terminator does to the agreement between device buffers and the host
/// arrays they mirror.
pub(crate) enum DeviceEffect<'a> {
    /// A fresh activation: the handle has no device buffer, so every host array
    /// mirroring it is the only copy of its value.
    Activates(u64),
    /// A readback of the handle's device buffer into the host array of the
    /// binding named, or — for a scalar, read back through a one-element array
    /// — of no binding in particular.
    ReadsBack(u64, Option<Local>),
    /// An upload of a host array into the handle's device buffer.
    Uploads(u64),
    /// A gpu-to-gpu move: `to` takes over `from`'s device buffer, and `from` is
    /// left with none.
    Transfers { from: u64, to: u64 },
    /// A kernel launch, or a call specialized to launch on a caller's buffer:
    /// each handle it runs on may now hold results its host arrays lack.
    Launches(&'a [Option<DeviceHandleId>]),
    /// Neither.
    Nothing,
}

/// The effect `kind` has on the handles it names.
pub(crate) fn device_effect(kind: &TerminatorKind) -> DeviceEffect<'_> {
    match kind {
        TerminatorKind::Call {
            func,
            args,
            arg_handles,
            ..
        } => runtime_entry_effect(func, args).unwrap_or(DeviceEffect::Launches(arg_handles)),
        TerminatorKind::GpuLaunch { launch_args, .. } => {
            DeviceEffect::Launches(launch_args.arg_handles())
        }
        TerminatorKind::VirtualCall { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => DeviceEffect::Nothing,
    }
}

/// The effect of a call to one of the residency runtime entries, or `None` for
/// any other callee.
fn runtime_entry_effect<'a>(func: &Operand, args: &[Operand]) -> Option<DeviceEffect<'a>> {
    let symbol = func.called_symbol()?;
    let handle = || args.first().and_then(handle_argument);
    let effect = match symbol {
        READBACK_FN => {
            handle().map(|h| DeviceEffect::ReadsBack(h, args.get(1).and_then(whole_local)))
        }
        UPLOAD_FN => handle().map(DeviceEffect::Uploads),
        ACQUIRE_FN => handle().map(DeviceEffect::Activates),
        TRANSFER_FN => handle()
            .zip(args.get(1).and_then(handle_argument))
            .map(|(from, to)| DeviceEffect::Transfers { from, to }),
        _ => return None,
    };
    Some(effect.unwrap_or(DeviceEffect::Nothing))
}

/// The value of the integer constant a synchronizing call passes as its handle.
pub(crate) fn handle_argument(operand: &Operand) -> Option<u64> {
    let Operand::Constant(constant) = operand else {
        return None;
    };
    let Literal::Integer(value) = &constant.literal else {
        return None;
    };
    u64::try_from(value.to_i128()).ok()
}

/// The device handle of `local` when it is a `gpu`-resident binding.
pub(crate) fn gpu_handle(body: &Body, local: Local) -> Option<DeviceHandleId> {
    let decl = &body.local_decls[local.0];
    if decl.residency == BindingResidency::Gpu {
        decl.device_handle
    } else {
        None
    }
}

/// The local an operand reads whole, or `None` for a constant or a projection.
///
/// A projection reads a part of the host array, which the element cross-read
/// diagnostic refuses at the source level before lowering ever sees it.
pub(crate) fn whole_local(operand: &Operand) -> Option<Local> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            Some(place.local)
        }
        Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => None,
    }
}

/// The operands an rvalue reads as values.
///
/// A reference, a length, and a GPU intrinsic read no element data; an atomic
/// operation only exists in device code, which has no gpu-resident bindings.
pub(crate) fn host_read_operands(rvalue: &Rvalue) -> Vec<&Operand> {
    match rvalue {
        Rvalue::Use(operand) => vec![operand],
        Rvalue::UnaryOp(_, operand) | Rvalue::Cast(operand, _) => vec![operand.as_ref()],
        Rvalue::BinaryOp(_, left, right) => vec![left.as_ref(), right.as_ref()],
        Rvalue::MathIntrinsic(_, operands) | Rvalue::Aggregate(_, operands) => {
            operands.iter().collect()
        }
        Rvalue::Phi(incoming) => incoming.iter().map(|(operand, _)| operand).collect(),
        Rvalue::Ref(_) | Rvalue::Len(_) | Rvalue::GpuIntrinsic(_) | Rvalue::AtomicOp { .. } => {
            Vec::new()
        }
    }
}

/// The host locals a readback writes a gpu scalar's device value into: the
/// one-element array a scalar is read back through.
///
/// Seeding one reads the scalar's host value, but only to fill the array the
/// readback is about to overwrite, so it is not a read of the result.
pub(crate) fn readback_destinations(body: &Body) -> HashSet<Local> {
    body.basic_blocks
        .iter()
        .filter_map(|block| {
            let Some(TerminatorKind::Call { func, args, .. }) =
                block.terminator.as_ref().map(|t| &t.kind)
            else {
                return None;
            };
            if func.called_symbol() != Some(READBACK_FN) {
                return None;
            }
            args.get(1).and_then(whole_local)
        })
        .filter(|&local| gpu_handle(body, local).is_none())
        .collect()
}

/// The gpu-resident bindings `statement` reads whole into a host value.
///
/// Every operand the store reads counts — a copy into a host binding, a closure
/// capture, an element of a tuple, array or collection literal, an operand of an
/// operator — except two that keep the value where it is: seeding the array a
/// scalar readback is about to fill, and a copy into another binding carrying
/// the same handle, which adopts the host array (see [`statement_adoption`]). A
/// copy into a gpu binding with a handle of its own is a host read, since it
/// copies the host array — after a move has handed the source's buffer to the
/// target (`gpu var b = a`), it reads a source the transfer left current.
pub(crate) fn statement_host_reads(
    body: &Body,
    statement: &Statement,
    readback_destinations: &HashSet<Local>,
) -> Vec<Local> {
    let (StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue)) =
        &statement.kind
    else {
        return Vec::new();
    };
    let whole_dest = dest.projection.is_empty().then_some(dest.local);
    if whole_dest.is_some_and(|local| readback_destinations.contains(&local)) {
        return Vec::new();
    }
    let dest_handle = whole_dest.and_then(|local| gpu_handle(body, local));
    host_read_operands(rvalue)
        .into_iter()
        .filter_map(whole_local)
        .filter(|&local| gpu_handle(body, local).is_some_and(|h| Some(h) != dest_handle))
        .collect()
}

/// The binding `statement` hands another binding's host array to without
/// reading it, as `(target, source)`: a whole copy between two gpu bindings
/// carrying the same handle, as a move out of a borrowed parameter makes. The
/// target's host array is then the source's, and lags the device exactly as far.
pub(crate) fn statement_adoption(body: &Body, statement: &Statement) -> Option<(Local, Local)> {
    let (StatementKind::Assign(dest, Rvalue::Use(operand))
    | StatementKind::Reassign(dest, Rvalue::Use(operand))) = &statement.kind
    else {
        return None;
    };
    let source = whole_local(operand)?;
    let target = dest.projection.is_empty().then_some(dest.local)?;
    let handle = gpu_handle(body, target)?;
    (source != target && gpu_handle(body, source) == Some(handle)).then_some((target, source))
}

/// The gpu-resident bindings a terminator reads whole on the host: the value a
/// branch switches on, and the host array an upload copies into another
/// binding's device buffer.
///
/// Other call arguments are left to the type checker's residency gate, which
/// refuses a gpu binding passed to any function that reads its elements. What
/// reaches a call is either a device buffer a residency-specialized callee
/// launches on, or a binding whose callee reads only its length, which the host
/// array holds correctly whatever the device did.
pub(crate) fn terminator_host_reads(body: &Body, kind: &TerminatorKind) -> Vec<Local> {
    let read = match kind {
        TerminatorKind::SwitchInt { discr, .. } => whole_local(discr),
        TerminatorKind::Call { func, args, .. } if func.called_symbol() == Some(UPLOAD_FN) => {
            upload_source(body, args)
        }
        TerminatorKind::Call { .. }
        | TerminatorKind::VirtualCall { .. }
        | TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => None,
    };
    read.filter(|&local| gpu_handle(body, local).is_some())
        .into_iter()
        .collect()
}

/// The gpu binding an upload copies from, when it is not the binding being
/// uploaded into.
fn upload_source(body: &Body, args: &[Operand]) -> Option<Local> {
    let target = args.first().and_then(handle_argument);
    let source = args.get(1).and_then(whole_local)?;
    let handle = gpu_handle(body, source)?;
    (Some(handle.0) != target).then_some(source)
}
