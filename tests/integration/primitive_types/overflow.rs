// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Integer overflow stops the program on every integer type, at the boundary
//! of each operation, and the explicit wrapping operations wrap.

use super::utils::*;

/// Every integer type a program computes at, with its smallest and largest
/// value spelled as Miri literals.
const TYPES: [(&str, &str, &str); 9] = [
    ("i8", "-128", "127"),
    ("i16", "-32768", "32767"),
    ("i32", "-2147483648", "2147483647"),
    ("i64", "-9223372036854775808", "9223372036854775807"),
    ("int", "-9223372036854775808", "9223372036854775807"),
    ("u8", "0", "255"),
    ("u16", "0", "65535"),
    ("u32", "0", "4294967295"),
    ("u64", "0", "18446744073709551615"),
];

/// A program that starts `x` at `start` and applies `step` to it once. `var`
/// keeps the value out of compile-time folding, so the run decides.
fn stepped(ty: &str, start: &str, step: &str) -> String {
    format!(
        "fn main()\n    var x {ty} = {start}\n    x = {step}\n    println(f\"{{x}}\")\n",
        ty = ty,
        start = start,
        step = step
    )
}

#[test]
fn test_every_overflowing_integer_operation_stops_the_program() {
    for (ty, min, max) in TYPES {
        let signed = !ty.starts_with('u');
        let mut cases = vec![
            (max, "x + 1".to_string()),
            (min, "x - 1".to_string()),
            (max, format!("x * 2")),
        ];
        if signed {
            cases.push((min, "-x".to_string()));
            cases.push((min, "x * -1".to_string()));
        }
        for (start, step) in cases {
            assert_runtime_error(&stepped(ty, start, &step), "integer overflow");
        }
    }
}

#[test]
fn test_every_integer_operation_that_fits_computes_its_value() {
    for (ty, min, max) in TYPES {
        let signed = !ty.starts_with('u');
        let code = format!(
            "fn main()\n    var hi {ty} = {max}\n    var lo {ty} = {min}\n    hi = hi - 1\n    hi = hi + 1\n    lo = lo + 1\n    lo = lo - 1\n    var one {ty} = 1\n    one = one * 1\n    println(f\"{{hi}} {{lo}} {{one}}\")\n",
            ty = ty,
            min = min,
            max = max
        );
        assert_runs_with_output(&code, &format!("{} {} 1", max, min));
        if signed {
            let negated = format!(
                "fn main()\n    var x {ty} = {max}\n    x = -x\n    println(f\"{{x}}\")\n",
                ty = ty,
                max = max
            );
            assert_runs_with_output(&negated, &format!("-{}", max));
        }
    }
}

#[test]
fn test_the_wrapping_operations_wrap_at_the_width_of_their_type() {
    assert_runs_with_output(
        r#"
use system.math

fn main()
    var big i32 = 2147483647
    var small i32 = -2147483648
    var byte u8 = 255
    var word u64 = 0
    println(f"{wrapping_add(big, 1 as i32)} {wrapping_sub(small, 1 as i32)} {wrapping_mul(big, 2 as i32)}")
    println(f"{wrapping_add(byte, 1 as u8)} {wrapping_sub(word, 1 as u64)} {wrapping_mul(byte, byte)}")
"#,
        "-2147483648 2147483647 -2\n0 18446744073709551615 1",
    );
}

#[test]
fn test_a_hash_built_from_wrapping_products_runs() {
    // `hash_u32` multiplies far past 32 bits on purpose; it must keep running
    // and give the same value the GPU gives.
    assert_runs_with_output(
        r#"
use system.math

fn main()
    println(f"{hash_u32(1 as u32)} {hash_u32(0 as u32)}")
"#,
        "2936555912 0",
    );
}

#[test]
fn test_128_bit_overflow_stops_the_program_and_a_fitting_product_computes() {
    // The 128-bit product has no checked instruction and is computed by the
    // runtime, so it is checked there; the sum and difference are checked in
    // compiled code like the narrower widths.
    let max = "170141183460469231731687303715884105727";
    for (ty, start, step) in [
        ("i128", max, "x * 2"),
        ("i128", max, "x + 1"),
        ("u128", "340282366920938463463374607431768211455", "x * 2"),
        ("u128", "0", "x - 1"),
    ] {
        assert_runtime_error(
            &format!(
                "fn main()\n    var x {ty} = {start}\n    x = {step}\n    println(\"unreachable\")\n"
            ),
            "integer overflow",
        );
    }
    assert_runs_with_output(
        "fn main()\n    var x i128 = 85070591730234615865843651857942052863\n    var y i128 = x * 2\n    let fits = y == 170141183460469231731687303715884105726\n    println(f\"{fits}\")\n",
        "true",
    );
}

#[test]
fn test_min_divided_or_reduced_by_minus_one_stops_the_program() {
    // The quotient of a signed type's minimum by -1 is one past its maximum,
    // and the remainder is computed by the same division, so both stop.
    for (ty, min) in [
        ("i8", "-128"),
        ("i16", "-32768"),
        ("i32", "-2147483648"),
        ("i64", "-9223372036854775807 - 1"),
        ("int", "-9223372036854775807 - 1"),
        ("i128", "-170141183460469231731687303715884105727 - 1"),
    ] {
        for op in ["/", "%"] {
            assert_runtime_error(
                &format!(
                    "fn main()\n    var x {ty} = {min}\n    var d {ty} = -1\n    x = x {op} d\n    println(\"unreachable\")\n"
                ),
                "integer overflow: the result does not fit its type",
            );
        }
    }
}

#[test]
fn test_division_next_to_the_minimum_computes_its_value() {
    assert_runs_with_output(
        "fn main()\n    var x i32 = -2147483648\n    var d i32 = -2\n    var one i32 = 1\n    println(f\"{x / d} {x % d} {x / one} {(x + 1) / -1}\")\n",
        "1073741824 0 -2147483648 2147483647",
    );
}

#[test]
fn test_abs_of_the_minimum_stops_the_program() {
    // The minimum of a signed type has no magnitude at its width.
    for (ty, min) in [
        ("i8", "-128"),
        ("i16", "-32768"),
        ("i32", "-2147483648"),
        ("i64", "-9223372036854775807 - 1"),
        ("int", "-9223372036854775807 - 1"),
        ("i128", "-170141183460469231731687303715884105727 - 1"),
    ] {
        assert_runtime_error(
            &format!(
                "use system.math\n\nfn main()\n    var x {ty} = {min}\n    x = abs(x)\n    println(\"unreachable\")\n"
            ),
            "integer overflow: the result does not fit its type",
        );
    }
}

#[test]
fn test_abs_computes_magnitudes_and_leaves_unsigned_values_alone() {
    assert_runs_with_output(
        "use system.math\n\nfn main()\n    let a i32 = -2147483647\n    let b i8 = -5\n    let u u32 = 3000000000\n    let w u8 = 200\n    println(f\"{abs(a)} {abs(b)} {abs(u)} {abs(w)}\")\n",
        "2147483647 5 3000000000 200",
    );
}
