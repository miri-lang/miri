// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

// ==================== Basic declarations ====================

#[test]
fn runtime_function_no_params_no_return() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_noop()
",
    );
}

#[test]
fn runtime_function_with_return_type() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_alloc(size int) i64
",
    );
}

#[test]
fn runtime_function_with_params_and_return() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_string_from_raw(data i64, len int) i64
",
    );
}

#[test]
fn runtime_function_void_return() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_free(ptr i64)
",
    );
}

// ==================== Explicit runtime name ====================

#[test]
fn runtime_function_explicit_core() {
    type_checker_test(
        r#"
fn main()
    runtime "core" fn miri_rt_string_len(ptr i64) int
"#,
    );
}

// ==================== Multiple declarations ====================

#[test]
fn multiple_runtime_functions() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_alloc(size int) i64
    runtime fn miri_rt_free(ptr i64)
    runtime fn miri_rt_string_new() i64
",
    );
}

// ==================== Scope registration ====================

#[test]
fn runtime_function_callable_in_scope() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_string_new() i64
    let ptr = miri_rt_string_new()
",
    );
}

#[test]
fn runtime_function_callable_with_args() {
    type_checker_test(
        "
fn main()
    runtime fn miri_rt_string_len(ptr i64) int
    let ptr i64 = 0
    let len = miri_rt_string_len(ptr)
",
    );
}

// ==================== Error cases ====================
// Note: Unknown runtime names (e.g. "gpu") are caught by the parser
// as SyntaxError::UnknownRuntime. Those are tested in parser tests.

#[test]
fn runtime_function_wrong_arg_type() {
    type_checker_error_test(
        r#"
fn main()
    runtime fn miri_rt_free(ptr i64)
    miri_rt_free("hello")
"#,
        "Type mismatch",
    );
}
