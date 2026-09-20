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
fn test_unsigned_64bit_formatting() {
    // A u64 value at or above 2^63 must format as unsigned, not be reinterpreted
    // as a negative i64. The stored value already round-trips through unsigned
    // division and comparison correctly; only the to-string path was signed.
    assert_runs_with_output(
        r#"
let x u64 = 18446744073709551615
println(f"{x}")
"#,
        "18446744073709551615",
    );
    assert_runs_with_output(
        r#"
let x u64 = 10000000000000000000
println(f"{x}")
"#,
        "10000000000000000000",
    );
    // A u64 below 2^63 is unaffected.
    assert_runs_with_output(
        r#"
let x u64 = 42
println(f"{x}")
"#,
        "42",
    );
    // An untyped hex bit-pattern literal is a signed `int`, so its all-ones form
    // stays -1 — the unsigned fix must not disturb it.
    assert_runs_with_output("println(f'{0xFFFFFFFFFFFFFFFF}')", "-1");
}

#[test]
fn test_negative_integers() {
    assert_runs("let x i8 = -128");
    assert_runs("let x i32 = -2147483648");
}

#[test]
fn test_bitwise_not_on_integer_types() {
    // Bitwise NOT on unsigned 8-bit integer: ~0u8 must yield 255, not 1.
    assert_runs_with_output(
        r#"
let x u8 = 0
let y = ~x
println(f"{y}")
"#,
        "255",
    );

    // Bitwise NOT on signed 8-bit integer: ~0i8 must yield -1, not 1.
    assert_runs_with_output(
        r#"
let x i8 = 0
let y = ~x
println(f"{y}")
"#,
        "-1",
    );
}

#[test]
fn test_signed_128bit_formatting() {
    // A 128-bit value above the 64-bit range must render every digit. The stored
    // value is already sound — comparisons and collection round-trips agree — so
    // a wrong rendering here is the formatter narrowing the value at the call.
    assert_runs_with_output(
        r#"
let big i128 = 170141183460469231731687303715884105727
println(f"{big}")
"#,
        "170141183460469231731687303715884105727",
    );
    // Just past i64::MAX is the smallest value that exposes the narrowing: read
    // as an i64 its bit pattern is i64::MIN.
    assert_runs_with_output(
        r#"
let med i128 = 9223372036854775808
println(f"{med}")
"#,
        "9223372036854775808",
    );
    // A negative value beyond the 64-bit range keeps its sign and its digits.
    // It is reached by subtraction rather than written as a literal, because a
    // negative literal whose magnitude exceeds i64::MAX does not survive being
    // stored — a defect in the literal's construction, not in this rendering.
    assert_runs_with_output(
        r#"
let zero i128 = 0
let big i128 = 170141183460469231731687303715884105727
let neg = zero - big
println(f"{neg}")
"#,
        "-170141183460469231731687303715884105727",
    );
    // A 128-bit value inside the 64-bit range is unchanged.
    assert_runs_with_output(
        r#"
let small i128 = -42
println(f"{small}")
"#,
        "-42",
    );
}

#[test]
fn test_unsigned_128bit_formatting() {
    // A u128 above 2^64 renders its magnitude; nothing about it may be read as
    // signed.
    assert_runs_with_output(
        r#"
let x u128 = 18446744073709551616
println(f"{x}")
"#,
        "18446744073709551616",
    );
    // A u128 whose top bit is set must not print as a negative i128.
    assert_runs_with_output(
        r#"
let hi u128 = ~(0 as u128)
println(f"{hi}")
"#,
        "340282366920938463463374607431768211455",
    );
    // A u128 inside the 64-bit range is unaffected.
    assert_runs_with_output(
        r#"
let small u128 = 7
println(f"{small}")
"#,
        "7",
    );
}

#[test]
fn test_128bit_interpolation_renders_a_computed_value() {
    // A computed 128-bit expression reaches the formatter by the same route as
    // a bare binding, so it must render the same way rather than narrowing.
    assert_runs_with_output(
        r#"
let med i128 = 9223372036854775808
println(f"{med} {med + 1}")
"#,
        "9223372036854775808 9223372036854775809",
    );
}

#[test]
fn test_negated_wide_literal_keeps_its_magnitude() {
    // A negative literal whose magnitude exceeds i64::MAX must reach its slot
    // intact. Materializing the magnitude at 64 bits and negating it there
    // flips the sign: 0x8000_0000_0000_0001 read as an i64 is negative, so
    // negating it yields a positive number.
    assert_runs_with_output(
        r#"
let zero i128 = 0
let b i128 = -9223372036854775809
println(f"{b} {b < zero}")
"#,
        "-9223372036854775809 true",
    );
    assert_runs_with_output(
        r#"
let zero i128 = 0
let c i128 = -18446744073709551617
println(f"{c} {c < zero}")
"#,
        "-18446744073709551617 true",
    );
    // The full negative range bar i128::MIN, whose magnitude cannot be written.
    assert_runs_with_output(
        r#"
let zero i128 = 0
let d i128 = -170141183460469231731687303715884105727
println(f"{d} {d < zero}")
"#,
        "-170141183460469231731687303715884105727 true",
    );
    // A magnitude that fits i64 was always correct and must stay so.
    assert_runs_with_output(
        r#"
let zero i128 = 0
let a i128 = -9223372036854775807
println(f"{a} {a < zero}")
"#,
        "-9223372036854775807 true",
    );
}

#[test]
fn test_a_wide_literal_is_accepted_in_an_argument_position() {
    // The expected type at an argument position decides the literal's width, so
    // a value above i64::MAX is spellable there and not only as an annotated
    // binding — and it must arrive whole.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var l = List<i128>([0, 0])
    l.set(0, 170141183460469231731687303715884105727)
    println(f"{l[0] == big}")
"#,
        "true",
    );
}

#[test]
fn test_an_out_of_range_literal_names_the_type_it_was_written_into() {
    // The bound reported is the one the source asked for, not the default the
    // literal was inferred as before its context was known.
    assert_compiler_error(
        r#"
let x i64 = 170141183460469231731687303715884105727
"#,
        "out of range for i64 (max 9223372036854775807)",
    );
    // A literal nothing typed is still judged against the default `int`.
    assert_compiler_error(
        r#"
let x = 170141183460469231731687303715884105727
"#,
        "out of range for the default int type (i64, max 9223372036854775807)",
    );
}
