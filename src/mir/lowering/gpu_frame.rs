// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering for `gpu frame <ident> in <range>` loops.
//!
//! A `gpu frame` loop is a variant of `gpu for` that:
//! - Reads from exactly ONE gpu-resident buffer (read-only).
//! - Writes to exactly ONE gpu-resident buffer (read-write).
//! - Synthesizes a kernel marked with `is_frame_step=true` for animation drivers.
//!
//! The lowering reuses `gpu for` infrastructure and marks the result with
//! the frame-step flag in the GPU metadata.

use crate::ast::expression::Expression;
use crate::ast::statement::{Statement, VariableDeclaration};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;

use super::context::LoweringContext;
use super::gpu_for;

/// Lowers a `gpu frame` loop into a synthesized kernel + `GpuLaunch`.
///
/// Delegates to the gpu_for lowering and then marks the generated kernel
/// with `is_frame_step=true` in its GPU metadata.
pub fn lower_gpu_frame(
    ctx: &mut LoweringContext,
    span: &Span,
    stmt_id: usize,
    decls: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
) -> Result<(), LoweringError> {
    // Get the lambda body count before lowering so we can find the generated kernel.
    let body_count_before = ctx.lambda_bodies.len();

    // Delegate to gpu_for 1D lowering (frame must have exactly 1 loop var).
    gpu_for::lower_gpu_for(ctx, span, stmt_id, decls, iterable, body)?;

    // Mark the newly-generated kernel with is_frame_step=true.
    // gpu_for always generates exactly one kernel per call, so we find it
    // in the lambda_bodies list.
    if ctx.lambda_bodies.len() > body_count_before {
        let kernel_idx = ctx.lambda_bodies.len() - 1;
        if let Some(backend_meta) = &mut ctx.lambda_bodies[kernel_idx].body.backend_metadata {
            let crate::mir::BackendMetadata::Gpu(gpu_meta) = backend_meta;
            gpu_meta.is_frame_step = true;
        }
    }

    Ok(())
}
