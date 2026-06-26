// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for GPU atomic operations in forall kernels.

use super::helpers::assert_gpu_wgsl_valid;

/// atomic_add on an Atomic<u32> buffer emits valid WGSL.
#[test]
fn atomic_add_u32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var hist = Array<Atomic<u32>, 4>()
    gpu forall i in 0..4
        atomic_add(hist, i, 1 as u32)
",
    );
}

/// atomic_add on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_add_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var hist = Array<Atomic<i32>, 4>()
    gpu forall i in 0..4
        atomic_add(hist, i, 1 as i32)
",
    );
}

/// atomic_sub on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_sub_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var counter = Array<Atomic<i32>, 1>()
    gpu forall i in 0..1
        atomic_sub(counter, 0, 1 as i32)
",
    );
}

/// atomic_max on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_max_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var max_val = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_max(max_val, 0, i as i32)
",
    );
}

/// atomic_min on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_min_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var min_val = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_min(min_val, 0, i as i32)
",
    );
}

/// atomic_and on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_and_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var mask = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_and(mask, 0, -3 as i32)
",
    );
}

/// atomic_or on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_or_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var flags = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_or(flags, 0, 1 as i32)
",
    );
}

/// atomic_xor on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_xor_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var state = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_xor(state, 0, 1 as i32)
",
    );
}

/// atomic_exchange on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_exchange_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var value = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_exchange(value, 0, 42 as i32)
",
    );
}

/// atomic_compare_exchange on an Atomic<i32> buffer emits valid WGSL.
#[test]
fn atomic_compare_exchange_i32_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var value = Array<Atomic<i32>, 1>()
    gpu forall i in 0..4
        atomic_compare_exchange(value, 0, 0 as i32, 1 as i32)
",
    );
}

/// Contended atomic counter: verify 147456 parallel increments produce correct result.
/// This test requires GPU hardware (Metal adapter).
#[cfg_attr(not(feature = "gpu_hardware"), ignore)]
#[test]
fn atomic_contended_counter_produces_correct_value() {
    use crate::integration::utils::assert_runs_with_output;
    assert_runs_with_output(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var hist = Array<Atomic<u32>, 1>()
    gpu forall i in 0..147456
        atomic_add(hist, 0, 1 as u32)
    let host = hist
    println(f\"counter={host[0]}\")
",
        "counter=147456",
    );
}

/// Atomic histogram: verify bucket accumulation with 256 atomic counters.
#[cfg_attr(not(feature = "gpu_hardware"), ignore)]
#[test]
fn atomic_histogram_accumulates_correctly() {
    use crate::integration::utils::assert_runs_with_output;
    assert_runs_with_output(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var hist = Array<Atomic<u32>, 256>()
    gpu forall i in 0..147456
        atomic_add(hist, i % 256, 1 as u32)
    let host = hist
    let bucket_0 = host[0]
    let bucket_1 = host[1]
    println(f\"bucket_0={bucket_0} bucket_1={bucket_1}\")
",
        "bucket_0=576 bucket_1=576",
    );
}

/// atomic_add on non-atomic buffer is rejected during code generation.
#[test]
fn atomic_add_on_plain_array_rejected() {
    use crate::integration::utils::assert_runtime_error;
    assert_runtime_error(
        "
use system.gpu
use system.gpu.atomic

fn main()
    gpu var plain_buf = Array<u32, 4>()
    gpu forall i in 0..4
        atomic_add(plain_buf, i, 1 as u32)
",
        "requires an Array<Atomic<u32|i32>, N> buffer",
    );
}

/// atomic_add outside a GPU kernel (in host code) is rejected.
#[test]
fn atomic_add_in_host_fn_rejected() {
    use crate::integration::utils::assert_runtime_error;
    assert_runtime_error(
        "
use system.gpu
use system.gpu.atomic

fn main()
    let hist = Array<Atomic<u32>, 4>()
    atomic_add(hist, 0, 1 as u32)
",
        "GPU-only",
    );
}
