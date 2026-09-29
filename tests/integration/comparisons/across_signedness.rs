// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An unsigned and a signed integer compare by the numbers they hold, not by
//! their bits read either way.

use super::utils::*;

#[test]
fn an_unsigned_value_compares_with_a_negative_one_by_its_number() {
    assert_runs_with_output(
        r#"
fn main()
    let m u64 = 18446744073709551615
    let n int = -1
    let small u8 = 200
    let neg i8 = -1
    println(f"{m > -1} {n < m} {m == n} {small > neg} {neg >= small} {m != n}")
"#,
        "true true false true false true",
    );
}

#[test]
fn a_128_bit_unsigned_value_compares_with_a_signed_one_by_its_number() {
    assert_runs_with_output(
        r#"
fn main()
    let top u128 = 340282366920938463463374607431768211455
    let three u128 = 3
    let minus i128 = -5
    let seven i128 = 7
    println(f"{top > minus} {minus < top} {three < seven} {seven <= three} {three == minus}")
"#,
        "true true true false false",
    );
}
