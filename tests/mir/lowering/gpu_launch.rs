// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{has_gpu_launch, mir_lower_code};

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
