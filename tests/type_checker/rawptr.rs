// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_test, type_checker_vars_type_test};
use miri::ast::factory::type_rawptr;

#[test]
fn test_rawptr_variable_declaration() {
    type_checker_test(
        "\
runtime \"core\" fn alloc(size int) RawPtr
let ptr = alloc(64)
",
    );
}

#[test]
fn test_rawptr_function_parameter_and_return() {
    type_checker_test(
        "\
runtime \"core\" fn alloc(size int) RawPtr
runtime \"core\" fn free(ptr RawPtr)
let ptr = alloc(128)
free(ptr)
",
    );
}

#[test]
fn test_rawptr_inferred_type() {
    type_checker_vars_type_test(
        "\
runtime \"core\" fn alloc(size int) RawPtr
let ptr = alloc(64)
",
        vec![("ptr", type_rawptr())],
    );
}

#[test]
fn test_rawptr_type_mismatch_string_to_rawptr() {
    type_checker_error_test(
        "\
runtime \"core\" fn free(ptr RawPtr)
let s = \"hello\"
free(s)
",
        "Type mismatch",
    );
}

#[test]
fn test_rawptr_type_mismatch_int_to_rawptr() {
    type_checker_error_test(
        "\
runtime \"core\" fn free(ptr RawPtr)
free(42)
",
        "Type mismatch",
    );
}

#[test]
fn test_rawptr_passed_between_runtime_functions() {
    type_checker_test(
        "\
runtime \"core\" fn alloc(size int) RawPtr
runtime \"core\" fn realloc(ptr RawPtr, new_size int) RawPtr
runtime \"core\" fn free(ptr RawPtr)
let ptr = alloc(64)
let new_ptr = realloc(ptr, 128)
free(new_ptr)
",
    );
}
