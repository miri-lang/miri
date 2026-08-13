// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A decimal literal has no width of its own: it carries the value written in
//! the source and takes the width of the context that consumes it. With no
//! context to take a width from, it defaults to the widest float the target
//! can represent — `f64` on a CPU target.

use super::utils::*;

#[test]
fn test_untyped_literal_keeps_full_precision_through_an_f64_parameter() {
    // The literal is written at f64 precision and the parameter is f64, so the
    // value must survive the call unchanged. Rounding it to f32 anywhere along
    // the way surfaces as 3.140000104904175, which the bracket sentinels catch
    // — the output assertion is a substring match.
    assert_runs_with_output(
        r#"
fn ident(x f64) f64
    return x

fn main()
    println(f"[{ident(3.14)}]")
"#,
        "[3.14]",
    );
}

#[test]
fn test_untyped_literal_arithmetic_is_evaluated_at_f64() {
    // 0.1 + 0.2 is the canonical discriminator between the two widths: f32
    // rounds the sum back to exactly 0.3, f64 does not.
    assert_runs_with_output(
        r#"
fn main()
    println(f"[{0.1 + 0.2}]")
"#,
        "[0.30000000000000004]",
    );
}

#[test]
fn test_untyped_literal_compares_equal_to_a_parsed_f64() {
    assert_runs_with_output(
        r#"
let s = "3.14"
match s.to_float()
    Some(f): println(f"{f == 3.14}")
    None: println("failed")
"#,
        "true",
    );
}

#[test]
fn test_declared_f32_binding_narrows_the_literal() {
    // The declared width wins over the default: the literal is rounded to f32
    // at the binding rather than rejected as an f64-to-f32 assignment.
    assert_runs_with_output(
        r#"
fn main()
    let x f32 = 3.14159265358979
    println(f"[{x}]")
"#,
        "[3.1415927]",
    );
}

#[test]
fn test_f32_parameter_narrows_the_literal_at_the_call_site() {
    assert_runs_with_output(
        r#"
fn takes(x f32) f32
    return x

fn main()
    println(f"[{takes(3.14159265358979)}]")
"#,
        "[3.1415927]",
    );
}

#[test]
fn test_f32_return_type_narrows_the_literal() {
    assert_runs_with_output(
        r#"
fn wide() f32
    return 3.14159265358979

fn main()
    println(f"[{wide()}]")
"#,
        "[3.1415927]",
    );
}

#[test]
fn test_f32_array_element_assignment_narrows_the_literal() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    var a = Array<f32,2>()
    a[0] = 3.14159265358979
    println(f"[{a[0]}]")
"#,
        "[3.1415927]",
    );
}

#[test]
fn test_f32_struct_field_initializer_narrows_the_literal() {
    assert_runs_with_output(
        r#"
struct Point
    x f32
    y f32

fn main()
    let p = Point(3.14159265358979, 0.0)
    println(f"[{p.x}]")
"#,
        "[3.1415927]",
    );
}

#[test]
fn test_f64_binding_keeps_the_literal_at_full_precision() {
    assert_runs_with_output(
        r#"
fn main()
    let x f64 = 3.14
    println(f"[{x}]")
"#,
        "[3.14]",
    );
}

#[test]
fn test_narrowing_applies_through_a_unary_sign() {
    assert_runs_with_output(
        r#"
fn takes(x f32) f32
    return x

fn main()
    println(f"[{takes(-3.14159265358979)}]")
"#,
        "[-3.1415927]",
    );
}

#[test]
fn test_only_a_literal_narrows_never_a_value() {
    // Narrowing is a property of the literal, not of the type it lands on: a
    // value that already has f64 width keeps it, and passing it where f32 is
    // required stays the error it always was. Otherwise the default width
    // would silently round every f64 in the program down at the first f32
    // boundary it met.
    assert_compiler_error(
        r#"
fn wants_f32(x f32) f32
    return x

fn main()
    let x f64 = 3.14159265358979
    println(f"{wants_f32(x)}")
"#,
        "f32",
    );
}
