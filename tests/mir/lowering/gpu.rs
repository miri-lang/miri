// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::{expect_assignment, lower_code};
use miri::mir::Rvalue;

#[test]
fn test_gpu_function_flag() {
    let body = lower_code(
        "
gpu fn kernel()
    // empty
",
    );
    assert!(body.is_gpu, "Expected is_gpu to be true for gpu function");
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
        !body.is_gpu,
        "Expected is_gpu to be false for normal function"
    );
}

#[test]
fn test_gpu_thread_idx_x() {
    let body = lower_code(
        "
fn gpu_thread_idx_x() int: 0
gpu fn main(): gpu_thread_idx_x()
",
    );
    assert!(body.is_gpu);

    let first_block = &body.basic_blocks[0];
    // We expect an assignment from Rvalue::GpuThreadIdx(0)
    // The expression statement lowering might assign to a temp
    let (_, rvalue) = expect_assignment(&first_block.statements[0]);

    if let Rvalue::GpuThreadIdx(dim) = rvalue {
        assert_eq!(*dim, 0);
    } else {
        panic!("Expected GpuThreadIdx(0), got {:?}", rvalue);
    }
}

#[test]
fn test_gpu_block_idx_all() {
    let body = lower_code(
        "
fn gpu_block_idx_x() int: 0
fn gpu_block_idx_y() int: 0
fn gpu_block_idx_z() int: 0

gpu fn main()
    let x = gpu_block_idx_x()
    let y = gpu_block_idx_y()
    let z = gpu_block_idx_z()
",
    );

    // Block 0 should have assignments for x, y, z
    // Note: lower_code might produce intermediate temps.
    // We can iterate statements and look for the Rvalues.

    let mut found_x = false;
    let mut found_y = false;
    let mut found_z = false;

    for stmt in &body.basic_blocks[0].statements {
        if let miri::mir::StatementKind::Assign(_, rvalue) = &stmt.kind {
            match rvalue {
                Rvalue::GpuBlockIdx(0) => found_x = true,
                Rvalue::GpuBlockIdx(1) => found_y = true,
                Rvalue::GpuBlockIdx(2) => found_z = true,
                _ => {}
            }
        }
    }

    assert!(found_x, "Missing gpu_block_idx_x");
    assert!(found_y, "Missing gpu_block_idx_y");
    assert!(found_z, "Missing gpu_block_idx_z");
}
