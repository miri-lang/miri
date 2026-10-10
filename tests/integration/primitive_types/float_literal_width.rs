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

#[test]
fn test_let_bound_to_a_literal_takes_the_width_of_the_operand_beside_it() {
    // `let e = 0.04` names a number written in the source, so it adapts the way
    // the literal would: `y + e` computes at f32 and is accepted by the f32
    // parameter instead of being refused as an f64 store.
    assert_runs_with_output(
        r#"
fn twice(x f32) f32
    return x * 2.0

fn shifted(y f32) f32
    let e = 0.04
    return twice(y + e)

fn main()
    println(f"[{shifted(1.0)}]")
"#,
        "[2.08]",
    );
}

#[test]
fn test_let_bound_to_a_literal_narrows_where_an_f32_is_declared() {
    // Passed on its own, the binding rounds to f32 at the call, as the literal
    // would: 3.1415927, not the f64 value.
    assert_runs_with_output(
        r#"
fn ident(x f32) f32
    return x

fn main()
    let pi = 3.14159265358979
    let narrow f32 = pi
    println(f"[{ident(pi)}] [{narrow}]")
"#,
        "[3.1415927] [3.1415927]",
    );
}

#[test]
fn test_let_bound_to_a_literal_keeps_full_precision_where_nothing_narrows_it() {
    // One use at f32 does not fix the binding's width: every other use still
    // reads the value at f64, so 0.1 + 0.2 is not rounded back to 0.3.
    assert_runs_with_output(
        r#"
fn ident(x f32) f32
    return x

fn main()
    let a = 0.1
    println(f"[{ident(a)}]")
    println(f"[{a + 0.2}]")
"#,
        "[0.1]\n[0.30000000000000004]",
    );
}

#[test]
fn test_var_bound_to_a_literal_keeps_the_float_default() {
    // A `var` can be reassigned, so its value is not the literal any more by
    // the time it is read: it takes `float` at declaration, and an f32
    // parameter refuses it.
    assert_compiler_error(
        r#"
fn twice(x f32) f32
    return x * 2.0

fn main()
    var t = 0.04
    t = t + 0.01
    println(f"{twice(t)}")
"#,
        "expected f32, got float",
    );
}

#[test]
fn test_let_bound_to_a_computed_value_does_not_adapt() {
    // Only a number written in the source adapts. A binding holding a value
    // the program computed keeps the width it was computed at.
    assert_compiler_error(
        r#"
fn twice(x f32) f32
    return x * 2.0

fn scale() float
    return 0.5

fn main()
    let s = scale()
    println(f"{twice(s)}")
"#,
        "expected f32, got float",
    );
}

#[test]
fn test_arithmetic_of_literals_takes_the_width_of_the_operand_beside_it() {
    // `2.0 * 0.04` and `2.0 * e` are numbers written in the source and
    // nothing else, so each takes f32 as one number beside an f32, and the
    // f32 return accepts the quotient.
    assert_runs_with_output(
        r#"
fn by_literals(x f32) f32
    return x / (2.0 * 0.04)

fn by_binding(x f32) f32
    let e = 0.04
    return x / (2.0 * e)

fn main()
    println(f"[{by_literals(1.0)}] [{by_binding(1.0)}]")
"#,
        "[12.5] [12.5]",
    );
}

#[test]
fn test_arithmetic_of_literals_narrows_where_an_f32_is_returned() {
    assert_runs_with_output(
        r#"
fn eighth() f32
    let e = 0.5
    return e * 0.25

fn main()
    println(f"[{eighth()}]")
"#,
        "[0.125]",
    );
}

#[test]
fn test_arithmetic_with_a_computed_value_does_not_adapt() {
    // `s` holds a value the program computed at float, so `2.0 * s` is no
    // number written in the source, and stays a float the f32 return refuses.
    assert_compiler_error(
        r#"
fn half() float
    return 0.5

fn scaled() f32
    let s = half()
    return 2.0 * s
"#,
        "f32",
    );
}

#[test]
fn test_signed_arithmetic_of_literals_takes_the_width_beside_it() {
    assert_runs_with_output(
        r#"
fn twice(x f32) f32
    return x * 2.0

fn main()
    let e = 0.25
    let y f32 = 1.0
    println(f"[{twice(-(2.0 * e) + y)}]")
"#,
        "[1.0]",
    );
}
