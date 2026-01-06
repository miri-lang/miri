// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::TerminatorKind;

#[test]
fn test_gpu_launch_terminator() {
    let body = lower_code(
        "
gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
    );

    let found_launch = body.basic_blocks.iter().any(|bb| {
        if let Some(terminator) = &bb.terminator {
            matches!(terminator.kind, TerminatorKind::GpuLaunch { .. })
        } else {
            false
        }
    });

    assert!(found_launch, "Expected TerminatorKind::GpuLaunch");
}
