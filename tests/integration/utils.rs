// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::interpreter::Value;
use miri::pipeline::Pipeline;
use std::io::Write;
use tempfile::NamedTempFile;

use crate::utils::{miri_cmd, run_compiler, CompilerResult};

/// Result of running code on both backends.
#[derive(Debug)]
pub struct DualRunResult {
    /// Value returned by the interpreter.
    pub interpreter_value: Value,
    /// Whether the compiler successfully built the code.
    pub compiler_success: bool,
}

/// Run the build command and return the result.
pub fn build_compiler(input: &str) -> CompilerResult {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", input).unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let output = cmd.arg("build").arg(&path).output().unwrap();

    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

pub fn assert_valid(code: &str) {
    let result = run_compiler(code);

    // We check if there are any "error:" lines in the output.
    let output = result.output();
    if output.contains("error:") {
        panic!("Expected valid program, but got errors:\n{}", output);
    }
}

/// Assert that the code successfully compiles to an executable.
/// This uses the build command to verify full compilation works.
pub fn assert_compiles(code: &str) {
    let result = build_compiler(code);

    if !result.success {
        panic!(
            "Expected program to compile successfully, but got errors:\n{}",
            result.output()
        );
    }
}

/// Run code on the interpreter and return the result.
///
/// This runs the code through the full pipeline: parse -> type-check -> MIR -> interpret.
pub fn interpret(code: &str) -> Result<Value, String> {
    let pipeline = Pipeline::new();
    pipeline.interpret(code).map_err(|e| e.to_string())
}

/// Build code with the compiler (Cranelift backend).
///
/// Returns Ok(()) if compilation succeeds, Err with message otherwise.
pub fn compile(code: &str) -> Result<(), String> {
    let pipeline = Pipeline::new();
    let opts = miri::pipeline::BuildOptions::default();
    pipeline.build(code, &opts).map_err(|e| e.to_string())?;
    Ok(())
}

/// Get MIR output for debugging.
pub fn get_mir(code: &str) -> String {
    let pipeline = Pipeline::new();
    pipeline
        .get_mir(code)
        .unwrap_or_else(|e| format!("Failed to get MIR: {}", e))
}

/// Assert that code runs successfully on BOTH the interpreter AND compiler.
///
/// This is the primary test helper for feature parity testing.
/// Use this when you don't care about the return value, just that both work.
///
/// # Panics
/// Panics if either the interpreter or compiler fails to process the code.
pub fn assert_runs(code: &str) {
    // Test interpreter
    let interp_result = interpret(code);
    if let Err(e) = &interp_result {
        let mir = get_mir(code);
        panic!(
            "Interpreter failed:\n{}\n\nCode:\n{}\n\nMIR:\n{}",
            e, code, mir
        );
    }

    // Test compiler
    let compile_result = compile(code);
    if let Err(e) = &compile_result {
        let mir = get_mir(code);
        panic!(
            "Compiler failed:\n{}\n\nCode:\n{}\n\nMIR:\n{}",
            e, code, mir
        );
    }
}

/// The same as `assert_runs`, but for multiple codes.
pub fn assert_runs_many(codes: &[&str]) {
    for code in codes {
        assert_runs(code);
    }
}

/// Assert that code returns the expected integer value on BOTH backends.
///
/// This verifies both:
/// 1. The interpreter returns the expected value
/// 2. The compiler successfully compiles the code
///
/// Note: We can only verify the interpreter's return value directly.
/// The compiler produces an executable - testing its output requires
/// running it, which we do in `assert_runs_and_returns`.
///
/// # Panics
/// Panics if the interpreter returns a different value or if either backend fails.
pub fn assert_returns(code: &str, expected: i64) {
    // Test interpreter
    let interp_result = interpret(code);
    match &interp_result {
        Ok(value) => {
            let actual = value.as_int().unwrap_or_else(|| {
                let mir = get_mir(code);
                panic!(
                    "Interpreter returned non-integer value: {:?}\n\nCode:\n{}\n\nMIR:\n{}",
                    value, code, mir
                )
            });
            assert_eq!(
                actual, expected as i128,
                "Interpreter returned wrong value.\nExpected: {}\nGot: {}\n\nCode:\n{}",
                expected, actual, code
            );
        }
        Err(e) => {
            let mir = get_mir(code);
            panic!(
                "Interpreter failed:\n{}\n\nCode:\n{}\n\nMIR:\n{}",
                e, code, mir
            );
        }
    }

    // Test compiler
    let compile_result = compile(code);
    if let Err(e) = &compile_result {
        let mir = get_mir(code);
        panic!(
            "Compiler failed (interpreter returned {}):\n{}\n\nCode:\n{}\n\nMIR:\n{}",
            expected, e, code, mir
        );
    }
}

/// The same as `assert_returns`, but for multiple codes.
pub fn assert_returns_many(tests: &[(&str, i64)]) {
    for (code, expected) in tests {
        assert_returns(code, *expected);
    }
}

/// Assert that the code fails during compilation with a specific error message.
pub fn assert_compiler_error(code: &str, expected_error: &str) {
    let result = run_compiler(code);
    let output = result.output();

    if !output.contains("error:") {
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

/// Assert that the code fails with a specific runtime error message.
/// For the interpreter, this checks the error message.
/// For the compiler, this is currently a type-check error (mixed types are caught at compile time).
pub fn assert_runtime_error(code: &str, expected_error: &str) {
    // Test interpreter
    let interp_result = interpret(code);
    match interp_result {
        Err(e) => {
            if !e.contains(expected_error) {
                panic!(
                    "Expected interpreter error '{}', but got different error:\n{}",
                    expected_error, e
                );
            }
        }
        Ok(value) => {
            panic!(
                "Expected interpreter error '{}', but got value: {:?}",
                expected_error, value
            );
        }
    }

    // Test compiler - for type errors, compilation should fail
    let compile_result = compile(code);
    match compile_result {
        Err(e) => {
            if !e.contains(expected_error) {
                panic!(
                    "Expected compiler error '{}', but got different error:\n{}",
                    expected_error, e
                );
            }
        }
        Ok(()) => {
            // Compilation succeeded, which is fine for runtime errors
            // that can't be detected at compile time
        }
    }
}

/// Assert that code returns the expected value and also emits a warning.
/// The warning is printed to stderr during compilation.
pub fn assert_returns_with_warning(code: &str, expected: i64, expected_warning: &str) {
    // First verify interpreter succeeds with expected value
    let interp_result = interpret(code);
    match &interp_result {
        Ok(value) => {
            let actual = value.as_int().unwrap_or_else(|| {
                panic!(
                    "Interpreter returned non-integer value: {:?}\n\nCode:\n{}",
                    value, code
                )
            });
            assert_eq!(
                actual, expected as i128,
                "Interpreter returned wrong value.\nExpected: {}\nGot: {}\n\nCode:\n{}",
                expected, actual, code
            );
        }
        Err(e) => {
            panic!("Interpreter failed:\n{}\n\nCode:\n{}", e, code);
        }
    }

    // Verify compiler succeeds
    let compile_result = compile(code);
    if let Err(e) = &compile_result {
        panic!("Compiler failed:\n{}\n\nCode:\n{}", e, code);
    }

    // Check that warning was printed using the CLI (which captures stderr)
    let cli_result = run_compiler(code);
    let output = cli_result.output();

    if !output.contains(expected_warning) {
        panic!(
            "Expected warning '{}' not found in output:\n{}",
            expected_warning, output
        );
    }
}
