// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL backend value-correctness tests (M6.5 Task 7).
//!
//! Each test exercises a small `gpu for` kernel through the WGSL backend and
//! `naga`'s validator. Compute tests additionally dispatch the kernel via
//! `wgpu` directly from the test harness (bypassing the Cranelift native
//! dispatch path); when no adapter is available (bare CI without a software
//! fallback) the dispatch is skipped without failing the suite.
//!
//! The host-side scalar type is `i64` to match the WGSL mapping for
//! Miri's `int` (see `src/codegen/wgsl/types.rs::scalar`). Capture order is
//! the textual discovery order of outer identifiers in the kernel body:
//! the first identifier mentioned in the body becomes binding 0, the next
//! new identifier becomes binding 1, and so on.
//!
//! Value-correctness for `vector_add` and `scalar_multiply` now lives in
//! `super::launch` (compiler end-to-end), since the WGSL backend maps
//! `int` to WGSL `i64` so host and device buffer widths align. Remaining
//! compute tests in this file exercise kernel shapes whose native-dispatch
//! variants are still being designed (PLAN M6.5 task "Helper-shrink").

use super::helpers::{assert_gpu_compute_i64, assert_gpu_wgsl_valid};

#[test]
fn vector_add_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3, 4]
    let b = [10, 20, 30, 40]
    var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
}

#[test]
fn elementwise_madd_compute_matches_expected() {
    // dst[i] = a[i] * b[i] + c[i]
    // Capture order: dst, a, b, c.
    assert_gpu_compute_i64(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3, 4]
    let b = [10, 20, 30, 40]
    let c = [100, 200, 300, 400]
    var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] * b[i] + c[i]
",
        &[
            &[0, 0, 0, 0],
            &[1, 2, 3, 4],
            &[10, 20, 30, 40],
            &[100, 200, 300, 400],
        ],
        &[
            // 1*10+100=110, 2*20+200=240, 3*30+300=390, 4*40+400=560
            &[110, 240, 390, 560],
            &[1, 2, 3, 4],
            &[10, 20, 30, 40],
            &[100, 200, 300, 400],
        ],
    );
}

#[test]
fn bounds_check_skips_threads_past_range_end() {
    // Range `0..7` against the synthesized `@workgroup_size(256, 1, 1)`
    // kernel means threads 7..255 must hit the WGSL bounds guard and
    // skip the body. Buffer length is 8 with sentinel 999 in the last
    // slot — if the guard is missing, thread 7 would overwrite it.
    assert_gpu_compute_i64(
        "
use system.gpu
use system.collections.array

fn main()
    var dst = [999, 999, 999, 999, 999, 999, 999, 999]
    gpu for i in 0..7
        dst[i] = i + 100
",
        &[&[999, 999, 999, 999, 999, 999, 999, 999]],
        &[&[100, 101, 102, 103, 104, 105, 106, 999]],
    );
}

#[test]
fn reduction_fixed_sum_writes_single_total() {
    // Single-thread reduction (range `0..1`) computes a fixed-size sum
    // on the GPU. Captures: dst, a.
    assert_gpu_compute_i64(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3, 4, 5, 6, 7, 8]
    var dst = [0, 0, 0, 0]
    gpu for i in 0..1
        dst[0] = a[0] + a[1] + a[2] + a[3] + a[4] + a[5] + a[6] + a[7]
",
        &[&[0, 0, 0, 0], &[1, 2, 3, 4, 5, 6, 7, 8]],
        &[&[36, 0, 0, 0], &[1, 2, 3, 4, 5, 6, 7, 8]],
    );
}

#[test]
fn float_kernel_emits_naga_valid_wgsl() {
    // F32 arithmetic in a `gpu for` body. Naga rejects type or alignment
    // mistakes in float storage bindings, so this exercises the F32
    // half of the scalar mapping independently of the integer paths.
    // Float literals that round-trip in f32 are typed as `Array<f32, N>`
    // by the parser (see project memory: "Float layout in collections"),
    // so the unannotated literal binds the kernel buffers as f32 storage.
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn main()
    let a = [1.0, 2.0, 3.0, 4.0]
    let b = [0.5, 1.5, 2.5, 3.5]
    var dst = [0.0, 0.0, 0.0, 0.0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
}

/// Per-(scalar, op-kind) coverage grid for the WGSL backend.
///
/// For every Miri primitive that today round-trips into a WGSL storage buffer
/// element width, there is at least one `gpu for` kernel exercising the
/// scalar against a representative arithmetic op. For every primitive the
/// backend cannot represent today (bool buffers, heap-only types), there is
/// an `assert_compiler_error` proving the diagnostic fires before MIR
/// lowering — not as a naga validation failure on emitted shader source.
mod types {
    use super::{super::utils::assert_compiler_error, assert_gpu_wgsl_valid};

    #[test]
    fn int_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let a = [1, 2, 3, 4]
    let b = [10, 20, 30, 40]
    var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
        );
    }

    #[test]
    fn int_buffer_mul_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let src = [1, 2, 3, 4]
    var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = src[i] * 7
",
        );
    }

    #[test]
    fn int_buffer_sub_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let a = [100, 200, 300, 400]
    let b = [1, 2, 3, 4]
    var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] - b[i]
",
        );
    }

    #[test]
    fn f32_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let a = [1.0, 2.0, 3.0, 4.0]
    let b = [0.5, 1.5, 2.5, 3.5]
    var dst = [0.0, 0.0, 0.0, 0.0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
        );
    }

    #[test]
    fn f32_buffer_mul_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let a = [1.0, 2.0, 3.0, 4.0]
    var dst = [0.0, 0.0, 0.0, 0.0]
    gpu for i in 0..4
        dst[i] = a[i] * 2.5
",
        );
    }

    /// f64 literals (don't round-trip in f32) bind storage buffers as
    /// `array<f64>`. Requires the `enable shader_f64;` directive — naga
    /// rejects the kernel if the directive is missing or the elements
    /// disagree on width.
    #[test]
    fn f64_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let a = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    let b = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    var dst = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
        );
    }

    /// WGSL forbids `bool` in `var<storage>` bindings, so a captured
    /// `[true, false, ...]` array is rejected with a single source-cited
    /// diagnostic before MIR lowering.
    #[test]
    fn bool_storage_buffer_is_rejected_before_mir() {
        assert_compiler_error(
            "
use system.gpu
use system.collections.array

var flags = [true, false, true, false]
gpu for i in 0..4
    flags[i] = not flags[i]
",
            "not a valid WGSL storage-buffer element",
        );
    }

    /// String buffers have no storage layout on the device. Rejection
    /// reaches the user as the same buffer-element diagnostic class so
    /// the failure surface is uniform across non-mappable scalars.
    #[test]
    fn string_storage_buffer_is_rejected_before_mir() {
        assert_compiler_error(
            "
use system.gpu
use system.collections.array

var labels = [\"a\", \"b\", \"c\", \"d\"]
gpu for i in 0..4
    let _ = labels[i]
",
            "not a valid WGSL storage-buffer element",
        );
    }
}
