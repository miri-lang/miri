// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A 128-bit payload is twice as wide as the pointer-sized slot every other
//! payload fits in, so these tests check that the value read back out of a
//! match is the value that was stored — both halves of it. Every value is
//! larger than 2^64, so a lost or swapped high word shows up in the output.

use super::utils::*;

#[test]
fn test_enum_i128_payload_reads_back_the_stored_value() {
    assert_runs_with_output(
        r#"
public enum Val
    Big(i128)

fn main()
    let v = Val.Big(42)
    match v
        Val.Big(x): println(f"{x}")
        "#,
        "42",
    );
}

#[test]
fn test_enum_i128_payload_keeps_its_high_word() {
    assert_runs_with_output(
        r#"
public enum Val
    Big(i128)

fn main()
    let v = Val.Big(36893488147419103274)
    match v
        Val.Big(x): println(f"{x}")
        "#,
        "36893488147419103274",
    );
}

#[test]
fn test_enum_i128_payload_near_the_maximum() {
    assert_runs_with_output(
        r#"
public enum Val
    Big(i128)

fn main()
    let v = Val.Big(170141183460469231731687303715884105727)
    match v
        Val.Big(x): println(f"{x}")
        "#,
        "170141183460469231731687303715884105727",
    );
}

#[test]
fn test_enum_i128_payload_then_int_field() {
    assert_runs_with_output(
        r#"
public enum Sized
    Big(i128, int)

fn main()
    let s = Sized.Big(36893488147419103274, 7)
    match s
        Sized.Big(a, b): println(f"{a} {b}")
        "#,
        "36893488147419103274 7",
    );
}

#[test]
fn test_enum_int_field_then_u128_payload() {
    assert_runs_with_output(
        r#"
public enum Sized
    Big(int, u128)

fn main()
    let s = Sized.Big(7, 1267650600228229401496703205383)
    match s
        Sized.Big(a, b): println(f"{a} {b}")
        "#,
        "7 1267650600228229401496703205383",
    );
}

#[test]
fn test_enum_i128_payload_beside_a_narrow_variant() {
    assert_runs_with_output(
        r#"
public enum Mixed
    Small(int, int)
    Big(i128)

fn main()
    let values = [Mixed.Small(3, 4), Mixed.Big(-36893488147419103274)]
    for v in values
        match v
            Mixed.Small(a, b): println(f"{a} {b}")
            Mixed.Big(x): println(f"{x}")
        "#,
        "3 4\n-36893488147419103274",
    );
}

#[test]
fn test_enum_i128_payload_beside_a_managed_field() {
    assert_runs_with_output(
        r#"
public enum Tagged
    Named(String, i128)

fn main()
    let t = Tagged.Named("id", 36893488147419103274)
    match t
        Tagged.Named(name, value): println(f"{name}={value}")
        "#,
        "id=36893488147419103274",
    );
}

#[test]
fn test_option_i128_some_keeps_its_high_word() {
    assert_runs_with_output(
        r#"
fn main()
    let big i128 = 36893488147419103274
    let opt = Some(big)
    match opt
        Some(v): println(f"{v}")
        None: println("none")
        "#,
        "36893488147419103274",
    );
}

#[test]
fn test_generic_enum_i128_payload_keeps_its_high_word() {
    assert_runs_with_output(
        r#"
public enum Holder<T>
    Held(T)

fn main()
    let big i128 = 36893488147419103274
    let h Holder<i128> = Holder.Held(big)
    match h
        Holder.Held(v): println(f"{v}")
        "#,
        "36893488147419103274",
    );
}

/// A managed payload bound through a type parameter sits in a slot widened by
/// an `i128` in the same enum, so construction, the match's reads and the drop
/// guard must all place it at the widened offset. A drop guard at any other
/// offset releases the wrong word: the heap string leaks, or the program
/// crashes on a non-pointer.
#[test]
fn test_generic_enum_managed_payload_beside_an_i128_is_released() {
    assert_runs_with_output(
        r#"
public enum Tagged<T>
    Wide(T, i128)
    Slim(String)

fn describe(n int) Tagged<String>
    if n % 2 == 0
        return Tagged.Wide(f"even {n}", 36893488147419103274)
    return Tagged.Slim(f"odd {n}")

fn main()
    var i = 0
    while i < 4
        let t = describe(i)
        match t
            Tagged.Wide(name, value): println(f"{name} {value}")
            Tagged.Slim(name): println(name)
        i = i + 1
        "#,
        "even 0 36893488147419103274\nodd 1\neven 2 36893488147419103274\nodd 3",
    );
}
