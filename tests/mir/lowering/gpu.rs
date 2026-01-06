// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::{Dimension, GpuIntrinsic, Rvalue};

#[test]
fn test_gpu_function_flag() {
    let body = lower_code(
        "
gpu fn kernel()
    // empty
",
    );
    assert!(
        body.is_gpu(),
        "Expected is_gpu() to be true for gpu function"
    );
}

#[test]
fn test_normal_function_flag() {
    let body = lower_code(
        "
fn normal()
    // empty
",
    );
    assert!(
        !body.is_gpu(),
        "Expected is_gpu() to be false for normal function"
    );
}

#[test]
fn test_gpu_thread_idx_x() {
    let body = lower_code(
        "
    gpu fn main()
        let idx = gpu_context.thread_idx.x
",
    );
    assert!(body.is_gpu());

    let first_block = &body.basic_blocks[0];
    // Lowering produces GpuIntrinsic::ThreadIdx(Dimension::X)

    let found = first_block.statements.iter().any(|stmt| {
        if let miri::mir::StatementKind::Assign(_, rvalue) = &stmt.kind {
            matches!(
                rvalue,
                Rvalue::GpuIntrinsic(GpuIntrinsic::ThreadIdx(Dimension::X))
            )
        } else {
            false
        }
    });

    assert!(
        found,
        "Expected GpuIntrinsic::ThreadIdx(Dimension::X) assignment"
    );
}

#[test]
fn test_gpu_block_idx_all() {
    let body = lower_code(
        "
    gpu fn main()
        let x = gpu_context.block_idx.x
        let y = gpu_context.block_idx.y
        let z = gpu_context.block_idx.z
",
    );

    let mut found_x = false;
    let mut found_y = false;
    let mut found_z = false;

    for stmt in &body.basic_blocks[0].statements {
        if let miri::mir::StatementKind::Assign(_, rvalue) = &stmt.kind {
            match rvalue {
                Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::X)) => found_x = true,
                Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Y)) => found_y = true,
                Rvalue::GpuIntrinsic(GpuIntrinsic::BlockIdx(Dimension::Z)) => found_z = true,
                _ => {}
            }
        }
    }

    assert!(found_x, "Missing gpu_block_idx.x");
    assert!(found_y, "Missing gpu_block_idx.y");
    assert!(found_z, "Missing gpu_block_idx.z");
}
