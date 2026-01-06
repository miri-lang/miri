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

    // Find the main function body?
    // `lower_code` typically lowers the *last* function or the top-level statements?
    // `lower_code` in utils.rs usually wraps code in a function or lowers the whole thing.
    // Let's check `tests/mir/utils.rs` or how `lower_code` works.

    // Assuming `lower_code` lowers the top-level script or finds the main function.
    // Actually, `lower_code` in `tests/mir/utils.rs` usually returns the Body of the function definition found?
    // Inspecting `tests/mir/lowering/gpu.rs` uses `lower_code`.

    // If input has multiple functions, `lower_code` might just lower the first one or fail if it expects one.
    // Let's look at `lower_code` implementation if possible, or just assume it returns the body of the *last* function or strictly one function.

    // Update: `lower_code` parses and lowers. If I pass a full program with `main`, it might return `main`'s body if it's the last one.

    // In the test string above, `main` is second.

    let found_launch = body.basic_blocks.iter().any(|bb| {
        if let Some(terminator) = &bb.terminator {
            matches!(terminator.kind, TerminatorKind::GpuLaunch { .. })
        } else {
            false
        }
    });

    assert!(found_launch, "Expected TerminatorKind::GpuLaunch");
}
