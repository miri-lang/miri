// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_operation_outputs, assert_runs};

// =============================================================================
// Integer Type Tests
// =============================================================================

#[test]
fn test_integer_types_signed() {
    assert_runs("let x i8 = 127");
    assert_runs("let x i16 = 32767");
    assert_runs("let x i32 = 2147483647");
    assert_runs("let x i64 = 9223372036854775807");
}

#[test]
fn test_integer_types_unsigned() {
    assert_runs("let x u8 = 255");
    assert_runs("let x u16 = 65535");
    assert_runs("let x u32 = 4294967295");
}

#[test]
fn test_negative_integers() {
    assert_runs("let x i8 = -128");
    assert_runs("let x i32 = -2147483648");
}

// =============================================================================
// Float Type Tests
// =============================================================================

#[test]
fn test_float_types() {
    assert_runs("let x f32 = 3.14");
    assert_runs("let x f64 = 3.14159265358979");
}

#[test]
fn test_float_operations() {
    assert_runs("1.5 + 2.5");
    assert_runs("3.0 * 2.0");
    assert_runs("10.0 / 4.0");
}

// =============================================================================
// Boolean Logic Tests
// =============================================================================

#[test]
fn test_boolean_literals() {
    assert_runs("true");
    assert_runs("false");
}

#[test]
fn test_boolean_not() {
    assert_operation_outputs(&[
        ("if not false: 1 else: 0", "1"),
        ("if not true: 1 else: 0", "0"),
    ]);
}

#[test]
fn test_boolean_comparisons() {
    assert_operation_outputs(&[
        ("if true == true: 1 else: 0", "1"),
        ("if true != false: 1 else: 0", "1"),
        ("if false == false: 1 else: 0", "1"),
    ]);
}
