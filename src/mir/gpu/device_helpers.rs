// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The GPU copies of the functions GPU kernels call.

use std::collections::{HashMap, HashSet};

use crate::ast::types::TypeKind;
use crate::error::lowering::LoweringError;
use crate::mir::call_graph::CalleeWalk;
use crate::mir::gpu_names::GpuNames;
use crate::mir::symbol::Symbol;
use crate::mir::types::MirType;
use crate::mir::{Body, ExecutionModel, Operand, Rvalue, StatementKind, TerminatorKind};

/// Clone user functions that are transitively called from GPU kernels.
/// Each clone is marked as GpuDevice and has f32-narrowed types.
///
/// A clone keeps the symbol of the body it was cloned from, so afterwards a
/// helper's symbol names two bodies: the CPU original and its GpuDevice
/// clone. They are told apart by execution model, which is what every later
/// lookup by symbol selects on.
pub fn clone_gpu_device_helpers(
    bodies: &mut Vec<(Symbol, Body)>,
    gpu_names: &mut GpuNames,
) -> Result<(), LoweringError> {
    let body_of: HashMap<&Symbol, &Body> =
        bodies.iter().map(|(symbol, body)| (symbol, body)).collect();
    let kernels: HashSet<&Symbol> = bodies
        .iter()
        .filter(|(_, body)| body.execution_model == ExecutionModel::GpuKernel)
        .map(|(symbol, _)| symbol)
        .collect();
    if kernels.is_empty() {
        return Ok(());
    }

    let reachable: HashSet<&Symbol> = CalleeWalk::from(&body_of, kernels.iter().copied()).collect();
    let mut helpers_to_add = Vec::new();
    for (symbol, body) in bodies.iter() {
        if reachable.contains(symbol)
            && body.execution_model == ExecutionModel::Cpu
            && !kernels.contains(symbol)
        {
            let mut gpu_body = body.clone();
            gpu_body.execution_model = ExecutionModel::GpuDevice;
            narrow_float_types(&mut gpu_body);
            gpu_names.claim_helper(symbol, body.span)?;
            helpers_to_add.push((symbol.clone(), gpu_body));
        }
    }

    // A kernel that calls a (now f32-narrowed) GpuDevice helper must not widen
    // the f32 buffer values it passes to the helper's float params. Narrowing
    // the calling kernel's scalar f64 temps to f32 removes that spurious widen.
    // `narrow_type_kind` only touches scalar floats, so explicit `array<f64>`
    // buffers are left intact; f64-buffer kernels that call no helper are
    // unaffected because they never enter this set.
    let helpers = gpu_names.helpers();
    let kernels_calling_helpers: Vec<usize> = bodies
        .iter()
        .enumerate()
        .filter(|(_, (symbol, body))| {
            body.execution_model == ExecutionModel::GpuKernel
                && CalleeWalk::from(&body_of, [symbol]).any(|callee| helpers.contains(callee))
        })
        .map(|(index, _)| index)
        .collect();
    for index in kernels_calling_helpers {
        narrow_float_types(&mut bodies[index].1);
    }

    bodies.extend(helpers_to_add);
    Ok(())
}

/// Narrow all f64 (float) types in the body to f32 for GPU compatibility.
fn narrow_float_types(body: &mut Body) {
    for local_decl in &mut body.local_decls {
        narrow_type_kind(&mut local_decl.ty.kind);
        local_decl.mir_ty = MirType::from_type_kind(&local_decl.ty.kind);
    }
    // Constant operands carry their own type, which drives the WGSL literal
    // width (`2.0lf` for f64). Narrow them too, or an f64 literal survives
    // into an otherwise-f32 helper and trips the SHADER_F64 feature gate.
    for block in &mut body.basic_blocks {
        for stmt in &mut block.statements {
            if let StatementKind::Assign(_, rv) | StatementKind::Reassign(_, rv) = &mut stmt.kind {
                narrow_rvalue_floats(rv);
            }
        }
        if let Some(term) = &mut block.terminator {
            narrow_terminator_floats(&mut term.kind);
        }
    }
}

fn narrow_operand_floats(op: &mut Operand) {
    if let Operand::Constant(c) = op {
        narrow_type_kind(&mut c.ty.kind);
    }
}

fn narrow_rvalue_floats(rv: &mut Rvalue) {
    match rv {
        Rvalue::Use(o) => narrow_operand_floats(o),
        Rvalue::UnaryOp(_, o) => narrow_operand_floats(o),
        Rvalue::BinaryOp(_, a, b) => {
            narrow_operand_floats(a);
            narrow_operand_floats(b);
        }
        Rvalue::Cast(o, ty) => {
            narrow_operand_floats(o);
            narrow_type_kind(&mut ty.kind);
        }
        Rvalue::MathIntrinsic(_, args) | Rvalue::Aggregate(_, args) => {
            for a in args {
                narrow_operand_floats(a);
            }
        }
        Rvalue::Ref(_) | Rvalue::Len(_) | Rvalue::GpuIntrinsic(_) => {}
        Rvalue::AtomicOp {
            buffer,
            index,
            value,
            compare_expected,
            ..
        } => {
            narrow_operand_floats(buffer);
            narrow_operand_floats(index);
            narrow_operand_floats(value);
            if let Some(expected) = compare_expected {
                narrow_operand_floats(expected);
            }
        }
    }
}

fn narrow_terminator_floats(term: &mut TerminatorKind) {
    match term {
        TerminatorKind::SwitchInt { discr, .. } => narrow_operand_floats(discr),
        TerminatorKind::Call { func, args, .. } => {
            narrow_operand_floats(func);
            for a in args {
                narrow_operand_floats(a);
            }
        }
        TerminatorKind::VirtualCall { args, .. } => {
            for a in args {
                narrow_operand_floats(a);
            }
        }
        TerminatorKind::Goto { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::GpuLaunch { .. } => {}
    }
}

/// Narrow a scalar float type to `f32`; every other type is left as it is.
fn narrow_type_kind(kind: &mut TypeKind) {
    if matches!(kind, TypeKind::Float | TypeKind::F64) {
        *kind = TypeKind::F32;
    }
}
