// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A format spec after `:` in an interpolation says how its value is written:
//! `[[fill]align][0][width][.precision][type]`, as in Python.

use super::utils::*;

#[test]
fn test_precision_rounds_a_float_to_that_many_decimals() {
    assert_runs_with_output(
        "fn main()\n    let x = 3.14159\n    println(f\"{x:.2f} {x:.0f} {x:.4f}\")\n",
        "3.14 3 3.1416",
    );
}

#[test]
fn test_precision_writes_an_integer_as_a_float() {
    assert_runs_with_output(
        "fn main()\n    let n = 5\n    println(f\"{n:.1f}\")\n",
        "5.0",
    );
}

#[test]
fn test_precision_on_an_f32_uses_its_value() {
    assert_runs_with_output(
        "fn main()\n    let f f32 = 0.1\n    println(f\"{f:.3f}\")\n",
        "0.100",
    );
}

#[test]
fn test_width_right_aligns_a_number_and_left_aligns_text() {
    assert_runs_with_output(
        "fn main()\n    let n = 42\n    let s = \"ab\"\n    println(f\"[{n:5}] [{s:4}]\")\n",
        "[   42] [ab  ]",
    );
}

#[test]
fn test_alignment_and_fill_place_the_value() {
    assert_runs_with_output(
        "fn main()\n    let s = \"ab\"\n    let n = 7\n    println(f\"[{s:>4}] [{s:^6}] [{n:*<3}]\")\n",
        "[  ab] [  ab  ] [7**]",
    );
}

#[test]
fn test_a_leading_zero_pads_after_the_sign() {
    assert_runs_with_output(
        "fn main()\n    let n = -5\n    let x = 3.14159\n    println(f\"{n:04} {x:08.3f}\")\n",
        "-005 0003.142",
    );
}

#[test]
fn test_radix_types_write_an_integer_in_that_base() {
    assert_runs_with_output(
        "fn main()\n    let n = 255\n    let m = -255\n    println(f\"{n:x} {n:X} {5:b} {8:o} {n:04x} {m:x}\")\n",
        "ff FF 101 10 00ff -ff",
    );
}

#[test]
fn test_an_unsigned_value_keeps_its_magnitude_in_hex() {
    assert_runs_with_output(
        "fn main()\n    let u u64 = 18446744073709551615\n    println(f\"{u:x}\")\n",
        "ffffffffffffffff",
    );
}

#[test]
fn test_width_pads_any_value_by_its_ordinary_text() {
    assert_runs_with_output(
        "fn main()\n    let b = true\n    println(f\"[{b:>6}]\")\n",
        "[  true]",
    );
}

/// The `:` of an inline `if` inside an interpolation is not a format spec.
#[test]
fn test_an_inline_if_in_an_interpolation_still_reads_as_one() {
    assert_runs_with_output(
        "fn main()\n    let c = true\n    println(f\"{if c: 1 else: 2} {if c:3 else:4}\")\n",
        "1 3",
    );
}

/// A `:` inside brackets or a call belongs to the expression.
#[test]
fn test_a_colon_inside_a_call_is_part_of_the_expression() {
    assert_runs_with_output(
        "fn pick(a int, b int) int\n    return a + b\n\nfn main()\n    println(f\"{pick(a: 1, b: 2):>3}\")\n",
        "  3",
    );
}

#[test]
fn test_precision_without_a_type_is_refused_naming_f() {
    assert_compiler_error(
        "fn main()\n    let x = 3.14159\n    println(f\"{x:.2}\")\n",
        "`.2f`",
    );
}

#[test]
fn test_a_radix_on_a_float_is_refused() {
    assert_compiler_error(
        "fn main()\n    let x = 3.5\n    println(f\"{x:x}\")\n",
        "MER_TYP_082",
    );
}

#[test]
fn test_precision_on_text_is_refused() {
    assert_compiler_error(
        "fn main()\n    let s = \"ab\"\n    println(f\"{s:.2f}\")\n",
        "MER_TYP_082",
    );
}

#[test]
fn test_a_spec_miri_does_not_read_is_refused_with_its_shape() {
    assert_compiler_error(
        "fn main()\n    let n = 1000\n    println(f\"{n:,}\")\n",
        "[[fill]align][0][width][.precision][type]",
    );
}
