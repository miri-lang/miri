// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{has_gpu_launch, mir_lower_code};
use miri::mir::TerminatorKind;

#[test]
fn test_gpu_launch_terminator() {
    let body = mir_lower_code(
        "
gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
    );
    assert!(has_gpu_launch(&body));
}

fn literal_grids(source: &str) -> Vec<Option<[u32; 3]>> {
    let body = mir_lower_code(source);
    (body.basic_blocks.iter())
        .filter_map(|block| match &block.terminator.as_ref()?.kind {
            TerminatorKind::GpuLaunch { literal_grid, .. } => Some(*literal_grid),
            _ => None,
        })
        .collect()
}

#[test]
fn test_gpu_launch_carries_its_literal_grid() {
    let grids = literal_grids(
        "
gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(4, 2, 1), Dim3(1, 1, 1))
",
    );
    assert_eq!(grids, vec![Some([4, 2, 1])]);
}

#[test]
fn test_gpu_launch_with_a_computed_grid_carries_none() {
    let grids = literal_grids(
        "
gpu fn my_kernel()
    let x = 1

fn main()
    let n = 4
    my_kernel().launch(Dim3(n, 1, 1), Dim3(1, 1, 1))
",
    );
    assert_eq!(grids, vec![None]);
}
