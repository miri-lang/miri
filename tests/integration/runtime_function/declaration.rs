// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn runtime_function_declaration_compiles() {
    assert_runs(
        "
runtime fn miri_rt_noop()
fn main()
    let x = 42
",
    );
}

#[test]
fn runtime_function_with_params_compiles() {
    assert_runs(
        "
runtime fn miri_rt_string_len(ptr i64) int
fn main()
    let x = 42
",
    );
}

#[test]
fn multiple_runtime_functions_compile() {
    assert_runs(
        "
runtime fn miri_rt_alloc(size int) i64
runtime fn miri_rt_free(ptr i64)
fn main()
    let x = 42
",
    );
}

#[test]
fn explicit_core_runtime_compiles() {
    assert_runs(
        r#"
runtime "core" fn miri_rt_string_new() i64
fn main()
    let x = 42
"#,
    );
}
