// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Arithmetic written against a generic parameter, in a body instantiated at a
//! concrete type. The value the operator produces is held in a temp, and that
//! temp must be typed at the instantiation rather than at the parameter — a
//! temp left at the parameter is pointer-width integer, which truncates a float
//! result and makes the backend reject a second operation reading it.

use super::utils::*;

#[test]
fn test_generic_addition_through_a_lambda_at_float() {
    assert_runs_with_output(
        r#"

fn sum2<T>(a T, b T) T
    let f = fn(x T, y T) T: x + y
    return f(a, b)

fn main()
    println(f"{sum2(1.5, 2.25)}")
    "#,
        "3.75",
    );
}

#[test]
fn test_generic_nested_addition_at_float() {
    assert_runs_with_output(
        r#"

fn sum3<T>(a T, b T) T
    return (a + b) + a

fn main()
    println(f"{sum3(1.5, 2.25)}")
    "#,
        "5.25",
    );
}

#[test]
fn test_generic_nested_addition_at_f32() {
    assert_runs_with_output(
        r#"

fn sum3<T>(a T, b T) T
    return (a + b) + a

fn main()
    let a f32 = 1.5
    let b f32 = 2.25
    println(f"{sum3(a, b)}")
    "#,
        "5.25",
    );
}

#[test]
fn test_generic_nested_addition_at_int_is_unchanged() {
    assert_runs_with_output(
        r#"

fn sum3<T>(a T, b T) T
    return (a + b) + a

fn main()
    println(f"{sum3(3, 4)}")
    "#,
        "10",
    );
}

#[test]
fn test_generic_mixed_arithmetic_at_float() {
    assert_runs_with_output(
        r#"

fn blend<T>(a T, b T) T
    return ((a * b) - a) / b

fn main()
    println(f"{blend(4.0, 2.0)}")
    "#,
        "2",
    );
}

#[test]
fn test_generic_comparison_at_float_still_orders() {
    assert_runs_with_output(
        r#"

fn smaller<T>(a T, b T) bool
    return a < b

fn main()
    println(f"{smaller(1.5, 2.25)}")
    println(f"{smaller(2.25, 1.5)}")
    "#,
        "true\nfalse",
    );
}

#[test]
fn test_generic_comparison_of_a_computed_sum_at_float() {
    assert_runs_with_output(
        r#"

fn sum_exceeds<T>(a T, b T) bool
    return (a + b) > a

fn main()
    println(f"{sum_exceeds(1.5, 2.25)}")
    println(f"{sum_exceeds(1.5, 0.0 - 2.25)}")
    "#,
        "true\nfalse",
    );
}

#[test]
fn test_generic_field_compound_assignment_at_float() {
    assert_runs_with_output(
        r#"

class Box<T>
    v T

    fn new(v T)
        self.v = v

    fn bump(d T) T
        self.v += d
        return self.v

fn main()
    let b = Box<float>(1.5)
    println(f"{b.bump(2.25)}")
    "#,
        "3.75",
    );
}

#[test]
fn test_generic_compound_assignment_at_float() {
    assert_runs_with_output(
        r#"

fn accumulate<T>(a T, b T) T
    var total = a
    total += b
    total += a
    return total

fn main()
    println(f"{accumulate(1.5, 2.25)}")
    "#,
        "5.25",
    );
}

#[test]
fn test_generic_arithmetic_in_a_class_method_at_float() {
    assert_runs_with_output(
        r#"

class Pair<T>
    first T
    second T

    fn new(first T, second T)
        self.first = first
        self.second = second

    fn combined() T
        return (self.first + self.second) + self.first

fn main()
    let p = Pair<float>(1.5, 2.25)
    println(f"{p.combined()}")
    "#,
        "5.25",
    );
}
