// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Float→int cast saturation parity between the host and GPU backends.
//!
//! The host lowers `x as int` through Cranelift's `fcvt_to_sint_sat`, which
//! saturates an out-of-range float to the destination integer's bounds
//! (`i64::MAX` / `i64::MIN`) rather than wrapping or producing an undefined
//! value. The WGSL backend lowers the same cast to `i32(x)`; on the GPU
//! `int` is a 32-bit `i32`, so the saturation bounds are narrower
//! (`i32::MAX` / `i32::MIN`), but the *behavior* must match: an out-of-range
//! float clamps to the destination bounds, it never wraps and is never
//! undefined.
//!
//! These tests pin that parity. The host case (already covered by
//! `math::integer_math` FIX 6) is asserted here alongside the GPU case so the
//! two backends are checked against the same input in one place.

use super::device::assert_gpu_runs_with_output;
use crate::integration::utils::assert_runs_with_output;

/// The GPU positive saturation ceiling, `0x7FFF_FF80` = 2³¹ − 128.
///
/// It is *not* `i32::MAX` (2147483647): that value is not representable in
/// `f32`, so naga/Metal clamps an out-of-range float to the largest `f32`
/// strictly below 2³¹ (whose `f32` quantum near 2³¹ is 128) before converting
/// to `i32`. The result is still saturating — clamped into `i32` range, never
/// wrapped — which is the parity invariant under test; the host reaches the
/// *exact* `i64::MAX` only because `fcvt_to_sint_sat` saturates to the true
/// integer bound rather than a float-representable one.
const I32_SAT_MAX: &str = "2147483520";
/// The GPU negative saturation floor. Unlike the ceiling this is *exact*
/// `i32::MIN` = −2³¹, because −2³¹ *is* representable in `f32`.
const I32_MIN: &str = "-2147483648";

/// i64::MAX as decimal — the host saturation ceiling (`int` is `i64` on host).
const I64_MAX: &str = "9223372036854775807";
/// i64::MIN as decimal — the host saturation floor.
const I64_MIN: &str = "-9223372036854775808";

/// Host: a float far above the integer range saturates to `i64::MAX`
/// (Cranelift `fcvt_to_sint_sat`), never wraps.
#[test]
fn host_cast_large_positive_float_saturates_to_max() {
    assert_runs_with_output(
        "
let x = 1.0e30
let result = x as int
println(f'{result}')
",
        I64_MAX,
    );
}

/// Host: a float far below the integer range saturates to `i64::MIN`.
#[test]
fn host_cast_large_negative_float_saturates_to_min() {
    assert_runs_with_output(
        "
let x = 0.0 - 1.0e30
let result = x as int
println(f'{result}')
",
        I64_MIN,
    );
}

/// GPU: floats far outside the `i32` range saturate (never wrap), matching the
/// host's saturating semantics at the narrower device width. The positive
/// ceiling lands on the largest `f32` below 2³¹ ([`I32_SAT_MAX`]) and the
/// negative floor on the exact `i32::MIN` — see the constant docs for why the
/// two ends differ.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_cast_out_of_range_floats_saturate_without_wrapping() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [1.0e30, 0.0 - 1.0e30]
gpu var dst = [0, 0]

gpu forall i in 0..2
    dst[i] = src[i] as int

let h = dst
println(f'{h[0]} {h[1]}')
";
    assert_gpu_runs_with_output(source, &format!("{I32_SAT_MAX} {I32_MIN}"));
}

/// GPU: an in-range float truncates toward zero (same rounding the host uses),
/// so only genuinely out-of-range inputs engage saturation.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_cast_in_range_float_truncates_toward_zero() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [2.7, 0.0 - 2.7]
gpu var dst = [0, 0]

gpu forall i in 0..2
    dst[i] = src[i] as int

let h = dst
println(f'{h[0]} {h[1]}')
";
    assert_gpu_runs_with_output(source, "2 -2");
}
