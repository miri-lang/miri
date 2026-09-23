// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The device's integer rule, applied to kernel code (`gpu fn` bodies and
//! GPU `forall` / `gpu for` bodies).
//!
//! Every device integer is a 32-bit lane — signed or unsigned, as
//! [`crate::ast::gpu_wire`] decides for its type. An integer literal is judged
//! against the lane its recorded type occupies, and kernel code may not spell
//! `i64` or `u64`: the device would carry that arithmetic out at 32 bits, so
//! the program would silently compute something other than what it asked for.
//! A 64-bit value that *reaches* a kernel — a buffer element or a captured
//! host scalar — is range-checked into its lane at the host/device boundary,
//! which is the contract `int` already has, so reading and computing with it
//! stays allowed.

use crate::ast::gpu_wire::{device_scalar, DeviceScalar};
use crate::ast::types::TypeKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::expressions::literals::DeferredIntLiteralRange;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Holds a kernel-code integer literal that may not fit its 32-bit lane
    /// until the type it was written into is recorded.
    ///
    /// Every value up to `i32::MAX` fits either lane, so only a larger one is
    /// held; the lane itself is known only once the literal's type is final.
    pub(crate) fn hold_gpu_int_literal_range(&mut self, expr_id: usize, value: i128, span: Span) {
        if value <= i128::from(i32::MAX) {
            return;
        }
        self.deferred_gpu_int_literal_ranges
            .push(DeferredIntLiteralRange {
                expr_id,
                value,
                span,
            });
    }

    /// Reports every held kernel-code integer literal that does not fit the
    /// 32-bit lane its recorded type occupies on the device.
    pub(crate) fn report_deferred_gpu_int_literal_ranges(&mut self) {
        for held in std::mem::take(&mut self.deferred_gpu_int_literal_ranges) {
            let lane = self
                .type_table
                .types
                .get(&held.expr_id)
                .map_or(Some(DeviceScalar::I32), |ty| device_scalar(&ty.kind));
            let negated = self.negated_int_literals.contains(&held.expr_id);
            let Some(message) = lane_overflow_message(lane, held.value, negated) else {
                continue;
            };
            self.report_error(DiagnosticCode::TarGpuValueOutOfRange, message, held.span);
        }
    }

    /// Refuses a 64-bit integer type spelled in kernel code — a local's
    /// annotation, a scalar `gpu fn` parameter, or a cast target — whose
    /// arithmetic the device would silently carry out at 32 bits.
    pub(crate) fn reject_device_wide_integer(&mut self, kind: &TypeKind, span: Span) {
        let (name, narrow) = if matches!(kind, TypeKind::I64) {
            ("i64", "i32")
        } else if matches!(kind, TypeKind::U64) {
            ("u64", "u32")
        } else {
            return;
        };
        self.report_error_with_help(
            DiagnosticCode::TarGpuCodeRestriction,
            format!(
                "'{name}' is not supported in device code: device integers are 32-bit, so its \
                 arithmetic would silently wrap at 32 bits"
            ),
            span,
            format!(
                "use '{narrow}' (or 'int', the device's 32-bit default); a 64-bit buffer or \
                 captured value may still be read and written, and is range-checked into 32 \
                 bits when it crosses to the device"
            ),
        );
    }
}

/// The diagnostic for an integer literal past the top of `lane`, or `None`
/// when it fits (or the lane is not an integer lane). A literal directly under
/// a negation may reach one past the signed maximum, since `i32::MIN` can only
/// be spelled `-2147483648`.
fn lane_overflow_message(lane: Option<DeviceScalar>, value: i128, negated: bool) -> Option<String> {
    let (signedness, name, min, max) = match lane? {
        DeviceScalar::I32 => ("signed", "i32", i128::from(i32::MIN), i128::from(i32::MAX)),
        DeviceScalar::U32 => ("unsigned", "u32", 0, i128::from(u32::MAX)),
        DeviceScalar::F16 | DeviceScalar::F32 | DeviceScalar::F64 => return None,
    };
    let limit = if negated && lane == Some(DeviceScalar::I32) {
        max + 1
    } else {
        max
    };
    (value > limit).then(|| {
        format!(
            "Integer literal '{value}' is out of range for GPU 32-bit {signedness} integer \
             ({name} range is {min} to {max})"
        )
    })
}
