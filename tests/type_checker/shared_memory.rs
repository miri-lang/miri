// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::{check_error, check_success};

#[test]
fn test_shared_memory_valid() {
    check_success(
        "
gpu fn kernel():
    shared cache [float; 256]
        ",
    );
}

#[test]
fn test_shared_memory_in_regular_fn() {
    check_error(
        "
fn plain():
    shared cache [float; 256]
        ",
        "Shared variables can only be declared inside 'gpu' functions",
    );
}

#[test]
fn test_shared_memory_non_array() {
    check_error(
        "
gpu fn kernel():
    shared cache float
        ",
        "Shared variable 'cache' must be an array, got float",
    );
}
