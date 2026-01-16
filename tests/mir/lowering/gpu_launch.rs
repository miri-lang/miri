// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lowering_gpu_launch_test;

#[test]
fn test_gpu_launch_terminator() {
    mir_lowering_gpu_launch_test(
        "
gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
",
    );
}
