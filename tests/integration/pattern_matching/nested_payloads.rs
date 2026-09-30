// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A variant or literal written inside another pattern's payload is a test
//! the arm must pass: `Some(Shape.Circle(s))` matches only a `Circle`, and
//! binds the circle's own payload.

use super::utils::*;

const SHAPE: &str = r#"
enum Shape
    Circle(String)
    Square(int)
"#;

#[test]
fn test_a_variant_nested_in_some_binds_its_payload() {
    assert_runs_with_output(
        &format!(
            "{SHAPE}{}",
            r#"
fn main()
    let opt = Some(Shape.Circle("b" + ""))
    match opt
        Some(Shape.Circle(s)): println(f"circle {s}")
        Some(Shape.Square(n)): println(f"square {n}")
        None: println("none")
"#
        ),
        "circle b",
    );
}

#[test]
fn test_a_variant_nested_in_some_tests_its_discriminant() {
    assert_runs_with_output(
        &format!(
            "{SHAPE}{}",
            r#"
fn describe(opt Shape?) String
    match opt
        Some(Shape.Circle(s)): f"circle {s}"
        Some(Shape.Square(n)): f"square {n}"
        None: "none"

fn main()
    let name = "c"
    let a = describe(Some(Shape.Square(4)))
    let b = describe(Some(Shape.Circle(f"{name}1")))
    let c = describe(None)
    println(f"{a}|{b}|{c}")
"#
        ),
        "square 4|circle c1|none",
    );
}

#[test]
fn test_a_nested_variant_that_does_not_match_falls_to_the_catch_all() {
    assert_runs_with_output(
        &format!(
            "{SHAPE}{}",
            r#"
fn main()
    let opt = Some(Shape.Square(9))
    match opt
        Some(Shape.Circle(s)): println(f"circle {s}")
        _: println("other")
"#
        ),
        "other",
    );
}

#[test]
fn test_a_variant_nested_in_result_ok_binds_its_payload() {
    assert_runs_with_output(
        &format!(
            "{SHAPE}{}",
            r#"
fn make(flag bool) Result<Shape, String>
    if flag
        return Result.Ok(Shape.Square(4))
    return Result.Ok(Shape.Circle("r" + "1"))

fn show(r Result<Shape, String>) String
    match r
        Result.Ok(Shape.Square(n)): f"ok square {n}"
        Result.Ok(Shape.Circle(s)): f"ok circle {s}"
        Result.Err(e): f"err {e}"

fn main()
    println(f"{show(make(true))}|{show(make(false))}")
"#
        ),
        "ok square 4|ok circle r1",
    );
}

#[test]
fn test_three_levels_of_nested_variants_bind_the_innermost_payload() {
    assert_runs_with_output(
        &format!(
            "{SHAPE}{}",
            r#"
enum Box
    Wrap(Shape?)
    Empty

fn show(b Box?) String
    match b
        Some(Box.Wrap(Some(Shape.Circle(s)))): f"deep circle {s}"
        Some(Box.Wrap(Some(Shape.Square(n)))): f"deep square {n}"
        Some(Box.Wrap(None)): "wrap none"
        Some(Box.Empty): "empty"
        None: "none"

fn main()
    let t = "t"
    let a = show(Some(Box.Wrap(Some(Shape.Circle(f"{t}3")))))
    let b = show(Some(Box.Wrap(Some(Shape.Square(3)))))
    let c = show(Some(Box.Wrap(None)))
    let d = show(Some(Box.Empty))
    println(f"{a}|{b}|{c}|{d}")
"#
        ),
        "deep circle t3|deep square 3|wrap none|empty",
    );
}

#[test]
fn test_an_integer_literal_nested_in_some_is_tested() {
    assert_runs_with_output(
        r#"
fn show(o int?) String
    match o
        Some(0): "zero"
        Some(n): f"n {n}"
        None: "none"

fn main()
    println(f"{show(Some(0))}|{show(Some(5))}|{show(None)}")
"#,
        "zero|n 5|none",
    );
}

/// A string literal in a payload position has no test yet. It is refused
/// rather than bound unchecked, which would take the arm for any string.
#[test]
fn test_a_string_literal_nested_in_some_is_refused_until_it_can_be_tested() {
    assert_build_error(
        r#"
fn show(o String?) String
    match o
        Some("a"): "is a"
        Some(s): f"other {s}"
        None: "none"

fn main()
    println(show(Some("b")))
"#,
        "only variants and integer or boolean literals can be nested",
    );
}

#[test]
fn test_variants_nested_in_a_tuple_are_tested_and_bound() {
    assert_runs_with_output(
        r#"
fn pair(a int?, b int?) String
    match (a, b)
        (Some(x), Some(y)): f"both {x} {y}"
        (Some(x), None): f"left {x}"
        (None, Some(y)): f"right {y}"
        _: "neither"

fn main()
    println(f"{pair(Some(1), Some(2))}|{pair(Some(3), None)}|{pair(None, Some(4))}|{pair(None, None)}")
"#,
        "both 1 2|left 3|right 4|neither",
    );
}

/// A `None` payload fills the whole word of its slot. Written one byte wide,
/// the other bytes kept what the allocator left there, and releasing the value
/// read them as a pointer: invisible where fresh memory is zero, a crash where
/// it is not. `MallocScribble` makes every fresh block non-zero on macOS, which
/// is how glibc hands blocks back.
#[test]
fn a_none_payload_is_released_whatever_memory_held_before() {
    let result = crate::utils::miri_run_with_env(
        r#"
enum Box
    Wrap(String?)
    Empty

fn main()
    let w = Box.Wrap(None)
    match w
        Box.Wrap(None): println("wrap none")
        Box.Wrap(Some(s)): println(s)
        Box.Empty: println("empty")
"#,
        "MallocScribble",
        "1",
    );
    assert!(result.success, "{}", result.output());
    assert!(result.stdout.contains("wrap none"), "{}", result.output());
}
