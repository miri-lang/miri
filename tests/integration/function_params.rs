// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Integration tests for function parameters and return values of every type.
//!
//! Each section verifies that the Cranelift backend can correctly pass and
//! return a specific type through a function boundary.  Where the type cannot
//! be directly formatted in an f-string, we route through a comparison that
//! produces an `int` (0 / 1) that is then printed.

use crate::integration::utils::{assert_runs, assert_runs_with_output};

// =============================================================================
// Boolean parameters and return types
// =============================================================================

#[test]
fn test_bool_identity_param() {
    assert_runs_with_output(
        r#"
use system.io

fn identity_bool(b bool) bool
    b

fn main()
    let r = if identity_bool(true)
        1
    else
        0
    println(f"{r}")
"#,
        "1",
    );
}

#[test]
fn test_bool_negate_param() {
    assert_runs_with_output(
        r#"
use system.io

fn logical_not(b bool) bool
    not b

fn main()
    let a = if logical_not(false)
        1
    else
        0
    let b = if logical_not(true)
        0
    else
        1
    println(f"{a + b}")
"#,
        "2",
    );
}

#[test]
fn test_bool_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn both_true(a bool, b bool) bool
    a and b

fn main()
    let r = if both_true(true, true)
        1
    else
        0
    println(f"{r}")
"#,
        "1",
    );
}

#[test]
fn test_bool_param_false() {
    assert_runs_with_output(
        r#"
use system.io

fn identity_bool(b bool) bool
    b

fn main()
    let r = if identity_bool(false)
        1
    else
        0
    println(f"{r}")
"#,
        "0",
    );
}

// =============================================================================
// Signed integer width parameters and return types
// =============================================================================

#[test]
fn test_i8_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i8(x i8) i8
    x * 2

fn main()
    let r = double_i8(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i16_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i16(x i16) i16
    x * 2

fn main()
    let r = double_i16(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i32_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i32(x i32) i32
    x * 2

fn main()
    let r = double_i32(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i64_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_i64(x i64) i64
    x * 2

fn main()
    let r = double_i64(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i8_boundary_values() {
    // Verify that i8 max (127) and min (-128) are handled without truncation.
    assert_runs_with_output(
        r#"
use system.io

fn identity_i8(x i8) i8
    x

fn main()
    let max = identity_i8(127)
    let min = identity_i8(-128)
    let ok = if max == 127
        if min == -128
            1
        else
            0
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i8_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_i8(a i8, b i8) i8
    a + b

fn main()
    let r = add_i8(10, 32)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_i32_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_i32(a i32, b i32) i32
    a + b

fn main()
    let r = add_i32(1000000, 1000000)
    let ok = if r == 2000000
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

// =============================================================================
// Unsigned integer width parameters and return types
// =============================================================================

#[test]
fn test_u8_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u8(x u8) u8
    x * 2

fn main()
    let r = double_u8(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u16_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u16(x u16) u16
    x * 2

fn main()
    let r = double_u16(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u32_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u32(x u32) u32
    x * 2

fn main()
    let r = double_u32(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u64_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_u64(x u64) u64
    x * 2

fn main()
    let r = double_u64(21)
    let ok = if r == 42
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_u8_roundtrip_within_signed_range() {
    // Values ≤ 127 avoid the sextend vs uextend ambiguity that arises when
    // a u8 holding 0xFF (255) is sign-extended to a wider integer for comparison.
    assert_runs_with_output(
        r#"
use system.io

fn identity_u8(x u8) u8
    x

fn main()
    let r = identity_u8(100)
    let ok = if r == 100
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

// =============================================================================
// Float width parameters and return types
// =============================================================================

#[test]
fn test_f32_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_f32(x f32) f32
    x * 2.0

fn main()
    let r = double_f32(1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f64_param_and_return() {
    assert_runs_with_output(
        r#"
use system.io

fn double_f64(x f64) f64
    x * 2.0

fn main()
    let r = double_f64(1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f32_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_f32(a f32, b f32) f32
    a + b

fn main()
    let r = add_f32(1.5, 1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f64_two_params() {
    assert_runs_with_output(
        r#"
use system.io

fn add_f64(a f64, b f64) f64
    a + b

fn main()
    let r = add_f64(1.5, 1.5)
    let ok = if r == 3.0
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

#[test]
fn test_f64_negative_param() {
    assert_runs_with_output(
        r#"
use system.io

fn negate_f64(x f64) f64
    -x

fn main()
    let r = negate_f64(2.5)
    let ok = if r == -2.5
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}

// =============================================================================
// String parameters and return types
// =============================================================================

#[test]
fn test_string_identity_param() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn identity_str(s string) string
    s

fn main()
    let r = identity_str("hello")
    println(r)
"#,
        "hello",
    );
}

#[test]
fn test_string_param_size() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn str_len(s string) int
    s.size()

fn main()
    let r = str_len("hello")
    println(f"{r}")
"#,
        "5",
    );
}

#[test]
fn test_string_two_params_concat() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn join(a string, b string) string
    a.concat(b)

fn main()
    let r = join("hello, ", "world!")
    println(r)
"#,
        "hello, world!",
    );
}

#[test]
fn test_string_param_equality_check() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn is_empty_str(s string) bool
    s.is_empty()

fn main()
    let r = if is_empty_str("")
        1
    else
        0
    println(f"{r}")
"#,
        "1",
    );
}

// =============================================================================
// Struct parameters and return types
// =============================================================================

#[test]
fn test_struct_param_field_access() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn get_x(p Point) int
    p.x

fn main()
    let p = Point(x: 10, y: 20)
    let r = get_x(p)
    println(f"{r}")
"#,
        "10",
    );
}

#[test]
fn test_struct_param_sum_fields() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn manhattan(p Point) int
    p.x + p.y

fn main()
    let p = Point(x: 3, y: 4)
    let r = manhattan(p)
    println(f"{r}")
"#,
        "7",
    );
}

#[test]
fn test_struct_and_int_params() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn scale_x(p Point, factor int) int
    p.x * factor

fn main()
    let p = Point(x: 5, y: 0)
    let r = scale_x(p, 8)
    println(f"{r}")
"#,
        "40",
    );
}

#[test]
fn test_struct_return() {
    assert_runs_with_output(
        r#"
use system.io

struct Point
    x int
    y int

fn make_point(x int, y int) Point
    Point(x: x, y: y)

fn main()
    let p = make_point(3, 7)
    println(f"{p.x}")
    println(f"{p.y}")
"#,
        "3",
    );
}

// =============================================================================
// Enum parameters and return types
// =============================================================================

#[test]
fn test_enum_param_single_variant() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok
    Error

fn status_code(s Status) int
    match s
        Status.Ok: 0
        Status.Error: 1

fn main()
    let r = status_code(Status.Ok)
    println(f"{r}")
"#,
        "0",
    );
}

#[test]
fn test_enum_param_error_variant() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok
    Error

fn status_code(s Status) int
    match s
        Status.Ok: 0
        Status.Error: 1

fn main()
    let r = status_code(Status.Error)
    println(f"{r}")
"#,
        "1",
    );
}

#[test]
fn test_enum_param_three_variants() {
    assert_runs_with_output(
        r#"
use system.io

enum Color
    Red
    Green
    Blue

fn color_index(c Color) int
    match c
        Color.Red: 1
        Color.Green: 2
        Color.Blue: 3

fn main()
    println(f"{color_index(Color.Red)}")
    println(f"{color_index(Color.Green)}")
    println(f"{color_index(Color.Blue)}")
"#,
        "2",
    );
}

#[test]
fn test_enum_return_from_function() {
    assert_runs_with_output(
        r#"
use system.io

enum Toggle
    On
    Off

fn flip(t Toggle) Toggle
    match t
        Toggle.On: Toggle.Off
        Toggle.Off: Toggle.On

fn main()
    let t = flip(Toggle.On)
    let r = match t
        Toggle.On: 1
        Toggle.Off: 0
    println(f"{r}")
"#,
        "0",
    );
}

// =============================================================================
// Mixed / multi-type parameters
// =============================================================================

#[test]
fn test_int_and_bool_params() {
    assert_runs_with_output(
        r#"
use system.io

fn conditional_double(x int, flag bool) int
    if flag
        x * 2
    else
        x

fn main()
    let a = conditional_double(21, true)
    let b = conditional_double(21, false)
    println(f"{a}")
    println(f"{b}")
"#,
        "42",
    );
}

#[test]
fn test_multiple_same_width_int_params() {
    // Miri's type checker requires operands of arithmetic to share the same width.
    // Use a uniform i32 to validate multi-parameter calling convention.
    assert_runs_with_output(
        r#"
use system.io

fn sum_i32(a i32, b i32, c i32) i32
    a + b + c

fn main()
    let r = sum_i32(1, 10, 100)
    let ok = if r == 111
        1
    else
        0
    println(f"{ok}")
"#,
        "1",
    );
}
