// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for i32 range validation during GPU i64→i32 upload narrowing.
//!
//! The compiler ensures Array<int, N> elements falling outside i32 range are
//! detected at upload time (hard error), preventing silent truncation. Tests cover:
//! - In-range round-trip (boundary values 2147483647 / -2147483648)
//! - Out-of-range runtime error (non-literal, avoiding compile-time rejection)
//! - Out-of-range compile error (literal array with out-of-range elements)

use super::device::gpu_adapter_available;
use super::utils::{assert_compiler_error, assert_runs_with_output, assert_runtime_error};

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i32_range_inrange_roundtrip() {
    if !gpu_adapter_available() {
        return;
    }

    let code = "
use system.gpu
use system.io
use system.collections.array

gpu let probe_max = [2147483647]
gpu let probe_min = [-2147483648]
gpu var probe_dst_max = [0]
gpu var probe_dst_min = [0]

gpu forall i in 0..1
    probe_dst_max[i] = probe_max[i]
    probe_dst_min[i] = probe_min[i]

let max_host = probe_dst_max
let min_host = probe_dst_min
println(f'{max_host[0]} {min_host[0]}')
";
    assert_runs_with_output(code, "2147483647 -2147483648");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i32_range_outofrange_runtime_error() {
    if !gpu_adapter_available() {
        return;
    }

    let code = "
use system.gpu
use system.io
use system.collections.array

let overflow_val = 2147483647 + 1
gpu let probe_overflow = [overflow_val]
gpu var probe_dst = [0]

gpu forall i in 0..1
    probe_dst[i] = probe_overflow[i]

let result = probe_dst
println(f'{result[0]}')
";
    assert_runtime_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_i32_range_outofrange_compile_error_positive() {
    let code = "
use system.gpu

gpu let arr = [2147483648]
";
    assert_compiler_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_i32_range_outofrange_compile_error_negative() {
    let code = "
use system.gpu

gpu let arr = [-2147483649]
";
    assert_compiler_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_i32_range_inrange_compile_accepts_max() {
    let code = "
use system.gpu

gpu let arr = [2147483647]
";
    // This should type-check and compile successfully.
    // Just ensure it doesn't error.
    super::utils::assert_type_checks(code);
}

#[test]
fn test_gpu_i32_range_inrange_compile_accepts_min() {
    let code = "
use system.gpu

gpu let arr = [-2147483648]
";
    // This should type-check and compile successfully.
    super::utils::assert_type_checks(code);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i32_range_negative_overflow_runtime_error() {
    if !gpu_adapter_available() {
        return;
    }

    let code = "
use system.gpu
use system.io
use system.collections.array

let negative_overflow_val = -2147483648 - 1
gpu let probe_negative_overflow = [negative_overflow_val]
gpu var probe_dst = [0]

gpu forall i in 0..1
    probe_dst[i] = probe_negative_overflow[i]

let result = probe_dst
println(f'{result[0]}')
";
    assert_runtime_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_i32_range_multi_element_compile_error() {
    let code = "
use system.gpu

gpu let arr = [1, 2147483648, 3]
";
    // The error should cite the element index (1) and the out-of-range value.
    // We check for both the index and the value to confirm the diagnostic is specific.
    assert_compiler_error(code, "element 1");
    assert_compiler_error(code, "2147483648");
}

#[test]
fn test_gpu_i32_range_host_array_capture_rejected_by_type_checker() {
    // Host-resident arrays cannot be captured in `forall` loops; the type
    // checker enforces residency visibility. Once transient capture support
    // is added to the compiler, this test can evolve to runtime-test the
    // upload validation path (new_storage_buffer_with_upload with i32 narrowing)
    // for non-persistent bindings. For now, confirm the diagnostic fires.
    let code = "
use system.gpu
use system.collections.array

let overflow_val = 2147483647 + 1
var data = [overflow_val]

gpu forall i in 0..1
    data[i] = data[i]
";
    assert_compiler_error(code, "must be gpu-resident");
}

#[test]
fn test_gpu_i32_range_reassign_whole_array_outofrange() {
    let code = "
use system.gpu

gpu var g = [1, 2, 3]
g = [2147483648, 0, 0]
";
    // Whole-array reassignment with out-of-range literal should compile-error.
    assert_compiler_error(code, "exceeds i32 range");
}

#[test]
fn test_gpu_i32_range_reassign_whole_array_inrange() {
    let code = "
use system.gpu

gpu var g = [1, 2, 3]
g = [2147483647, -2147483648, 0]
";
    // Whole-array reassignment with in-range values should type-check and compile.
    super::utils::assert_type_checks(code);
}
