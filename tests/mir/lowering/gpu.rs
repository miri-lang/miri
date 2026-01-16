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
        let idx = gpu_context.thread_idx.x
";
    mir_lowering_gpu_flag_test(source, true);
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::ThreadIdx(Dimension::X));
}

#[test]
fn test_gpu_block_idx_all() {
    let source = "
    gpu fn main()
        let x = gpu_context.block_idx.x
        let y = gpu_context.block_idx.y
        let z = gpu_context.block_idx.z
";
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockIdx(Dimension::X));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockIdx(Dimension::Y));
    mir_lowering_gpu_intrinsic_test(source, GpuIntrinsic::BlockIdx(Dimension::Z));
}
