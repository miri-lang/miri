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

#[test]
fn test_wide_literal_in_an_index_assignment_reaches_the_slot_whole() {
    // `i128::MAX` and `-1` share their low word, so only a whole write tells
    // them apart.
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.array

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var l = List<i128>()
    l.push(-1)
    l[0] = 170141183460469231731687303715884105727
    var a = Array<i128, 1>()
    a[0] = 170141183460469231731687303715884105727
    println(f'{l[0] == big} {a[0] == big}')
",
        "true true",
    );
}

#[test]
fn test_wide_literal_in_a_return_takes_the_declared_width() {
    assert_runs_with_output(
        "
fn widest() i128
    return 170141183460469231731687303715884105727

fn above_i64() u128: 18446744073709551621

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let top u128 = 18446744073709551621
    println(f'{widest() == big} {above_i64() == top}')
",
        "true true",
    );
}

#[test]
fn test_literal_too_wide_for_an_assigned_slot_names_the_slot() {
    assert_compiler_error(
        "
fn main()
    var x i64 = 0
    x = 9223372036854775808
    println(f'{x}')
",
        "out of range for i64",
    );
}

/// The full ranges of the 128-bit types can be written: `u128::MAX` above
/// `i128::MAX`, and `i128::MIN`, whose magnitude no `i128` holds.
#[test]
fn test_128_bit_extremes_can_be_written() {
    assert_runs_with_output(
        "
fn main()
    let top u128 = 340282366920938463463374607431768211455
    let low_twin u128 = 18446744073709551615
    let bottom i128 = -170141183460469231731687303715884105728
    let next i128 = -170141183460469231731687303715884105727
    let zero i128 = 0
    println(f'{top} {top == low_twin} {top - 1 > low_twin}')
    println(f'{bottom} {bottom < zero} {bottom + 1 == next}')
",
        "340282366920938463463374607431768211455 false true\n-170141183460469231731687303715884105728 true true",
    );
}

#[test]
fn test_literal_above_i128_max_is_refused_for_an_i128() {
    assert_compiler_error(
        "
fn main()
    let x i128 = 340282366920938463463374607431768211455
    println(f'{x}')
",
        "out of range for i128",
    );
}

#[test]
fn test_literal_past_u128_max_is_refused() {
    assert_compiler_error(
        "
fn main()
    let x u128 = 340282366920938463463374607431768211456
    println(f'{x}')
",
        "Invalid Integer Literal",
    );
}

/// A constant too large for any `i128` is refused where a constant is needed,
/// rather than folded from its bit pattern.
#[test]
fn test_literal_above_i128_max_is_not_a_constant_size() {
    assert_compiler_error(
        "
use system.collections.array

fn main()
    let a = Array<int, 340282366920938463463374607431768211455>()
    println(f'{a.length()}')
",
        "compile-time constant",
    );
}

#[test]
fn test_literal_too_large_for_a_narrow_declaration_is_refused() {
    assert_compiler_error(
        "
fn main()
    let y i8 = 300
    println(f'{y}')
",
        "Integer literal '300' is out of range for i8 (max 127)",
    );
}

#[test]
fn test_literal_too_large_for_a_narrow_argument_is_refused() {
    assert_compiler_error(
        "
fn take(v i8) i8: v

fn main()
    println(f'{take(300)}')
",
        "Integer literal '300' is out of range for i8 (max 127)",
    );
}

#[test]
fn test_literal_too_large_for_a_narrow_assignment_is_refused() {
    assert_compiler_error(
        "
fn main()
    var x i8 = 0
    x = 300
    println(f'{x}')
",
        "Integer literal '300' is out of range for i8 (max 127)",
    );
}

#[test]
fn test_literal_too_large_for_a_narrow_field_or_return_is_refused() {
    assert_compiler_error(
        "
struct Pixel
    level u8

fn main()
    let p = Pixel(level: 256)
    println(f'{p.level}')
",
        "Integer literal '256' is out of range for u8 (max 255)",
    );
    assert_compiler_error(
        "
fn small() i16: 40000

fn main()
    println(f'{small()}')
",
        "Integer literal '40000' is out of range for i16 (max 32767)",
    );
}

#[test]
fn test_the_bottom_of_a_signed_range_is_written_negated() {
    assert_runs_with_output(
        "
fn main()
    let b i8 = -128
    let s i16 = -32768
    let w i32 = -2147483648
    println(f'{b} {s} {w}')
",
        "-128 -32768 -2147483648",
    );
    assert_compiler_error(
        "
fn main()
    let b i8 = -129
    println(f'{b}')
",
        "Integer literal '-129' is out of range for i8",
    );
}

#[test]
fn test_a_negative_literal_is_refused_for_an_unsigned_target() {
    assert_compiler_error(
        "
fn main()
    let b u8 = -1
    println(f'{b}')
",
        "Integer literal '-1' is out of range for u8",
    );
    assert_compiler_error(
        "
fn main()
    let b u64 = -1
    println(f'{b}')
",
        "Integer literal '-1' is out of range for u64",
    );
}

#[test]
fn test_the_edges_of_every_narrow_range_are_accepted() {
    assert_runs_with_output(
        "
fn main()
    let a i8 = 127
    let b u8 = 255
    let c i16 = 32767
    let d u16 = 65535
    let e i32 = 2147483647
    let f u32 = 4294967295
    let g u64 = 18446744073709551615
    println(f'{a} {b} {c} {d} {e} {f} {g}')
",
        "127 255 32767 65535 2147483647 4294967295 18446744073709551615",
    );
}

#[test]
fn test_a_full_width_bit_pattern_still_fills_a_64_bit_slot() {
    assert_runs_with_output(
        "
fn main()
    let mask u64 = 0xFFFFFFFFFFFFFFFF
    let all int = 0xFFFFFFFFFFFFFFFF
    var slot u64 = 0
    slot = 0xFFFFFFFFFFFFFFFF
    println(f'{mask} {all} {slot}')
",
        "18446744073709551615 -1 18446744073709551615",
    );
}

#[test]
fn test_base_n_literals_are_written_inside_brackets() {
    assert_runs_with_output(
        "
use system.collections.list

fn keep(v u64) u64: v

fn main()
    let k = (0xFF)
    let l = [0b101, 0o17]
    println(f'{k} {l[0]} {l[1]} {keep(0xFFFFFFFFFFFFFFFF)}')
",
        "255 5 15 18446744073709551615",
    );
}

/// A literal beside a 128-bit operand takes its width: the comparison and the
/// arithmetic run over all 128 bits, which the low-word twin (`-1` shares every
/// bit of `i128::MAX`'s low word) tells apart from a 64-bit reading.
#[test]
fn a_literal_beside_a_wide_operand_takes_its_width() {
    assert_runs_with_output(
        r#"
fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    let top u128 = 340282366920938463463374607431768211455
    let one = big - 170141183460469231731687303715884105726
    println(f"{big == 170141183460469231731687303715884105727} {twin == 170141183460469231731687303715884105727} {one == 1} {top > 18446744073709551616}")
"#,
        "true false true true",
    );
}

/// A literal past the wide operand's own range is still refused, naming it.
#[test]
fn a_literal_past_the_wide_operands_range_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let small u64 = 1
    println(f"{small == 18446744073709551616}")
"#,
        "out of range for u64",
    );
}
