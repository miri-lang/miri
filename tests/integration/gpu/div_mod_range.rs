// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Static range diagnostic for GPU `/` and `%` operands.
//!
//! On the GPU, Miri's `int` lowers to a 32-bit WGSL `i32`. A `/` or `%` operand
//! that is a compile-time integer literal outside the signed 32-bit range would
//! be silently narrowed to `i32` by the WGSL backend, so the kernel would divide
//! by a different value than the source spells. The type checker rejects such an
//! operand inside a `forall` (GPU) kernel with a cast/clamp suggestion, rather
//! than truncating silently.

use super::helpers::assert_gpu_wgsl_valid;
use crate::integration::utils::assert_compiler_error;

const OUT_OF_RANGE: &str = "outside the 32-bit range";

/// A divisor within `i32` range is representable on the GPU and accepted.
#[test]
fn div_by_in_range_literal_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / 2
",
    );
}

/// A modulo by a literal at the `i32` max boundary is still representable.
#[test]
fn mod_by_i32_max_literal_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i % 2147483647
",
    );
}

/// A divisor greater than `i32::MAX` (2^31) would be narrowed to a different
/// value; it is rejected with the out-of-range diagnostic.
#[test]
fn div_by_over_i32_max_literal_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / 5000000000
",
        OUT_OF_RANGE,
    );
}

/// A modulo by a literal above the `i32` range is rejected the same way.
#[test]
fn mod_by_over_i32_max_literal_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i % 5000000000
",
        OUT_OF_RANGE,
    );
}

/// The dividend is checked too: a numerator above the `i32` range is rejected.
#[test]
fn div_with_over_i32_max_dividend_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = 5000000000 / (i + 1)
",
        OUT_OF_RANGE,
    );
}

/// A negated literal at the `i32::MIN` boundary (`-2^31`) is representable and
/// must not be flagged, guarding against a magnitude-only false positive.
#[test]
fn div_by_i32_min_literal_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / -2147483648
",
    );
}

/// A negated literal below `i32::MIN` is rejected.
#[test]
fn div_by_below_i32_min_literal_is_rejected() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / -5000000000
",
        OUT_OF_RANGE,
    );
}

/// An out-of-range operand nested under an `if` in the kernel body is still
/// reached by the walk and rejected.
#[test]
fn div_out_of_range_under_conditional_is_checked() {
    assert_compiler_error(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        if i > 0
            out[i] = i / 5000000000
",
        OUT_OF_RANGE,
    );
}

/// A runtime divisor (not a literal) is undecidable here and must pass — the
/// check only rejects provably out-of-range literals.
#[test]
fn div_by_runtime_value_is_allowed() {
    assert_gpu_wgsl_valid(
        "
use system.gpu

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / (i + 2)
",
    );
}
