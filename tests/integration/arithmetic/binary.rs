// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenco

use super::utils::*;
use crate::integration::utils::assert_runs_with_output;

#[test]
fn test_binary_operations_on_integers() {
    assert_operation_outputs(&[
        ("123 + 456", "579"),
        ("123 - 456", "-333"),
        ("123 * 456", "56088"),
        ("123 / 456", "0"),
        ("123 % 456", "123"),
    ]);
}

#[test]
fn test_binary_operations_on_floats() {
    assert_operation_outputs(&[
        ("1.5 + 2.5", "4.0"),
        ("3.0 * 2.5", "7.5"),
        ("10.0 / 4.0", "2.5"),
        ("1.0 - 0.5", "0.5"),
        ("-1.5 * 2.0", "-3.0"),
    ]);
}

#[test]
fn test_min_int_div_neg_one_stops_the_program() {
    // The quotient is one past the maximum, so it overflows; the remainder is
    // computed by the same division and stops the program the same way.
    for op in ["/", "%"] {
        assert_runtime_error(
            &format!(
                "fn main() int:\n    var x = -9223372036854775807 - 1\n    var y = -1\n    println(f\"{{x {op} y}}\")\n    return 0\n"
            ),
            "integer overflow: the result does not fit its type",
        );
    }
}

#[test]
fn test_128bit_division_and_remainder() {
    // Division emits a divide-by-zero guard whose zero has to be built at the
    // operand's width. There is no 128-bit form of the constant instruction, so
    // a guard built the narrow way takes the compiler down before it can emit
    // the divide at all — which is why `+`, `-` and `*` were unaffected.
    assert_runs_with_output(
        r#"
let a i128 = 100
let b i128 = 7
println(f"{a / b} {a % b}")
"#,
        "14 2",
    );
    // A dividend past the 64-bit range proves the divide itself spans the whole
    // value rather than its low word.
    assert_runs_with_output(
        r#"
let a i128 = 170141183460469231731687303715884105727
let b i128 = 3
println(f"{a / b} {a % b}")
"#,
        "56713727820156410577229101238628035242 1",
    );
    // A negative dividend truncates toward zero, as the narrower widths do.
    assert_runs_with_output(
        r#"
let a i128 = -100
let b i128 = 7
println(f"{a / b} {a % b}")
"#,
        "-14 -2",
    );
}

#[test]
fn test_128bit_division_overflow_and_signs() {
    // The one case that overflows — the most negative value over -1, whose true
    // quotient is one past the maximum — stops the program, as it does at the
    // narrower widths; its remainder comes from the same division and stops too.
    //
    // The most negative value is built rather than written: it cannot be spelled
    // as a literal, since the magnitude is parsed before the sign is applied.
    for op in ["/", "%"] {
        assert_runtime_error(
            &format!(
                "let maxv i128 = 170141183460469231731687303715884105727\nlet minv = -maxv - 1\nlet neg1 i128 = -1\nprintln(f\"{{minv {op} neg1}}\")\n"
            ),
            "integer overflow: the result does not fit its type",
        );
    }
    // A remainder takes the sign of the dividend, not the divisor.
    assert_runs_with_output(
        r#"
let a i128 = -100
let b i128 = -7
let c i128 = 100
println(f"{a / b} {a % b} {c / b} {c % b}")
"#,
        "14 -2 -14 2",
    );
}

#[test]
fn test_128bit_division_by_zero_reports_rather_than_crashes() {
    // The guard that catches a zero divisor is the very thing that used to take
    // the compiler down at this width, so it is worth pinning that it both
    // compiles and still fires.
    assert_runtime_error(
        r#"
let a i128 = 100
var z i128 = 7
z -= 7
println(f"{a / z}")
"#,
        "division by zero",
    );
}

#[test]
fn test_constant_fold_i128_negation() {
    assert_runs_with_output(
        r#"
let a i128 = 170141183460469231731687303715884105727
let neg_a i128 = -a
println(f"{neg_a}")
"#,
        "-170141183460469231731687303715884105727",
    );
}

#[test]
fn test_128bit_unsigned_division_and_remainder() {
    // The unsigned path takes the same guard, and its dividend has no sign bit
    // to mistake for one — read as signed, this dividend is -1 and the quotient
    // would come out as zero.
    //
    // The largest `u128` is reached by complementing zero rather than written
    // out: a literal that large cannot be spelled, because every integer literal
    // is parsed into an `i128` first and this one does not fit.
    assert_runs_with_output(
        r#"
let a u128 = ~(0 as u128)
let b u128 = 7
println(f"{a / b} {a % b}")
"#,
        "48611766702991209066196372490252601636 3",
    );
    assert_runs_with_output(
        r#"
let a u128 = 100
let b u128 = 7
println(f"{a / b} {a % b}")
"#,
        "14 2",
    );
}
