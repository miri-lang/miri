// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL backend shader-validity tests.
//!
//! Each test exercises a small `gpu for` kernel through the WGSL backend and
//! `naga`'s validator to confirm the shader source is syntactically and
//! type-correctly generated. These tests require no GPU hardware.
//!
//! Value-correctness testing (i.e., comparing computed values against
//! expected results) is owned by `super::launch` via the compiler-driven
//! native dispatch path.

use super::helpers::assert_gpu_wgsl_valid;

#[test]
fn vector_add_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
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
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [0.5, 1.5, 2.5, 3.5]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
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
    use super::super::utils::assert_compiler_error;
    use super::assert_gpu_wgsl_valid;

    #[test]
    fn int_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
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
    gpu let src = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
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
    gpu let a = [100, 200, 300, 400]
    gpu let b = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
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
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [0.5, 1.5, 2.5, 3.5]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
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
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu for i in 0..4
        dst[i] = a[i] * 2.5
",
        );
    }

    /// f64 literals (don't round-trip in f32) bind storage buffers as
    /// `array<f64>`. WGSL has no `enable` extension for 64-bit scalars —
    /// naga gates `f64` through validator `Capabilities::FLOAT64` and the
    /// device gates it through `Features::SHADER_F64`, so the shader source
    /// carries no directive. naga still rejects the kernel if the storage
    /// elements disagree on width.
    #[test]
    fn f64_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    gpu let b = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    gpu var dst = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
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

gpu var flags = [true, false, true, false]
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

gpu var labels = [\"a\", \"b\", \"c\", \"d\"]
gpu for i in 0..4
    let _ = labels[i]
",
            "not a valid WGSL storage-buffer element",
        );
    }
}
