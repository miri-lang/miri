// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_gpu_launch_returns_future() {
    let input = "
gpu fn my_kernel()
    let x = 1

fn main()
    let k = my_kernel()
    let f = k.launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
";
    check_success(input);
}

#[test]
fn test_gpu_launch_invalid_args() {
    let input = "
gpu fn my_kernel()
    let x = 1

fn main()
    let k = my_kernel()
    k.launch(1, 2)
";
    check_error(
        input,
        "Type mismatch for argument 'grid': expected Dim3, got int",
    );
}

#[test]
fn test_gpu_launch_method_missing_on_non_kernel() {
    let input = "
fn not_kernel()
    return

fn main()
    let k = not_kernel()
    k.launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
";
    // Check for correct error message for non-kernel type.
    // If not_kernel returns Void, it has no members.
    check_error(input, "Type 'void' does not have members");
}

#[test]
fn test_await_gpu_launch() {
    let input = "
gpu fn my_kernel()
    let x = 1

async fn main()
    await my_kernel().launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
";
    check_success(input);
}
