// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU tests for `system.math` functions used inside kernels: the transcendental
//! intrinsics (`tanh`), composed activations (`sigmoid`), and the procedural
//! noise stack (`hash_u32`, `value_noise`, `fbm`, `curl_noise_*`). Every value
//! test runs on Metal when an adapter is present, so a passing value check also
//! proves the emitted WGSL is valid and matches the CPU reference.

use super::device::assert_gpu_runs_with_output;
use super::helpers::assert_gpu_wgsl_valid;
use super::utils::*;

/// A plain Miri `fn` is callable inside a kernel and value-verifies.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_float_helper_kernel_runs() {
    let source = "
use system.gpu
use system.collections.array

fn my_double(x float) float: x * 2.0

fn main()
    gpu let src = [1.0, 2.0, 3.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = my_double(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    assert_gpu_runs_with_output(source, "2.0 4.0 6.0");
}

/// Helper function call inside a kernel emits naga-valid WGSL.
#[test]
fn gpu_helper_function_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn my_double(x int) int: x * 2

fn main()
    gpu let src = [1, 2, 3]
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        dst[i] = my_double(src[i])
",
    );
}

/// `tanh` value-verifies on the GPU against its CPU reference (tanh(0) = 0).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_tanh_kernel_value_correct() {
    let source = "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu let src = [0.0, 1.0, -1.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = tanh(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    // tanh(0)=0, tanh(1)≈0.7615942, tanh(-1)≈-0.7615942.
    assert_gpu_runs_with_output(source, "0.0 0.761594");
}

#[test]
fn gpu_tanh_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu let src = [0.0, 1.0, -1.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = tanh(src[i])
",
    );
}

/// `sigmoid` (composed in stdlib from `exp`) runs inside a kernel; sigmoid(0)=0.5.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_sigmoid_kernel_value_correct() {
    let source = "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu let src = [0.0, 1.0, 2.0]
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = sigmoid(src[i])
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    // sigmoid(0)=0.5, sigmoid(1)≈0.7310586, sigmoid(2)≈0.8807971.
    assert_gpu_runs_with_output(source, "0.5 0.731058");
}

/// `hash_u32` (Murmur3 fmix32) is deterministic and portable: the GPU value
/// matches the CPU reference. hash_u32(1) normalized = 0.6837202.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_hash_u32_value_matches_cpu() {
    let source = "
use system.gpu
use system.math
use system.collections.array

fn unit(s u32) float
    return hash_u32(s) as float / 4294967295.0

fn main()
    gpu var dst = [0.0, 0.0]
    gpu forall i in 0..2
        dst[i] = unit((i + 1) as u32)
    let host = dst
    println(f'{host[0]} {host[1]}')
";
    // CPU reference: unit(1)=0.6837202, unit(2)=0.1279962.
    assert_gpu_runs_with_output(source, "0.683720");
}

/// `value_noise` stays in [0, 1) on the GPU (the unit-range guard returns 1.0).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_value_noise_in_unit_range() {
    let source = "
use system.gpu
use system.math
use system.collections.array

fn in_unit(v float) float: 1.0 if v >= 0.0 and v < 1.0 else 0.0

fn main()
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = in_unit(value_noise(i as float * 0.7, 1.3))
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]}')
";
    assert_gpu_runs_with_output(source, "1.0 1.0 1.0");
}

/// `value_noise` is deterministic on the GPU (same coordinate → same value).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_value_noise_is_deterministic() {
    let source = "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu var dst = [9.0, 9.0]
    gpu forall i in 0..2
        dst[i] = value_noise(2.2, 3.3) - value_noise(2.2, 3.3)
    let host = dst
    println(f'{host[0]} {host[1]}')
";
    assert_gpu_runs_with_output(source, "0.0 0.0");
}

/// `value_noise` emits naga-valid WGSL through the real pipeline (helpers
/// `floor`, `hash_u32`, `lattice_unit` all resolve in the kernel module).
#[test]
fn gpu_value_noise_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu var dst = [0.0, 0.0, 0.0]
    gpu forall i in 0..3
        dst[i] = value_noise(0.5, 0.5)
",
    );
}

/// `fbm` and the `curl_noise_*` field compose multiple noise octaves and emit
/// naga-valid WGSL.
#[test]
fn gpu_fbm_and_curl_emit_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu var dst = [0.0, 0.0]
    gpu forall i in 0..2
        dst[i] = fbm(1.0, 2.0) + curl_noise_x(1.0, 2.0) + curl_noise_y(1.0, 2.0)
",
    );
}

/// The curl field is deterministic on the GPU.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_curl_noise_is_deterministic() {
    let source = "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu var dst = [7.0, 7.0]
    gpu forall i in 0..2
        dst[i] = curl_noise_x(1.5, 2.5) - curl_noise_x(1.5, 2.5)
    let host = dst
    println(f'{host[0]} {host[1]}')
";
    assert_gpu_runs_with_output(source, "0.0 0.0");
}
