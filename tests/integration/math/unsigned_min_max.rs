// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `min` and `max` order unsigned operands as unsigned values at every width,
//! so a value with the top bit set is the larger one rather than a negative.

use super::super::gpu::device::assert_gpu_runs_with_output;
use super::super::utils::*;

/// The program printing `min(a, b) max(a, b)` and the same with the arguments
/// swapped, for two bindings of the type `ty`.
fn both_orders(ty: &str, a: &str, b: &str) -> String {
    format!(
        r#"
use system.io
use system.math

fn main()
    let a {ty} = {a}
    let b {ty} = {b}
    println(f"{{min(a, b)}} {{max(a, b)}} {{min(b, a)}} {{max(b, a)}}")
"#
    )
}

/// `low` is the smaller of the two values and `high` the larger, whichever
/// order they are passed in.
fn assert_orders(ty: &str, low: &str, high: &str) {
    assert_runs_with_output(
        &both_orders(ty, low, high),
        &format!("{low} {high} {low} {high}"),
    );
}

#[test]
fn test_min_max_u8_above_sign_bit() {
    assert_orders("u8", "1", "255");
    assert_orders("u8", "127", "128");
}

#[test]
fn test_min_max_u16_above_sign_bit() {
    assert_orders("u16", "1", "65535");
    assert_orders("u16", "32767", "32768");
}

#[test]
fn test_min_max_u32_above_sign_bit() {
    assert_orders("u32", "1", "4294967295");
    assert_orders("u32", "2147483647", "2147483648");
}

#[test]
fn test_min_max_u64_at_sign_boundary() {
    assert_orders("u64", "1", "18446744073709551615");
    assert_orders("u64", "9223372036854775807", "9223372036854775808");
}

#[test]
fn test_min_max_u64_both_above_sign_boundary() {
    assert_orders("u64", "9223372036854775808", "18446744073709551615");
}

#[test]
fn test_min_max_u64_equal_values() {
    assert_orders("u64", "18446744073709551615", "18446744073709551615");
}

#[test]
fn test_min_max_u128_at_sign_boundary() {
    assert_orders("u128", "1", "340282366920938463463374607431768211455");
    assert_orders(
        "u128",
        "170141183460469231731687303715884105727",
        "170141183460469231731687303715884105728",
    );
}

#[test]
fn test_min_max_u128_both_above_sign_boundary() {
    assert_orders(
        "u128",
        "170141183460469231731687303715884105728",
        "340282366920938463463374607431768211455",
    );
}

/// The two 64-bit halves of a `u128` are ordered together: a larger high half
/// wins over a low half with every bit set.
#[test]
fn test_min_max_u128_orders_across_halves() {
    assert_orders("u128", "18446744073709551615", "18446744073709551616");
}

#[test]
fn test_min_max_u128_equal_values() {
    assert_orders(
        "u128",
        "340282366920938463463374607431768211455",
        "340282366920938463463374607431768211455",
    );
}

/// A number literal takes the unsigned argument's type in either position.
#[test]
fn test_min_max_u64_with_literal_in_either_position() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let w u64 = 18446744073709551615
    println(f"{min(w, 5)} {min(5, w)} {max(w, 5)} {max(5, w)}")
"#,
        "5 5 18446744073709551615 18446744073709551615",
    );
}

/// Two unsigned widths meet at the wider unsigned type.
#[test]
fn test_min_max_mixed_unsigned_widths() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let wide u64 = 18446744073709551615
    let narrow u8 = 200
    println(f"{min(wide, narrow)} {max(narrow, wide)}")
"#,
        "200 18446744073709551615",
    );
}

/// An unsigned and a signed argument meet at a signed type that holds both, so
/// the signed comparison there is the right one and the negative value is least.
#[test]
fn test_min_max_mixed_signedness_stays_numeric() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

fn main()
    let wide u64 = 18446744073709551615
    let negative i64 = -1
    println(f"{min(wide, negative)} {max(negative, wide)}")
"#,
        "-1 18446744073709551615",
    );
}

/// An unsigned value read through a field is ordered by its own type, not by
/// the type of the local it is projected from.
#[test]
fn test_min_max_u64_read_through_a_field() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

struct Pair
    high u64
    low u64

fn main()
    let p = Pair(high: 18446744073709551615, low: 1)
    println(f"{min(p.high, p.low)} {max(p.low, p.high)}")
"#,
        "1 18446744073709551615",
    );
}

#[test]
fn test_min_max_signed_negatives_unchanged() {
    assert_orders("i8", "-128", "127");
    assert_orders("i16", "-32768", "32767");
    assert_orders("i32", "-2147483648", "2147483647");
    assert_orders("i64", "-9223372036854775808", "9223372036854775807");
    assert_orders("i64", "-2", "-1");
}

#[test]
fn test_min_max_i128_negatives_unchanged() {
    assert_orders(
        "i128",
        "-170141183460469231731687303715884105728",
        "170141183460469231731687303715884105727",
    );
    assert_orders("i128", "-18446744073709551616", "-1");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_min_max_u32_above_sign_bit_gpu() {
    let source = r#"
        use system.math.{min, max}
        use system.gpu
        use system.collections.array

        let big u32 = 4294967295
        let mid u32 = 2147483648
        let one u32 = 1
        let zero u32 = 0
        gpu let a = [big, one]
        gpu let b = [mid, mid]
        gpu var low = [zero, zero]
        gpu var high = [zero, zero]
        gpu forall i in 0..2
            low[i] = min(a[i], b[i])
            high[i] = max(a[i], b[i])

        let lows = low
        let highs = high
        println(f'{lows[0]} {lows[1]} {highs[0]} {highs[1]}')
        "#;
    assert_gpu_runs_with_output(source, "2147483648 1 4294967295 2147483648");
}
