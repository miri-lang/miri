// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lowering_gpu_flag_test, mir_lowering_gpu_intrinsic_test};
use miri::mir::{Dimension, GpuIntrinsic};

#[test]
fn test_gpu_function_flag() {
    mir_lowering_gpu_flag_test(
        "
gpu fn kernel()
    // empty
",
        true,
    );
}

#[test]
fn test_normal_function_flag() {
    mir_lowering_gpu_flag_test(
        "
fn normal()
    // empty
",
        false,
    );
}

#[test]
fn test_gpu_thread_idx_x() {
    let source = "
    gpu fn main()
        let idx = kernel.thread_idx.x
";
    mir_lowering_gpu_flag_test(source, true);
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::ThreadIdx(Dimension::X));
}

#[test]
fn test_gpu_block_idx_all() {
    let source = "
    gpu fn main()
        let x = kernel.block_idx.x
        let y = kernel.block_idx.y
        let z = kernel.block_idx.z
";
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockIdx(Dimension::X));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockIdx(Dimension::Y));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockIdx(Dimension::Z));
}

#[test]
fn test_gpu_context_alias_lowers_to_intrinsic() {
    let source = "
    gpu fn main()
        let idx = gpu_context.thread_idx.x
";
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::ThreadIdx(Dimension::X));
}

#[test]
fn test_kernel_block_dim_and_grid_dim() {
    let source = "
    gpu fn main()
        let bd = kernel.block_dim.x
        let gd = kernel.grid_dim.y
";
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockDim(Dimension::X));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::GridDim(Dimension::Y));
}

#[test]
fn test_kernel_global_idx_all_dimensions() {
    let source = "
    gpu fn main()
        let gx = kernel.global_idx.x
        let gy = kernel.global_idx.y
        let gz = kernel.global_idx.z
";
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::GlobalIdx(Dimension::X));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::GlobalIdx(Dimension::Y));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::GlobalIdx(Dimension::Z));
}
