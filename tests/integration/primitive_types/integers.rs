// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_hex_bit_pattern_literals() {
    // Hex/binary/octal literals are bit patterns: a value with the high bit set
    // is the corresponding signed `int` (i64), not an out-of-range error. This
    // is the conventional all-ones / sign-bit idiom.
    assert_runs_with_output("println(f'{0xFFFFFFFFFFFFFFFF}')", "-1");
    assert_runs_with_output("println(f'{0x8000000000000000}')", "-9223372036854775808");
    // A hex value that still fits the signed range is unchanged.
    assert_runs_with_output("println(f'{0xFF}')", "255");
    // Binary all-ones is likewise -1.
    assert_runs_with_output(
        "println(f'{0b1111111111111111111111111111111111111111111111111111111111111111}')",
        "-1",
    );
}

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
