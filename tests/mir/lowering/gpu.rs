// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{has_gpu_intrinsic, mir_lower_code};
use miri::mir::{Dimension, GpuIntrinsic};

fn assert_gpu_intrinsics(source: &str, expected: &[GpuIntrinsic]) {
    let body = mir_lower_code(source);
    for intrinsic in expected {
        assert!(
            has_gpu_intrinsic(&body, intrinsic),
            "Expected {:?} GPU intrinsic in MIR for source:\n{}",
            intrinsic,
            source
        );
    }
}

#[test]
fn test_gpu_function_flag() {
    let body = mir_lower_code(
        "
gpu fn kernel()
    // empty
",
    );
    assert!(body.is_gpu());
}

#[test]
fn test_normal_function_flag() {
    let body = mir_lower_code(
        "
fn normal()
    // empty
",
    );
    assert!(!body.is_gpu());
}

#[test]
fn test_gpu_thread_idx_x() {
    let source = "
    gpu fn main()
        let idx = kernel.thread_idx.x
";
    assert!(mir_lower_code(source).is_gpu());
    assert_gpu_intrinsics(source, &[GpuIntrinsic::ThreadIdx(Dimension::X)]);
}

#[test]
fn test_gpu_block_idx_all() {
    assert_gpu_intrinsics(
        "
    gpu fn main()
        let x = kernel.block_idx.x
        let y = kernel.block_idx.y
        let z = kernel.block_idx.z
",
        &[
            GpuIntrinsic::BlockIdx(Dimension::X),
            GpuIntrinsic::BlockIdx(Dimension::Y),
            GpuIntrinsic::BlockIdx(Dimension::Z),
        ],
    );
}

#[test]
fn test_gpu_context_alias_lowers_to_intrinsic() {
    assert_gpu_intrinsics(
        "
    gpu fn main()
        let idx = gpu_context.thread_idx.x
",
        &[GpuIntrinsic::ThreadIdx(Dimension::X)],
    );
}

#[test]
fn test_kernel_block_dim_and_grid_dim() {
    assert_gpu_intrinsics(
        "
    gpu fn main()
        let bd = kernel.block_dim.x
        let gd = kernel.grid_dim.y
",
        &[
            GpuIntrinsic::BlockDim(Dimension::X),
            GpuIntrinsic::GridDim(Dimension::Y),
        ],
    );
}

#[test]
fn test_kernel_global_idx_all_dimensions() {
    assert_gpu_intrinsics(
        "
    gpu fn main()
        let gx = kernel.global_idx.x
        let gy = kernel.global_idx.y
        let gz = kernel.global_idx.z
",
        &[
            GpuIntrinsic::GlobalIdx(Dimension::X),
            GpuIntrinsic::GlobalIdx(Dimension::Y),
            GpuIntrinsic::GlobalIdx(Dimension::Z),
        ],
    );
}

#[test]
fn a_gpu_fn_out_buffer_binds_read_write_and_stays_out() {
    let body = mir_lower_code(
        "
use system.collections.array

gpu fn fill(src Array<int, 4>, dst out Array<int, 4>)
    let i = kernel.thread_idx.x
    dst[i] = src[i]
",
    );
    assert_eq!(body.signature.out_params, vec![false, true]);
    assert_eq!(body.signature.storage_read_write, vec![false, true]);
}

/// A buffer of atomics binds read-write whatever the kernel does with it:
/// WGSL holds atomic storage only in a read-write binding.
#[test]
fn a_gpu_fn_atomic_buffer_binds_read_write() {
    let body = mir_lower_code(
        "
use system.gpu
use system.gpu.atomic
use system.collections.array

gpu fn count(src Array<int, 4>, hist Array<Atomic<u32>, 4>)
    let i = kernel.thread_idx.x
    atomic_add(hist, src[i], 1 as u32)
",
    );
    assert_eq!(body.signature.storage_read_write, vec![false, true]);
}

/// Only a buffer binds storage, so a scalar `out` parameter, a uniform, is
/// never marked read-write.
#[test]
fn a_gpu_fn_scalar_out_parameter_binds_no_storage() {
    let body = mir_lower_code(
        "
use system.collections.array

gpu fn fill(dst out Array<int, 4>, n out int)
    let i = kernel.thread_idx.x
    dst[i] = n
",
    );
    assert_eq!(body.signature.out_params, vec![true, true]);
    assert_eq!(body.signature.storage_read_write, vec![true, false]);
}
