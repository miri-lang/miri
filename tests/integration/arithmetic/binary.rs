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
fn test_min_int_div_neg_one() {
    let source = r#"
fn main() int:
    let x = -9223372036854775808
    let y = -1
    let div_res = x / y
    let rem_res = x % y
    println(f"{div_res} {rem_res}")
    return 0
"#;
    assert_runs_with_output(source, "-9223372036854775808 0\n");
}
