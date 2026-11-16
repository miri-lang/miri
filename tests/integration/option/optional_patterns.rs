// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The variants of `T?` are the declared `Option`'s: each is reached written
//! bare (`Some(x)`, `None`) or through the enum (`Option.Some(x)`,
//! `Option.None`), as a value and as a pattern, and a match on an optional is
//! checked against the declaration's two variants whichever way the subject's
//! type is spelled.
//!
//! Each program that runs does so under the MIR verifier and the heap guard.

use super::super::utils::*;

#[test]
fn test_the_optional_variants_are_reachable_through_the_declared_enum() {
    assert_heap_guard_output(
        r#"
use system.io

fn take(o int?) int
    return o ?? -1

fn main()
    let a = Option.Some(3)
    let b int? = Option.None
    let c Option<int> = Option.Some(9)
    println(f"{take(a)} {take(b)} {take(c)}")
"#,
        "3 -1 9",
    );
}

#[test]
fn test_an_absent_variant_written_through_the_enum_is_passed_as_an_argument() {
    assert_heap_guard_output(
        r#"
use system.io

fn take(o String?) String
    return o ?? "none"

fn main()
    println(f"{take(Option.None)} {take(Option.Some('x'))}")
"#,
        "none x",
    );
}

#[test]
fn test_patterns_through_the_declared_enum_match_a_short_spelled_subject() {
    assert_heap_guard_output(
        r#"
use system.io

fn show(o int?) String
    match o
        Option.Some(n): f"some {n}"
        Option.None: "none"

fn main()
    println(f"{show(Some(2))} {show(None)}")
"#,
        "some 2 none",
    );
}

#[test]
fn test_bare_patterns_match_a_subject_spelled_through_the_enum() {
    assert_heap_guard_output(
        r#"
use system.io

fn show(o Option<String>) String
    match o
        Some(s): s
        None: "none"

fn main()
    println(f"{show(Some('a'))} {show(None)}")
"#,
        "a none",
    );
}

#[test]
fn test_an_or_pattern_covers_both_variants() {
    assert_heap_guard_output(
        r#"
use system.io

fn show(o int?) String
    match o
        Some(1) | None: "one or none"
        Some(n): f"{n}"

fn main()
    println(f"{show(Some(1))} {show(None)} {show(Some(4))}")
"#,
        "one or none one or none 4",
    );
}

#[test]
fn test_a_nested_variant_pattern_reads_through_the_present_variant() {
    assert_heap_guard_output(
        r#"
use system.io

enum Shape
    Circle(float)
    Square(float)

fn show(o Shape?) String
    match o
        Some(Shape.Circle(r)): f"circle {r}"
        Some(Shape.Square(s)): f"square {s}"
        None: "none"

fn main()
    println(f"{show(Some(Shape.Circle(1.5)))} {show(Some(Shape.Square(2.0)))} {show(None)}")
"#,
        "circle 1.5 square 2.0 none",
    );
}

#[test]
fn test_if_let_binds_the_present_payload_of_a_subject_spelled_through_the_enum() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let count Option<String> = Option.Some("n")
    if let Some(n) = count
        println(n)
    if let Option.Some(m) = count
        println(m)
"#,
        "n\nn",
    );
}

#[test]
fn test_a_match_missing_the_absent_variant_names_it() {
    assert_compiler_error(
        r#"
use system.io

fn show(o Option<int>) int
    match o
        Option.Some(n): n

fn main()
    println(f"{show(Some(1))}")
"#,
        "Missing variants: None",
    );
}

#[test]
fn test_a_match_missing_the_present_variant_names_it() {
    assert_compiler_error(
        r#"
use system.io

fn show(o int?) int
    match o
        Option.None: 0

fn main()
    println(f"{show(Some(1))}")
"#,
        "Missing variants: Some",
    );
}

#[test]
fn test_a_variant_the_optional_does_not_declare_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn show(o int?) int
    match o
        Option.Just(n): n
        Option.None: 0

fn main()
    println(f"{show(Some(1))}")
"#,
        "has no variant 'Just'",
    );
}

#[test]
fn test_a_default_arm_after_a_present_arm_takes_only_what_that_arm_leaves() {
    assert_heap_guard_output(
        r#"
use system.io

fn show(o int?) String
    match o
        Some(m): f"some {m}"
        default: "default"

fn exact(o int?) String
    match o
        Option.Some(1): "one"
        default: "other"

fn main()
    println(f"{show(Some(3))} {show(None)} {exact(Some(1))} {exact(Some(2))} {exact(None)}")
"#,
        "some 3 default one other other",
    );
}

#[test]
fn test_variants_reached_through_the_enum_at_written_arguments_are_the_optional() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let a = Option<int>.Some(3)
    let b = Option<String>.None
    let c int? = Option<int>.None
    println(f"{a} {b} {c} {a ?? 0}")
"#,
        "Some(3) None None 3",
    );
}

#[test]
fn test_the_present_variant_read_as_a_value_is_refused() {
    for spelling in ["Option.Some", "Some"] {
        assert_compiler_error(
            &format!(
                r#"
use system.io

fn main()
    let s = {spelling}
    println(f"{{s(2) ?? 0}}")
"#
            ),
            "the variant `Option.Some` is read as a value, but a variant carrying a payload can \
             only be called",
        );
    }
}

#[test]
fn test_a_payload_variant_of_any_enum_read_as_a_value_is_refused() {
    assert_compiler_error(
        r#"
enum Shape
    Circle(int)
    Dot

fn main()
    let s = Shape.Circle
"#,
        "error[MER_TYP_078]",
    );
}

#[test]
fn test_a_payload_free_variant_read_through_the_enum_is_still_a_value() {
    assert_heap_guard_output(
        r#"
use system.io

enum Shape
    Circle(int)
    Dot

fn main()
    let d = Shape.Dot
    match d
        Shape.Dot: println("dot")
        Shape.Circle(r): println(f"{r}")
"#,
        "dot",
    );
}
