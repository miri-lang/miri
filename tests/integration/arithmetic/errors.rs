// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_division_by_zero_compile_time() {
    // Miri should catch division by zero at compile time, at least for basic cases.
    assert_compiler_error("5 / 0", "Division by zero");
    assert_compiler_error("123 / 0.0", "Division by zero");
    assert_compiler_error("10 % 0", "Division by zero");
    assert_compiler_error("10 % -0", "Division by zero");
    assert_compiler_error("0 / 0", "Division by zero");
    assert_compiler_error("0.0 / 0.0", "Division by zero");

    // TODO: requires constant propagation
    // assert_compiler_error("let x = 0\nlet y = 1\n1 / x", "Division by zero");

    // And this (because of optimization)
    // assert_compiler_error("let x = 1\nlet y = 1\nlet z = 1\n1 / (x - y)", "Division by zero");
}

#[test]
fn test_division_by_zero_runtime() {
    // Division by zero at runtime causes a hardware trap (SIGILL/SIGFPE),
    // so we just verify the program crashes rather than looking for a specific message.
    let examples = ["var x = 10\nwhile x > 0:\n  x -= 1\n\n1 / x"];

    for example in examples {
        assert_runtime_crash(example);
    }
}

#[test]
fn test_mixed_type_operations() {
    assert_compiler_error("1 + 2.5", "Type mismatch: cannot add a float to an integer");
}
