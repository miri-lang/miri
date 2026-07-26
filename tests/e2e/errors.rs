// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! End-to-end failure cases: whole programs, in the same style as the passing
//! examples next to them, driven through `miri run` and checked against the
//! exact diagnostic the compiler prints.
//!
//! A compile-time failure additionally proves the program never started — the
//! diagnostic is the only thing on the wire, `stdout` stays empty. A run-time
//! failure proves the opposite half: the output produced before the fault is
//! intact, and the abort message follows it.

use crate::utils::{miri_run, strip_ansi};

#[test]
fn example_err_01_type_mismatch() {
    assert_compile_fails(
        include_str!("./err_01_type_mismatch.mi"),
        &[
            "error[E0110]",
            "if arr[mid] == target: return \"found\"",
            "|                                       ^^^^^^^ Invalid return type: expected int, got String",
        ],
    );
}

#[test]
fn example_err_02_undefined_function() {
    assert_compile_fails(
        include_str!("./err_02_undefined_function.mi"),
        &[
            "error[E0110]",
            "help: Did you mean 'bubble_sort'?",
            "let sorted = buble_sort(data)",
            "|                  ^^^^^^^^^^ Undefined variable: buble_sort",
        ],
    );
}

#[test]
fn example_err_03_indentation() {
    assert_compile_fails(
        include_str!("./err_03_indentation.mi"),
        &[
            "error[E0003]: Indentation Mismatch",
            "return n * factorial(n - 1)",
            "help: Ensure the indentation level matches the surrounding code block.",
        ],
    );
}

#[test]
fn example_err_04_missing_argument() {
    assert_compile_fails(
        include_str!("./err_04_missing_argument.mi"),
        &[
            "error[E0110]",
            "println(f\"gcd(48, 18) = {gcd(48)}\")",
            "|                              ^^^^^^^ Missing argument for parameter 'b'",
        ],
    );
}

#[test]
fn example_err_05_multiple_errors() {
    let output = assert_compile_fails(
        include_str!("./err_05_multiple_errors.mi"),
        &[
            "help: Did you mean 'data'?",
            "let sum int = total(dta)",
            "|                         ^^^ Undefined variable: dta",
            "let avg String = sum / data.length()",
            "|                      ^^^^^^^^^^^^^^^^^^^ Type mismatch for variable 'avg': expected String, got int",
        ],
    );

    assert_eq!(
        output.matches("error[").count(),
        2,
        "Both mistakes should be reported from one compile, not just the first.\nActual:\n{output}"
    );
    let first = output
        .find("Undefined variable: dta")
        .expect("the undefined-argument diagnostic was asserted above");
    let second = output
        .find("Type mismatch for variable 'avg'")
        .expect("the type-mismatch diagnostic was asserted above");
    assert!(
        first < second,
        "Diagnostics should be reported in source order.\nActual:\n{output}"
    );
}

#[test]
fn example_err_06_index_out_of_bounds() {
    assert_run_fails(
        include_str!("./err_06_index_out_of_bounds.mi"),
        "Scanning:\nitem 3\nitem 1\nitem 2\n",
        "Runtime error: Array index out of bounds: the len is 3 but the index is 3",
    );
}

#[test]
fn example_err_07_division_by_zero() {
    assert_run_fails(
        include_str!("./err_07_division_by_zero.mi"),
        "Computing mean:\n",
        "Runtime error: division by zero",
    );
}

/// Asserts the program is rejected before it runs, and that the diagnostic
/// contains every expected fragment. Returns the ANSI-stripped diagnostic so a
/// caller can make further claims about it.
fn assert_compile_fails(source: &str, expected_parts: &[&str]) -> String {
    let result = miri_run(source);
    let stdout = strip_ansi(&result.stdout);
    let diagnostic = strip_ansi(&result.stderr);

    assert!(
        !result.success,
        "Expected the program to be rejected, but it compiled and ran.\nStdout:\n{stdout}"
    );
    assert!(
        stdout.is_empty(),
        "A rejected program must not run: expected no stdout, got:\n{stdout}"
    );
    for part in expected_parts {
        assert!(
            diagnostic.contains(part),
            "Diagnostic did not contain expected fragment.\nExpected: '{part}'\nActual:\n{diagnostic}"
        );
    }
    diagnostic
}

/// Asserts the program compiles, prints `expected_stdout`, then aborts with
/// `expected_error` on stderr.
fn assert_run_fails(source: &str, expected_stdout: &str, expected_error: &str) {
    let result = miri_run(source);
    let stdout = strip_ansi(&result.stdout);
    let stderr = strip_ansi(&result.stderr);

    assert!(
        !result.success,
        "Expected the program to abort at runtime, but it exited successfully.\nStdout:\n{stdout}"
    );
    assert_eq!(
        stdout, expected_stdout,
        "Output produced before the fault differs.\nStderr:\n{stderr}"
    );
    assert!(
        stderr.contains(expected_error),
        "Runtime error text differs.\nExpected: '{expected_error}'\nActual:\n{stderr}"
    );
}
