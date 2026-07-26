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
