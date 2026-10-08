// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An enum's variants and method signatures are known before any body is
//! checked, so a body written above the enum — a free function or a class
//! method — can build its variants, match on them and call its methods.

use super::utils::*;

#[test]
fn an_earlier_function_builds_and_matches_a_later_enum() {
    assert_runs_with_output(
        r#"
use system.io

fn name(c Color) int
    return match c
        Color.Red: 1
        Color.Blue(n): n

fn make() Color
    return Color.Blue(7)

fn main()
    println(f"{name(make())} {name(Color.Red)}")

enum Color
    Red
    Blue(int)
"#,
        "7 1\n",
    );
}

#[test]
fn an_earlier_class_method_builds_and_matches_a_later_enum() {
    assert_runs_with_output(
        r#"
use system.io

class Painter
    var base int
    fn pick(dark bool) Shade
        if dark
            return Shade.Dark(self.base)
        return Shade.Light
    fn weight(s Shade) int
        return match s
            Shade.Light: 0
            Shade.Dark(n): n * 10

fn main()
    let p = Painter(base: 4)
    println(f"{p.weight(p.pick(true))} {p.weight(p.pick(false))}")

enum Shade
    Light
    Dark(int)
"#,
        "40 0\n",
    );
}

#[test]
fn an_earlier_body_calls_a_method_of_a_later_enum() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    println(f"{Level.High.rank()} {Level.Low.rank()}")

enum Level
    Low
    High

    fn rank() int
        return match self
            Level.Low: 1
            Level.High: 2
"#,
        "2 1\n",
    );
}

#[test]
fn a_later_generic_enum_is_built_and_matched_from_an_earlier_function() {
    assert_runs_with_output(
        r#"
use system.io

fn unwrap_or(m Maybe<int>, fallback int) int
    return match m
        Maybe.Some(v): v
        Maybe.Nothing: fallback

fn main()
    println(f"{unwrap_or(Maybe.Some(3), 9)} {unwrap_or(Maybe.Nothing, 9)}")

enum Maybe<T>
    Some(T)
    Nothing
"#,
        "3 9\n",
    );
}

#[test]
fn a_class_field_initializer_builds_a_later_enum() {
    assert_runs_with_output(
        r#"
use system.io

class Lamp
    var state = Power.On(5)

fn main()
    let l = Lamp()
    let n = match l.state
        Power.On(w): w
        Power.Off: 0
    println(f"{n}")

enum Power
    On(int)
    Off
"#,
        "5\n",
    );
}

#[test]
fn an_enum_method_reads_a_field_of_a_class_declared_below_it() {
    assert_runs_with_output(
        r#"
use system.io

enum Size
    Small
    Large

    fn scaled(c Config) int
        return match self
            Size.Small: c.factor
            Size.Large: c.factor * 10

class Config
    var factor int

fn main()
    let c = Config(factor: 3)
    println(f"{Size.Small.scaled(c)} {Size.Large.scaled(c)}")
"#,
        "3 30\n",
    );
}

#[test]
fn a_missing_variant_of_a_later_enum_is_still_refused() {
    assert_compiler_error(
        r#"
fn make() Color
    return Color.Green

fn main()
    let c = make()

enum Color
    Red
    Blue(int)
"#,
        "Enum 'Color' has no variant 'Green'",
    );
}

#[test]
fn an_enum_declared_twice_is_reported_exactly_once() {
    let output = check_project_report(&[(
        "main.mi",
        concat!(
            "enum Color\n",
            "    Red\n",
            "\n",
            "enum Color\n",
            "    Blue\n",
            "\n",
            "fn main()\n",
            "    let c = Color.Red\n",
        ),
    )]);

    assert_eq!(
        output.matches("Type 'Color' is already defined").count(),
        1,
        "the second declaration is a duplicate, reported once:\n{}",
        output
    );
}
