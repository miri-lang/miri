// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::{miri_check, miri_run};

/// Checks whether the output contains an error header in either format:
/// - `error:` (plain, no error code)
/// - `error[E0xxx]:` (with error code)
fn has_error_header(output: &str) -> bool {
    output.contains("error:") || output.contains("error[")
}

/// Assert that the code passes type checking (no errors).
pub fn assert_type_checks(code: &str) {
    let result = miri_check(code);
    let output = result.output();

    if has_error_header(&output) {
        panic!(
            "Expected program to pass type checking, but got errors:\n{}",
            output
        );
    }
}

/// Assert that the code successfully compiles to an executable.
pub fn assert_runs(code: &str) {
    let result = miri_run(code);

    if !result.success {
        if result.stderr.contains("MIRI_LEAK_CHECK: leaked") {
            panic!("Memory leak detected:\n{}", result.output());
        }
        panic!(
            "Expected program to compile and run successfully, but it failed:\n{}",
            result.output()
        );
    }
}

/// The same as `assert_runs`, but for multiple codes.
pub fn assert_runs_many(codes: &[&str]) {
    for code in codes {
        assert_runs(code);
    }
}

/// Assert that the code successfully compiles to an executable and prints the expected output.
pub fn assert_runs_with_output(code: &str, expected_output: &str) {
    let result = miri_run(code);

    if !result.success {
        if result.stderr.contains("MIRI_LEAK_CHECK: leaked") {
            panic!("Memory leak detected:\n{}", result.output());
        }
        panic!(
            "Expected program to compile and run successfully, but it failed:\n{}",
            result.output()
        );
    }

    if !result.output().contains(expected_output) {
        panic!(
            "Expected output '{}' not found in output:\n{}",
            expected_output,
            result.output()
        );
    }
}

pub fn assert_operation_outputs(operations: &[(&str, &str)]) {
    let statements = operations
        .iter()
        .map(|(op, _)| format!("println(f'{{{op}}}')"))
        .collect::<Vec<_>>()
        .join("\n");
    let expected_output = operations
        .iter()
        .map(|(_, expected_output)| *expected_output)
        .collect::<Vec<_>>()
        .join("\n");
    assert_runs_with_output(
        format!("use system.io\n{statements}").as_str(),
        &expected_output,
    );
}

/// Assert that the code fails during compilation with a specific error message.
pub fn assert_compiler_error(code: &str, expected_error: &str) {
    let result = miri_check(code);
    let output = result.output();

    if !has_error_header(&output) {
        panic!(
            "Expected invalid program, but got no errors.\nOutput:\n{}",
            output
        );
    }

    if !output.contains(expected_error) {
        panic!(
            "Expected error '{}' not found in output:\n{}",
            expected_error, output
        );
    }
}

/// Assert that the code fails during compilation with a specific warning message.
pub fn assert_compiler_warning(code: &str, expected_warning: &str) {
    let result = miri_check(code);
    let output = result.output();

    if !output.contains(expected_warning) {
        panic!(
            "Expected warning '{}' not found in output:\n{}",
            expected_warning, output
        );
    }
}

/// Assert that the code fails with a specific runtime error message.
pub fn assert_runtime_error(code: &str, expected_error: &str) {
    let result = miri_run(code);
    let output = result.output();

    if !has_error_header(&output) {
        panic!(
            "Expected invalid program, but got no errors.\nOutput:\n{}",
            output
        );
    }

    if !output.contains(expected_error) {
        panic!(
            "Expected error '{}' not found in output:\n{}",
            expected_error, output
        );
    }
}

/// Assert that the code compiles but crashes at runtime (non-zero exit code or signal).
///
/// This is used for cases like hardware traps (e.g. division by zero on AArch64)
/// where the process is killed by a signal rather than printing an error message.
pub fn assert_runtime_crash(code: &str) {
    let result = miri_run(code);

    if result.success {
        panic!(
            "Expected program to crash at runtime, but it succeeded.\nOutput:\n{}",
            result.output()
        );
    }
}
