// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_shared_memory_valid() {
    type_checker_test(
        "
gpu fn kernel():
    shared cache [float; 256]
        ",
    );
}

#[test]
fn test_shared_memory_in_regular_fn() {
    type_checker_error_test(
        "
fn plain():
    shared cache [float; 256]
        ",
        "Shared variables can only be declared inside 'gpu' functions",
    );
}

#[test]
fn test_shared_memory_non_array() {
    type_checker_error_test(
        "
gpu fn kernel():
    shared cache float
        ",
        "Shared variable 'cache' must be an array, got float",
    );
}
