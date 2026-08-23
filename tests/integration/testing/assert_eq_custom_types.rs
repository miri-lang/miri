// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `assert_eq` / `assert_ne` over types beyond the primitive scalars: enums,
//! `Result`, `Option`, structs, and classes that define their own equality.

use super::utils::*;

#[test]
fn test_assert_eq_accepts_enum_variants() {
    assert_runs_with_output(
        r#"
use system.testing.{assert_eq, assert_ne}

enum Color
    Red
    Green

fn main()
    assert_eq(Color.Red, Color.Red)
    assert_ne(Color.Red, Color.Green)
    println("enum compared")
"#,
        "enum compared",
    );
}

#[test]
fn test_assert_eq_enum_failure_names_both_variants() {
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}

enum Color
    Red
    Green

fn main()
    assert_eq(Color.Red, Color.Green)
"#,
        "expected Green, got Red",
    );
}

#[test]
fn test_assert_eq_renders_enum_payload_in_diff() {
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}

enum Shape
    Circle(int)
    Square(int)

fn main()
    assert_eq(Shape.Circle(3), Shape.Circle(4))
"#,
        "expected Circle(4), got Circle(3)",
    );
}

#[test]
fn test_assert_eq_compares_enum_string_payload_by_value() {
    // Built at runtime so the two payloads occupy different allocations: a
    // comparison that only checked addresses would fail this.
    assert_runs_with_output(
        r#"
use system.testing.{assert_eq}

fn s() String
    return "h" + "i"

enum Msg
    Text(String)
    Num(int)

fn main()
    assert_eq(Msg.Text(s()), Msg.Text(s()))
    println("payload compared by value")
"#,
        "payload compared by value",
    );
}

#[test]
fn test_assert_eq_accepts_result() {
    assert_runs_with_output(
        r#"
use system.testing.{assert_eq, assert_ne}

fn main()
    let a Result<int, String> = Result.Ok(42)
    let b Result<int, String> = Result.Ok(42)
    let e Result<int, String> = Result.Err("boom")
    assert_eq(a, b)
    assert_ne(a, e)
    println("result compared")
"#,
        "result compared",
    );
}

#[test]
fn test_assert_eq_result_failure_renders_both_sides() {
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}

fn main()
    let a Result<int, String> = Result.Ok(1)
    let b Result<int, String> = Result.Err("boom")
    assert_eq(a, b)
"#,
        "expected Err(boom), got Ok(1)",
    );
}

#[test]
fn test_assert_eq_accepts_option() {
    assert_runs_with_output(
        r#"
use system.testing.{assert_eq, assert_ne}

fn main()
    assert_eq(Some(3), Some(3))
    assert_ne(Some(3), Some(4))
    assert_ne(Some(3), None)
    println("option compared")
"#,
        "option compared",
    );
}

#[test]
fn test_assert_eq_option_failure_renders_none() {
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}

fn main()
    assert_eq(Some(1), None)
"#,
        "expected None, got Some(1)",
    );
}

#[test]
fn test_assert_eq_accepts_struct() {
    assert_runs_with_output(
        r#"
use system.testing.{assert_eq}

fn s() String
    return "te" + "st"

struct Pair
    name String
    count int

fn main()
    assert_eq(Pair(s(), 1), Pair(s(), 1))
    println("struct compared")
"#,
        "struct compared",
    );
}

#[test]
fn test_assert_eq_struct_failure_names_each_field() {
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}

struct Point
    x int
    y int

fn main()
    assert_eq(Point(1, 2), Point(3, 4))
"#,
        "expected Point(x=3, y=4), got Point(x=1, y=2)",
    );
}

#[test]
fn test_assert_eq_class_with_equals_uses_that_method() {
    assert_runs_with_output(
        r#"
use system.testing.{assert_eq}
use system.ops.{Equatable}

class Point implements Equatable
    var x int
    var y int
    public fn equals(other Self) bool
        return self.x == other.x and self.y == other.y

fn main()
    assert_eq(Point(1, 2), Point(1, 2))
    println("class compared by equals")
"#,
        "class compared by equals",
    );
}

#[test]
fn test_assert_eq_class_failure_renders_fields() {
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}
use system.ops.{Equatable}

class Point implements Equatable
    var x int
    var y int
    public fn equals(other Self) bool
        return self.x == other.x and self.y == other.y

fn main()
    assert_eq(Point(1, 2), Point(3, 4))
"#,
        "expected Point(x=3, y=4), got Point(x=1, y=2)",
    );
}

#[test]
fn test_assert_eq_rejects_class_without_equals() {
    // `==` on such a class answers "the same object". Letting an assertion
    // compare addresses would make a test pass or fail for a reason that has
    // nothing to do with the values it names.
    //
    // The refusal is raised while lowering to MIR, which `miri check` does not
    // reach, so this asserts against a full build rather than a type-check.
    assert_build_error(
        r#"
use system.testing.{assert_eq}

class Bare
    var x int

fn main()
    assert_eq(Bare(1), Bare(1))
"#,
        "defines no `equals` method",
    );
}

#[test]
fn test_assert_eq_float_payload_renders_as_written() {
    // A payload read at the base slot's width instead of its own renders a
    // float as its bit pattern; this pins the value the user actually wrote.
    assert_runtime_error(
        r#"
use system.testing.{assert_eq}

enum Reading
    Value(float)

fn main()
    assert_eq(Reading.Value(2.5), Reading.Value(3.5))
"#,
        "expected Value(3.5), got Value(2.5)",
    );
}
