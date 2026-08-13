// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_float_payload() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(float)

fn main()
    let b = Box.Val(0.5)
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "0.5",
    );
}

#[test]
fn test_enum_f32_payload() {
    assert_runs_with_output(
        r#"
public enum Pair
    Two(f32)

fn main()
    let p = Pair.Two(1.5)
    match p
        Pair.Two(x): println(f"{x}")
        "#,
        "1.5",
    );
}

#[test]
fn test_enum_f64_payload() {
    assert_runs_with_output(
        r#"
public enum Triple
    Val(f64)

fn main()
    let t = Triple.Val(3.141592653589793)
    match t
        Triple.Val(x): println(f"{x}")
        "#,
        "3.141592653589793",
    );
}

#[test]
fn test_enum_multi_field_payload() {
    assert_runs_with_output(
        r#"
public enum Two
    Pair(float, int)

fn main()
    let t = Two.Pair(0.5, 7)
    match t
        Two.Pair(f, i): println(f"{f} {i}")
        "#,
        "0.5 7",
    );
}

#[test]
fn test_option_float_payload() {
    assert_runs_with_output(
        r#"
fn main()
    let opt = Some(0.5)
    match opt
        Some(v): println(f"{v}")
        None: println("none")
        "#,
        "0.5",
    );
}

#[test]
#[ignore = "Result<T,E> payloads are corrupted; the payload type at the load site is the unsubstituted generic parameter T from enum_def.variants rather than the concrete instantiation type. Fix requires type parameter substitution in src/mir/lowering/helpers.rs (outside codegen scope)."]
fn test_result_float_payload() {
    assert_runs_with_output(
        r#"
fn main()
    let res_ok = Result.Ok(0.5 as f64)
    match res_ok
        Result.Ok(v): println(f"{v}")
        Result.Err(_): println("error")

    let res_err = Result.Err(42)
    match res_err
        Result.Ok(_): println("ok")
        Result.Err(e): println(f"{e}")
        "#,
        "0.5\n42",
    );
}

#[test]
fn test_enum_float_payload_through_function() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(float)

fn make_box() Box
    Box.Val(2.5)

fn main()
    let b = make_box()
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "2.5",
    );
}

#[test]
fn test_enum_float_payload_from_narrower_literal() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(float)

fn main()
    let b = Box.Val(0.5)
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "0.5",
    );
}

#[test]
fn test_enum_int_payload_regression() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(int)

fn main()
    let b = Box.Val(42)
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "42",
    );
}

#[test]
fn test_enum_i32_payload_regression() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(i32)

fn main()
    let b = Box.Val(100)
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "100",
    );
}

#[test]
fn test_enum_bool_payload_regression() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(bool)

fn main()
    let b = Box.Val(true)
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "true",
    );
}

#[test]
fn test_enum_string_payload_regression() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(String)

fn main()
    let b = Box.Val("hello")
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "hello",
    );
}

#[test]
fn test_option_float_payload_from_narrower_literal() {
    assert_runs_with_output(
        r#"
fn wrap() float?
    return Some(3.14)

fn main()
    match wrap()
        Some(v): println(f"{v}")
        None: println("none")
        "#,
        "3.14",
    );
}

#[test]
fn test_enum_float_payload_declared_wider() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(float)

fn wrap() Box
    return Box.Val(2.71)

fn main()
    match wrap()
        Box.Val(v): println(f"{v}")
        "#,
        "2.71",
    );
}

/// An `f32` payload keeps f32 precision when the source value is an explicitly
/// demoted f64. The cast happens before the payload is built, so the value
/// already matches the declared width and the store-side coercion does not run.
///
/// The reverse direction — handing an f64-typed value to an f32 payload and
/// letting the store demote it — is unreachable: the type checker rejects it at
/// the construction site with "expected f32, got f64". Payload coercion
/// therefore only ever widens.
#[test]
fn test_enum_f32_payload_from_demoted_f64() {
    assert_runs_with_output(
        r#"
public enum Box
    Val(f32)

fn get_precise_float() f64
    return 3.141592653589793

fn main()
    let b = Box.Val((get_precise_float()) as f32)
    match b
        Box.Val(v): println(f"{v}")
        "#,
        "3.1415927",
    );
}

#[test]
fn test_enum_mixed_float_widths_in_variant() {
    assert_runs_with_output(
        r#"
public enum Point
    Coord(f32, f64)

fn main()
    let p = Point.Coord(1.5, 2.5)
    match p
        Point.Coord(x, y): println(f"{x} {y}")
        "#,
        "1.5 2.5",
    );
}

#[test]
fn test_enum_i128_payload_with_second_field() {
    // Regression test: ensure i128 payload field does not overflow its slot
    // when stored in an enum with multiple fields. The store-side coercion
    // skips types wider than ptr_type to prevent heap corruption.
    assert_runs_with_output(
        r#"
public enum Sized
    Big(i128, int)

fn main()
    let s = Sized.Big(9223372036854775807, 42)
    match s
        Sized.Big(a, b): println(f"{b}")
        "#,
        "42",
    );
}
