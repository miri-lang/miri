// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `default` arm takes only what the arms written above it leave. An
//! earlier arm that can refuse a value (a tuple with a literal in it, a
//! variant with a literal payload, a literal, a string) keeps every value it
//! matches, and a value it refuses falls through to `default`.

use super::utils::*;

#[test]
fn test_a_default_after_a_refutable_tuple_arm_takes_only_what_it_refuses() {
    assert_runs_with_output(
        r#"
use system.io

fn g(t (int, int)) String
    match t
        (1, y): f"one {y}"
        default: "d"

fn main()
    println(f"{g((1, 5))} {g((2, 5))}")
"#,
        "one 5 d",
    );
}

#[test]
fn test_a_default_after_a_refutable_variant_arm_takes_only_what_it_refuses() {
    assert_runs_with_output(
        r#"
use system.io

enum E
    A(int)
    B

fn f(e E) String
    match e
        E.A(1): "one"
        default: "d"

fn main()
    println(f"{f(E.A(1))} {f(E.A(2))} {f(E.B)}")
"#,
        "one d d",
    );
}

#[test]
fn test_a_default_after_integer_arms_takes_the_rest() {
    assert_runs_with_output(
        r#"
use system.io

fn f(n int) String
    match n
        1: "one"
        2: "two"
        default: "d"

fn main()
    println(f"{f(1)} {f(2)} {f(3)}")
"#,
        "one two d",
    );
}

#[test]
fn test_a_default_after_string_arms_takes_the_rest() {
    assert_runs_with_output(
        r#"
use system.io

fn f(s String) String
    match s
        "a": "first"
        "b": "second"
        default: "d"

fn main()
    println(f"{f('a')} {f('b')} {f('c')}")
"#,
        "first second d",
    );
}

#[test]
fn test_a_default_after_a_binding_arm_is_never_reached() {
    assert_runs_with_output(
        r#"
use system.io

fn f(t (int, int)) String
    match t
        (x, y): f"{x}{y}"
        default: "d"

fn main()
    println(f"{f((1, 2))}")
"#,
        "12",
    );
}
