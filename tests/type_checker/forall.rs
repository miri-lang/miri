// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_bare_forall_is_rejected_with_inferred_device() {
    type_checker_error_test(
        r#"
forall i in 0..4
    let x = i
"#,
        "device inference for 'forall' is not yet supported; use 'gpu forall'",
    );
}

#[test]
fn test_gpu_forall_with_explicit_device_is_accepted() {
    type_checker_test(
        r#"
gpu forall i in 0..4
    let x = i
"#,
    );
}
